// Internal kernel IPC transport. No ordinary stdio stream participates here.
export const IPC_FRAME_PREFIX = "\x1eAGENTOS_IPC:";
const MAX_BYTES = 8 * 1024 * 1024;
const MAX_MESSAGES = 1024;
const TURN_BYTES = 64 * 1024;
const TURN_CALLS = 8;

function ipcError(code, message) {
	return Object.assign(new Error(message), { code });
}

export class IpcTransport {
	channels = new Map();
	ready = new Set();
	poll = null;
	sequence = 0n;
	scheduled = false;
	retired = false;
	constructor(driver) {
		this.driver = driver;
	}

	open(childId, info, hooks) {
		const key = childId ?? "self";
		if (this.channels.has(key))
			throw ipcError("EBUSY", "IPC endpoint already registered");
		const target = { childId, generation: info.generation };
		const channel = {
			key,
			target,
			hooks,
			queue: [],
			bytes: 0,
			retainedBytes: 0,
			offset: 0,
			closed: false,
			draining: false,
			frame: this.driver.Buffer.alloc(65536),
			frameBytes: 0,
			timer: undefined,
			closedResolve: undefined,
			closedPromise: undefined,
		};
		channel.closedPromise = new Promise((resolve) => {
			channel.closedResolve = resolve;
		});
		channel.send = (frame, callback) => {
			if (channel.closed || channel.draining) {
				const error = ipcError(
					"ERR_IPC_CHANNEL_CLOSED",
					"IPC channel is closed",
				);
				if (callback) this.driver.defer(() => callback(error));
				else this.driver.defer(() => hooks.error(error));
				return false;
			}
			const size = this.driver.Buffer.byteLength(frame, "utf8");
			if (
				size > MAX_BYTES ||
				channel.queue.length >= MAX_MESSAGES ||
				channel.retainedBytes + size > MAX_BYTES
			) {
				const error = ipcError(
					"ERR_RESOURCE_BUDGET_EXCEEDED",
					"IPC encoded queue exceeds 1024 messages or 8388608 bytes",
				);
				if (callback) this.driver.defer(() => callback(error));
				else this.driver.defer(() => hooks.error(error));
				return false;
			}
			const buffer = this.driver.Buffer.from(frame, "utf8");
			let written = 0;
			if (channel.queue.length === 0) {
				try {
					const chunk = buffer.subarray(0, TURN_BYTES);
					written = this.driver.write(channel.target, chunk).written;
					if (
						!Number.isInteger(written) ||
						written < 0 ||
						written > chunk.length
					)
						throw ipcError(
							"ERR_IPC_MESSAGE_FAILED",
							"invalid IPC write completion",
						);
				} catch (error) {
					if (callback) this.driver.defer(() => callback(error));
					else this.driver.defer(() => hooks.error(error));
					return false;
				}
				// Like Node, a small send can reach the kernel before process.exit().
				// Only the callback is deferred. Partial writes retain their offset.
				if (written === size) {
					if (callback) this.driver.defer(() => callback(null));
					return true;
				}
			}
			channel.queue.push({ bytes: buffer, callback });
			if (channel.queue.length === 1) channel.offset = written;
			channel.bytes += size - written;
			channel.retainedBytes += size;
			this.driver.refWrite?.(key, (error) => channel.abort(error));
			this.ready.add(key);
			this.wake();
			// false with an accepted entry is backpressure; rejected enqueue above
			// instead settles its callback with the original typed failure.
			return channel.bytes < 65536;
		};
		channel.disconnect = () => {
			if (channel.closed || channel.draining) return;
			channel.draining = true;
			if (channel.queue.length === 0) {
				this.close(channel);
				return;
			}
			channel.timer = this.driver.delay(
				() =>
					this.close(
						channel,
						ipcError(
							"ERR_AGENTOS_IPC_DRAIN_TIMEOUT",
							"IPC disconnect drain exceeded its lifecycle deadline",
						),
					),
				hooks.drainMs,
			);
			this.ready.add(key);
			this.wake();
		};
		channel.afterPeerExit = () => {
			if (!channel.closed && !channel.timer) {
				this.failQueued(
					channel,
					ipcError("ERR_IPC_CHANNEL_CLOSED", "IPC peer exited"),
				);
				channel.timer = this.driver.delay(
					() =>
						this.close(
							channel,
							ipcError(
								"ERR_AGENTOS_IPC_DRAIN_TIMEOUT",
								"IPC peer EOF exceeded its lifecycle deadline",
							),
						),
					hooks.drainMs,
				);
				this.ready.add(key);
				this.wake();
			}
			return channel.closedPromise;
		};
		channel.abort = (error) => this.close(channel, error);
		this.channels.set(key, channel);
		this.ready.add(key);
		this.wake();
		return channel;
	}

	interests() {
		return [...this.channels.values()].map((channel) => ({
			...channel.target,
			writable: channel.queue.length > 0,
		}));
	}

	wake() {
		if (this.retired) return;
		if (this.poll) {
			if (this.channels.size === 0) return; // Native close wakes the old waiter.
			try {
				const value = this.driver.update(
					this.poll.id,
					this.poll.revision,
					this.interests(),
				);
				this.poll.revision = value.revision;
			} catch (error) {
				// A completed poll can race a new send. Its completion schedules the
				// ready work below; it cannot authorize a replacement channel.
				if (error.code !== "ESTALE") this.failAll(error);
			}
			return;
		}
		if (this.scheduled) return;
		this.scheduled = true;
		this.driver.defer(() => {
			this.scheduled = false;
			this.pump();
		});
	}

	pump() {
		if (this.retired || this.poll) return;
		let calls = 0,
			bytes = 0;
		for (const key of [...this.ready]) {
			if (calls >= TURN_CALLS || bytes >= TURN_BYTES) break;
			const channel = this.channels.get(key);
			this.ready.delete(key);
			if (!channel || channel.closed) continue;
			try {
				const input = this.driver.read(
					channel.target,
					Math.min(4096, TURN_BYTES - bytes),
				);
				calls++;
				if (input?.done) {
					if (channel.frameBytes)
						throw ipcError(
							"ERR_IPC_MESSAGE_FAILED",
							"IPC peer closed an incomplete frame",
						);
					this.close(channel);
					continue;
				}
				if (input?.dataBase64) {
					const chunk = this.driver.Buffer.from(input.dataBase64, "base64");
					bytes += chunk.length;
					this.receive(channel, chunk);
					this.ready.add(key);
				}
				if (channel.queue.length && calls < TURN_CALLS && bytes < TURN_BYTES) {
					const entry = channel.queue[0];
					const chunk = entry.bytes.subarray(
						channel.offset,
						Math.min(entry.bytes.length, channel.offset + TURN_BYTES - bytes),
					);
					const result = this.driver.write(channel.target, chunk);
					calls++;
					if (
						!Number.isInteger(result.written) ||
						result.written < 0 ||
						result.written > chunk.length
					)
						throw ipcError(
							"ERR_IPC_MESSAGE_FAILED",
							"invalid IPC write completion",
						);
					channel.offset += result.written;
					channel.bytes -= result.written;
					bytes += result.written;
					if (channel.offset === entry.bytes.length) {
						channel.queue.shift();
						channel.offset = 0;
						channel.retainedBytes -= entry.bytes.length;
						if (channel.queue.length === 0) this.driver.unrefWrite?.(key);
						if (entry.callback) this.driver.defer(() => entry.callback(null));
					}
					if (result.written > 0) this.ready.add(key);
				}
				if (channel.draining && channel.queue.length === 0) this.close(channel);
			} catch (error) {
				this.close(channel, error);
			}
		}
		if (this.ready.size) {
			this.wake();
			return;
		}
		if (!this.channels.size) return;
		const poll = { id: String(++this.sequence), revision: "0" };
		this.poll = poll;
		Promise.resolve(this.driver.poll(poll.id, this.interests())).then(
			(result) => {
				if (this.poll !== poll) return;
				this.poll = null;
				for (const entry of result.ready) {
					const key = entry.childId ?? "self",
						channel = this.channels.get(key);
					if (channel?.target.generation === entry.generation)
						this.ready.add(key);
				}
				this.wake();
			},
			(error) => {
				if (this.poll !== poll) return;
				this.poll = null;
				if (this.channels.size) this.failAll(error);
			},
		);
	}

	receive(channel, chunk) {
		let offset = 0;
		while (offset < chunk.length) {
			const newline = chunk.indexOf(10, offset);
			const end = newline < 0 ? chunk.length : newline + 1;
			const required = channel.frameBytes + end - offset;
			if (required > MAX_BYTES)
				throw ipcError(
					"ERR_RESOURCE_BUDGET_EXCEEDED",
					"IPC encoded frame exceeds 8388608 bytes",
				);
			if (required > channel.frame.length) {
				const buffer = this.driver.Buffer.alloc(
					Math.min(MAX_BYTES, Math.max(required, channel.frame.length * 2)),
				);
				channel.frame.copy(buffer, 0, 0, channel.frameBytes);
				channel.frame = buffer;
			}
			channel.frame.set(chunk.subarray(offset, end), channel.frameBytes);
			channel.frameBytes = required;
			offset = end;
			if (newline >= 0) {
				const frame = channel.frame.toString("utf8", 0, channel.frameBytes);
				if (!frame.startsWith(IPC_FRAME_PREFIX))
					throw ipcError("ERR_IPC_MESSAGE_FAILED", "invalid private IPC frame");
				const encodedBytes = channel.frameBytes;
				channel.frameBytes = 0;
				channel.hooks.message(
					frame.slice(IPC_FRAME_PREFIX.length, -1),
					encodedBytes,
				);
			}
		}
	}

	failQueued(channel, error) {
		for (const entry of channel.queue.splice(0)) {
			if (entry.callback) this.driver.defer(() => entry.callback(error));
			else this.driver.defer(() => channel.hooks.error(error));
		}
		channel.bytes = 0;
		channel.retainedBytes = 0;
		channel.offset = 0;
		this.driver.unrefWrite?.(channel.key);
	}

	close(channel, error, abandon = false) {
		if (channel.closed) return;
		channel.closed = true;
		this.driver.clearDelay(channel.timer);
		const hadQueued = channel.queue.length > 0;
		this.failQueued(
			channel,
			error ?? ipcError("ERR_IPC_CHANNEL_CLOSED", "IPC peer disconnected"),
		);
		this.channels.delete(channel.key);
		this.ready.delete(channel.key);
		channel.frame = this.driver.Buffer.alloc(0);
		channel.frameBytes = 0;
		if (!abandon)
			try {
				this.driver.close(channel.target);
			} catch (closeError) {
				if (closeError.code !== "ESTALE" && closeError.code !== "EBADF")
					error ??= closeError;
			}
		if (error && !hadQueued)
			this.driver.defer(() => channel.hooks.error(error));
		this.driver.defer(() => {
			try {
				channel.hooks.close();
			} finally {
				channel.closedResolve();
			}
		});
		this.wake();
	}

	failAll(error) {
		for (const channel of [...this.channels.values()])
			this.close(channel, error);
	}
	abandon(error) {
		this.retired = true;
		// Snapshot/execute retirement has already canceled the old native calls.
		// Do not schedule old user callbacks in a replacement execution.
		const failures = [];
		for (const channel of this.channels.values()) {
			channel.closed = true;
			this.driver.clearDelay(channel.timer);
			try {
				this.driver.close(channel.target);
			} catch (closeError) {
				if (!["ESTALE", "EBADF", "ESRCH"].includes(closeError.code))
					failures.push(closeError);
			}
			channel.queue.length = 0;
			channel.bytes = 0;
			channel.retainedBytes = 0;
			channel.frame = this.driver.Buffer.alloc(0);
			channel.frameBytes = 0;
			this.driver.unrefWrite?.(channel.key);
		}
		this.channels.clear();
		this.ready.clear();
		if (failures.length)
			throw new AggregateError(
				failures,
				"IPC retirement could not close its owned descriptors",
			);
	}
}
