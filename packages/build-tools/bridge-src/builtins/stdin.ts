import {
	exposeCustomGlobal,
	exposeMutableRuntimeStateGlobal,
} from "../global-exposure.js";
import { TextDecoder } from "../polyfills/index.js";
import { stringDecoderStdlibModuleNs } from "../prelude.js";
import {
	getAsyncContextFrame,
	runInAsyncContextFrame,
} from "./async-context.js";
import { import_buffer2 } from "./buffer-runtime.js";
import { once } from "./events.js";
import {
	_getStdinIsTTY,
	isProcessExitError,
	routeAsyncCallbackError,
	scheduleAsyncRethrow,
} from "./process.js";

var _stdinListeners = {};

var _stdinOnceListeners = {};

var _stdinLiveDecoder = new TextDecoder();

var STDIN_HANDLE_ID = "process.stdin";

var _stdinLiveBuffer = "";

var _stdinLiveStarted = false;

var _stdinReadEpoch = 0;

// Default pull-mode prefetch stays bounded even if a consumer reads only one
// byte per readable event. Larger explicit read(n) requests have Node's ceiling.
const STDIN_READ_HIGH_WATER_MARK = 64 * 1024;
const STDIN_MAX_READ_SIZE = 1024 * 1024 * 1024;
var _stdinReadDemand = 0;
var _stdinReadableScheduled = false;

function hasStdinReadableListeners() {
	return (
		(_stdinListeners.readable?.length || 0) +
			(_stdinOnceListeners.readable?.length || 0) >
		0
	);
}

function hasLiveStdinDemand() {
	return (
		(getStdinFlowMode() && !_stdin.paused) ||
		_stdinReadDemand > _stdinLiveBuffer.length
	);
}

function requestLiveStdinRead(size = STDIN_READ_HIGH_WATER_MARK) {
	if (getStdinEnded()) return;
	_stdinReadDemand = Math.max(
		_stdinReadDemand,
		STDIN_READ_HIGH_WATER_MARK,
		size,
	);
	ensureLiveStdinStarted();
}

function notifyStdinReadable() {
	if (_stdinReadableScheduled || !hasStdinReadableListeners()) return;
	_stdinReadableScheduled = true;
	const epoch = _stdinReadEpoch;
	queueMicrotask(() => {
		if (epoch !== _stdinReadEpoch) return;
		_stdinReadableScheduled = false;
		if (_stdinLiveBuffer.length > 0 || getStdinEnded()) {
			emitStdinListeners("readable");
			if (
				!hasStdinReadableListeners() &&
				(_stdinListeners.data?.length || 0) +
					(_stdinOnceListeners.data?.length || 0) >
					0
			) {
				_stdin.resume();
			}
		}
	});
}

function liveStdinBytesArrived() {
	if (_stdinLiveBuffer.length >= _stdinReadDemand) _stdinReadDemand = 0;
	notifyStdinReadable();
	flushLiveStdinBuffer();
}

var _stdinAsyncContextFrame = undefined;

var _stdinAsyncContextFrameCaptured = false;

var _stdinLiveHandleRegistered = false;

var _stdinLiveTerminalEventsScheduled = false;

var _stdinLiveTerminalEventsEmitted = false;

exposeMutableRuntimeStateGlobal(
	"_stdinData",
	(typeof _processConfig !== "undefined" && _processConfig.stdin) || "",
);

exposeMutableRuntimeStateGlobal("_stdinPosition", 0);

exposeMutableRuntimeStateGlobal("_stdinEnded", false);

exposeMutableRuntimeStateGlobal("_stdinFlowMode", false);

function getStdinData() {
	return globalThis._stdinData;
}

function setStdinDataValue(v) {
	globalThis._stdinData = v;
}

function getStdinPosition() {
	return globalThis._stdinPosition;
}

function setStdinPosition(v) {
	globalThis._stdinPosition = v;
}

function getStdinEnded() {
	return globalThis._stdinEnded;
}

function setStdinEnded(v) {
	globalThis._stdinEnded = v;
}

function getStdinFlowMode() {
	return globalThis._stdinFlowMode;
}

function setStdinFlowMode(v) {
	globalThis._stdinFlowMode = v;
}

// Retained contexts preserve the stream, but an operation's parked read must
// not keep its pump active after the native resolver has been retired.
function retireStdinOperation() {
	_stdinReadEpoch++;
	_stdinReadDemand = 0;
	_stdinReadableScheduled = false;
	_stdinLiveStarted = false;
	_stdin.paused = true;
	setStdinFlowMode(false);
	syncLiveStdinHandle(false);
}

exposeCustomGlobal("_retireStdinOperation", retireStdinOperation);

function resetLiveStdinState(decoder) {
	_stdinReadEpoch++;
	_stdinReadDemand = 0;
	_stdinReadableScheduled = false;
	_stdinLiveBuffer = "";
	_stdinLiveStarted = false;
	_stdinLiveDecoder = decoder;
	_stdinLiveTerminalEventsScheduled = false;
	_stdinLiveTerminalEventsEmitted = false;
}

function _emitStdinData() {
	if (getStdinEnded() || !getStdinData()) return;
	if (getStdinFlowMode() && getStdinPosition() < getStdinData().length) {
		const chunk = getStdinData().slice(getStdinPosition());
		setStdinPosition(getStdinData().length);
		const dataListeners = [
			...(_stdinListeners["data"] || []),
			...(_stdinOnceListeners["data"] || []),
		];
		_stdinOnceListeners["data"] = [];
		for (const listener of dataListeners) {
			listener(chunk);
		}
		setStdinEnded(true);
		const endListeners = [
			...(_stdinListeners["end"] || []),
			...(_stdinOnceListeners["end"] || []),
		];
		_stdinOnceListeners["end"] = [];
		for (const listener of endListeners) {
			listener();
		}
		const closeListeners = [
			...(_stdinListeners["close"] || []),
			...(_stdinOnceListeners["close"] || []),
		];
		_stdinOnceListeners["close"] = [];
		for (const listener of closeListeners) {
			listener();
		}
	}
}

function emitStdinListeners(event, value) {
	const listeners = [
		...(_stdinListeners[event] || []),
		...(_stdinOnceListeners[event] || []),
	];
	_stdinOnceListeners[event] = [];
	for (const listener of listeners) {
		try {
			listener(value);
		} catch (error) {
			const outcome = routeAsyncCallbackError(error);
			if (!outcome.handled) {
				if (isProcessExitError(outcome.rethrow)) {
					scheduleAsyncRethrow(outcome.rethrow);
					return true;
				}
				throw outcome.rethrow;
			}
			return true;
		}
	}
	return listeners.length > 0;
}

function syncLiveStdinHandle(active) {
	if (active) {
		if (!_stdinLiveHandleRegistered && typeof _registerHandle === "function") {
			_registerHandle(STDIN_HANDLE_ID, "process.stdin");
			_stdinLiveHandleRegistered = true;
		}
		return;
	}
	if (_stdinLiveHandleRegistered && typeof _unregisterHandle === "function") {
		try {
			_unregisterHandle(STDIN_HANDLE_ID);
		} catch {}
		_stdinLiveHandleRegistered = false;
	}
}

function configureLiveStdin(active, eager = false) {
	globalThis.__runtimeStreamStdin = !!active;
	syncLiveStdinHandle(!!active && !!eager && !getStdinEnded());
}

exposeCustomGlobal("__runtimeConfigureStreamStdin", configureLiveStdin);

function flushLiveStdinBuffer() {
	if (!getStdinFlowMode() || _stdinLiveBuffer.length === 0) return;
	const chunk = _stdinLiveBuffer;
	_stdinLiveBuffer = "";
	const data = _stdin.encoding
		? chunk
		: import_buffer2.Buffer.from(chunk, "latin1");
	emitStdinListeners("data", data);
	maybeEmitLiveStdinTerminalEvents();
}

function maybeEmitLiveStdinTerminalEvents() {
	if (
		!getStdinEnded() ||
		_stdinLiveTerminalEventsEmitted ||
		_stdinLiveBuffer.length > 0
	) {
		return;
	}
	if (_stdinLiveTerminalEventsScheduled) {
		return;
	}
	_stdinLiveTerminalEventsScheduled = true;
	queueMicrotask(() => {
		_stdinLiveTerminalEventsScheduled = false;
		if (
			!getStdinEnded() ||
			_stdinLiveTerminalEventsEmitted ||
			_stdinLiveBuffer.length > 0
		) {
			return;
		}
		_stdinLiveTerminalEventsEmitted = true;
		emitStdinListeners("end");
		emitStdinListeners("close");
		syncLiveStdinHandle(false);
	});
}

function finishLiveStdin() {
	if (getStdinEnded()) return;
	setStdinEnded(true);
	if (_stdin.encoding) _stdinLiveBuffer += _stdinLiveDecoder.decode();
	_stdinReadDemand = 0;
	notifyStdinReadable();
	flushLiveStdinBuffer();
	maybeEmitLiveStdinTerminalEvents();
}

function _getStreamStdin() {
	return typeof __runtimeStreamStdin !== "undefined" && !!__runtimeStreamStdin;
}

function _getKernelStdin() {
	return typeof __runtimeKernelStdin !== "undefined" && !!__runtimeKernelStdin;
}

function ensureLiveStdinStarted() {
	if (!hasLiveStdinDemand() || getStdinEnded()) return;
	if (!_getStdinIsTTY() && !_getStreamStdin() && !_getKernelStdin()) return;
	// A paused in-flight read is reused, but renewed read() demand must also
	// restore its active handle before returning to the caller.
	syncLiveStdinHandle(true);
	if (_stdinLiveStarted) return;
	_stdinLiveStarted = true;
	if (_getStreamStdin() && !_getKernelStdin()) {
		return;
	}
	if (typeof _kernelStdinRead === "undefined") return;
	const readEpoch = _stdinReadEpoch;
	void runInAsyncContextFrame(
		_stdinAsyncContextFrame,
		async () => {
			try {
				let finished = false;
				try {
					while (
						readEpoch === _stdinReadEpoch &&
						!getStdinEnded() &&
						hasLiveStdinDemand()
					) {
						if (typeof _kernelStdinRead === "undefined") {
							break;
						}
						let next: { done?: boolean; dataBase64?: string } | undefined;
						try {
							const readSize = getStdinFlowMode()
								? STDIN_READ_HIGH_WATER_MARK
								: Math.min(
										STDIN_READ_HIGH_WATER_MARK,
										_stdinReadDemand - _stdinLiveBuffer.length,
									);
							next = await _kernelStdinRead.apply(void 0, [readSize, null], {
								result: { promise: true },
							});
						} catch (error) {
							if (readEpoch !== _stdinReadEpoch) return;
							if (
								error &&
								typeof error === "object" &&
								error.code === "EINTR"
							) {
								// Node/libuv retries interrupted stream reads after running the
								// caught signal handler. EINTR must not become stdin EOF.
								continue;
							}
							throw error;
						}
						if (readEpoch !== _stdinReadEpoch) return;
						if (next?.done) {
							finished = true;
							break;
						}
						const dataBase64 = String(next?.dataBase64 ?? "");
						if (!dataBase64) {
							continue;
						}
						const bytes = import_buffer2.Buffer.from(dataBase64, "base64");
						_stdinLiveBuffer += _stdin.encoding
							? _stdinLiveDecoder.decode(bytes, { stream: true })
							: bytes.toString("latin1");
						liveStdinBytesArrived();
					}
				} catch (error) {
					if (readEpoch !== _stdinReadEpoch) return;
					finished = true;
					const normalized =
						error instanceof Error ? error : new Error(String(error));
					const handled = emitStdinListeners("error", normalized);
					if (!handled) {
						scheduleAsyncRethrow(normalized);
					}
				}
				// Pausing releases read demand, not the stream or its decoder.
				if (readEpoch !== _stdinReadEpoch) return;
				if (finished || getStdinEnded()) {
					finishLiveStdin();
				}
			} finally {
				if (readEpoch === _stdinReadEpoch) {
					_stdinLiveStarted = false;
					if (!hasLiveStdinDemand()) syncLiveStdinHandle(false);
				}
			}
		},
		undefined,
		[],
	);
}

// Node makes the stdin handle on the first read of process.stdin, and stdin events
// keep that context.
function getProcessStdin() {
	if (!_stdinAsyncContextFrameCaptured) {
		_stdinAsyncContextFrameCaptured = true;
		_stdinAsyncContextFrame = getAsyncContextFrame();
	}
	return _stdin;
}

function stdinDispatch(eventType, payload) {
	return runInAsyncContextFrame(
		_stdinAsyncContextFrame,
		deliverStdinEvent,
		undefined,
		[eventType, payload],
	);
}

function deliverStdinEvent(eventType, payload) {
	if (eventType === "stdin_end") {
		finishLiveStdin();
		return;
	}
	if (eventType !== "stdin" || getStdinEnded()) {
		return;
	}
	const bytes =
		payload &&
		typeof payload === "object" &&
		typeof payload.dataBase64 === "string"
			? import_buffer2.Buffer.from(payload.dataBase64, "base64")
			: import_buffer2.Buffer.from(payload == null ? "" : payload);
	if (bytes.length === 0) return;
	_stdinLiveBuffer += _stdin.encoding
		? _stdinLiveDecoder.decode(bytes, { stream: true })
		: bytes.toString("latin1");
	liveStdinBytesArrived();
}

var _stdin = {
	fd: 0,
	readable: true,
	paused: true,
	encoding: null,
	isRaw: false,
	read(size) {
		// Match Readable.read's integer coercion, including NaN meaning "all".
		size = Number.parseInt(size, 10);
		if (Number.isNaN(size)) size = undefined;
		if (size !== undefined && size > STDIN_MAX_READ_SIZE) {
			const error = new RangeError("read size must not exceed 1073741824");
			error.code = "ERR_OUT_OF_RANGE";
			throw error;
		}
		if (size !== undefined && size <= 0) {
			if (_stdinLiveBuffer.length === 0) requestLiveStdinRead();
			maybeEmitLiveStdinTerminalEvents();
			return null;
		}
		const available = _stdinLiveBuffer.length;
		if (
			available > 0 &&
			(size === undefined || size <= available || getStdinEnded())
		) {
			const count = size === undefined ? available : Math.min(size, available);
			const chunk = _stdinLiveBuffer.slice(0, count);
			_stdinLiveBuffer = _stdinLiveBuffer.slice(count);
			_stdinReadDemand = 0;
			const value = this.encoding
				? chunk
				: import_buffer2.Buffer.from(chunk, "latin1");
			const epoch = _stdinReadEpoch;
			emitStdinListeners("data", value);
			// Consuming even part of a chunk renews prefetch up to the fixed
			// watermark. Never request bufferedLength + one host chunk: that
			// would grow without bound for one-byte-per-event consumers.
			if (epoch === _stdinReadEpoch) {
				requestLiveStdinRead();
			}
			maybeEmitLiveStdinTerminalEvents();
			return value;
		}
		if (_getStdinIsTTY() || _getStreamStdin() || _getKernelStdin()) {
			requestLiveStdinRead(size || 1);
			maybeEmitLiveStdinTerminalEvents();
			return null;
		}
		if (getStdinPosition() >= getStdinData().length) return null;
		const chunk = size
			? getStdinData().slice(getStdinPosition(), getStdinPosition() + size)
			: getStdinData().slice(getStdinPosition());
		setStdinPosition(getStdinPosition() + chunk.length);
		return this.encoding ? chunk : import_buffer2.Buffer.from(chunk);
	},
	on(event, listener) {
		if (!_stdinListeners[event]) _stdinListeners[event] = [];
		_stdinListeners[event].push(listener);
		if (event === "readable") {
			this.paused = true;
			setStdinFlowMode(false);
			if (_stdinLiveBuffer.length > 0 || getStdinEnded()) notifyStdinReadable();
			else requestLiveStdinRead();
		}
		if (
			(_getStdinIsTTY() || _getStreamStdin() || _getKernelStdin()) &&
			(event === "data" || event === "end" || event === "close")
		) {
			ensureLiveStdinStarted();
		}
		if (event === "data" && this.paused && !hasStdinReadableListeners()) {
			this.resume();
		}
		if (
			(event === "end" || event === "close") &&
			(_getStdinIsTTY() || _getStreamStdin() || _getKernelStdin())
		) {
			maybeEmitLiveStdinTerminalEvents();
		}
		if (event === "end" && getStdinData() && !getStdinEnded()) {
			setStdinFlowMode(true);
			_emitStdinData();
		}
		return this;
	},
	once(event, listener) {
		if (!_stdinOnceListeners[event]) _stdinOnceListeners[event] = [];
		_stdinOnceListeners[event].push(listener);
		if (event === "readable") {
			this.paused = true;
			setStdinFlowMode(false);
			if (_stdinLiveBuffer.length > 0 || getStdinEnded()) notifyStdinReadable();
			else requestLiveStdinRead();
		}
		if (
			(_getStdinIsTTY() || _getStreamStdin() || _getKernelStdin()) &&
			(event === "data" || event === "end" || event === "close")
		) {
			ensureLiveStdinStarted();
		}
		if (event === "data" && this.paused && !hasStdinReadableListeners()) {
			this.resume();
		}
		if (
			(event === "end" || event === "close") &&
			(_getStdinIsTTY() || _getStreamStdin() || _getKernelStdin())
		) {
			maybeEmitLiveStdinTerminalEvents();
		}
		if (event === "end" && getStdinData() && !getStdinEnded()) {
			setStdinFlowMode(true);
			_emitStdinData();
		}
		return this;
	},
	off(event, listener) {
		for (const table of [_stdinListeners, _stdinOnceListeners]) {
			const idx = table[event]?.lastIndexOf(listener) ?? -1;
			if (idx !== -1) {
				table[event].splice(idx, 1);
				break;
			}
		}
		if (event === "readable" && !hasStdinReadableListeners()) {
			_stdinReadDemand = 0;
			if (
				(_stdinListeners.data?.length || 0) +
					(_stdinOnceListeners.data?.length || 0) >
				0
			)
				this.resume();
		}
		return this;
	},
	removeListener(event, listener) {
		return this.off(event, listener);
	},
	emit(event, ...args) {
		const listeners = [
			...(_stdinListeners[event] || []),
			...(_stdinOnceListeners[event] || []),
		];
		_stdinOnceListeners[event] = [];
		for (const listener of listeners) {
			listener(args[0]);
		}
		return listeners.length > 0;
	},
	pause() {
		_stdinReadDemand = 0;
		this.paused = true;
		setStdinFlowMode(false);
		syncLiveStdinHandle(false);
		return this;
	},
	resume() {
		if (hasStdinReadableListeners()) {
			requestLiveStdinRead();
			return this;
		}
		this.paused = false;
		setStdinFlowMode(true);
		flushLiveStdinBuffer();
		_emitStdinData();
		maybeEmitLiveStdinTerminalEvents();
		if (
			!this.paused &&
			!getStdinEnded() &&
			(_getStdinIsTTY() || _getStreamStdin() || _getKernelStdin())
		) {
			ensureLiveStdinStarted();
			syncLiveStdinHandle(true);
		}
		return this;
	},
	setEncoding(enc) {
		const StringDecoder =
			stringDecoderStdlibModuleNs.StringDecoder ??
			stringDecoderStdlibModuleNs.default?.StringDecoder;
		const decoder = new StringDecoder(enc);
		_stdinLiveDecoder = {
			decode(bytes, options) {
				return options?.stream ? decoder.write(bytes) : decoder.end(bytes);
			},
		};
		if (!this.encoding && _stdinLiveBuffer.length > 0) {
			_stdinLiveBuffer = _stdinLiveDecoder.decode(
				import_buffer2.Buffer.from(_stdinLiveBuffer, "latin1"),
				{ stream: !getStdinEnded() },
			);
		}
		this.encoding = decoder.encoding;
		return this;
	},
	setRawMode(mode) {
		if (!_getStdinIsTTY()) {
			throw new Error("setRawMode is not supported when stdin is not a TTY");
		}
		if (typeof _ptySetRawMode !== "undefined") {
			_ptySetRawMode.applySync(void 0, [mode]);
		}
		this.isRaw = mode;
		return this;
	},
	get isTTY() {
		return _getStdinIsTTY();
	},
	[Symbol.asyncIterator]: function () {
		const stream = this;
		const queuedChunks = [];
		const pendingResolves = [];
		let done = false;
		let error = null;
		const flush = () => {
			while (pendingResolves.length > 0) {
				if (error) {
					pendingResolves.shift()(Promise.reject(error));
					continue;
				}
				if (queuedChunks.length > 0) {
					pendingResolves.shift()(
						Promise.resolve({ done: false, value: queuedChunks.shift() }),
					);
					continue;
				}
				if (done) {
					pendingResolves.shift()(
						Promise.resolve({ done: true, value: void 0 }),
					);
					continue;
				}
				break;
			}
		};
		const onData = (chunk) => {
			queuedChunks.push(chunk);
			flush();
		};
		const onEnd = () => {
			done = true;
			flush();
		};
		const onError = (reason) => {
			error = reason;
			done = true;
			flush();
		};
		stream.on("end", onEnd);
		stream.on("close", onEnd);
		stream.on("error", onError);
		stream.on("data", onData);
		stream.resume();
		return {
			next() {
				if (error) {
					return Promise.reject(error);
				}
				if (queuedChunks.length > 0) {
					return Promise.resolve({ done: false, value: queuedChunks.shift() });
				}
				if (done) {
					return Promise.resolve({ done: true, value: void 0 });
				}
				return new Promise((resolve) => {
					pendingResolves.push(resolve);
				});
			},
			return() {
				done = true;
				stream.off?.("data", onData);
				stream.off?.("end", onEnd);
				stream.off?.("close", onEnd);
				stream.off?.("error", onError);
				flush();
				return Promise.resolve({ done: true, value: void 0 });
			},
			[Symbol.asyncIterator]() {
				return this;
			},
		};
	},
};

export {
	_emitStdinData,
	_getKernelStdin,
	_getStreamStdin,
	_stdin,
	_stdinListeners,
	_stdinLiveBuffer,
	_stdinLiveDecoder,
	_stdinLiveHandleRegistered,
	_stdinLiveStarted,
	_stdinLiveTerminalEventsEmitted,
	_stdinLiveTerminalEventsScheduled,
	_stdinOnceListeners,
	configureLiveStdin,
	emitStdinListeners,
	ensureLiveStdinStarted,
	finishLiveStdin,
	flushLiveStdinBuffer,
	getProcessStdin,
	getStdinData,
	getStdinEnded,
	getStdinFlowMode,
	getStdinPosition,
	maybeEmitLiveStdinTerminalEvents,
	resetLiveStdinState,
	retireStdinOperation,
	STDIN_HANDLE_ID,
	setStdinDataValue,
	setStdinEnded,
	setStdinFlowMode,
	setStdinPosition,
	stdinDispatch,
	syncLiveStdinHandle,
};
