import {
	intrinsicArrayBufferBytes,
	intrinsicArrayBufferViewBytes,
} from "../builtins/buffer-source.js";

// Web compression uses the guest's existing zlib implementation. Stream queues
// pause the Node transform when full, so decompression does not drain into an
// unbounded array of chunks while its Web reader is idle.
const compressionSlots = new WeakMap();
const decompressionSlots = new WeakMap();

function compressionPair(format, decompress) {
	const name = `${format}`;
	const methods = decompress
		? {
				gzip: "createGunzip",
				deflate: "createInflate",
				"deflate-raw": "createInflateRaw",
			}
		: {
				gzip: "createGzip",
				deflate: "createDeflate",
				"deflate-raw": "createDeflateRaw",
			};
	if (!Object.hasOwn(methods, name)) {
		const error = new TypeError(
			`The argument 'format' is invalid. Received '${name}'`,
		);
		error.code = "ERR_INVALID_ARG_VALUE";
		throw error;
	}
	const zlib = globalThis.__agentOsBuiltinZlibModule;
	const stream = zlib[methods[name]]();
	let readEnded = false,
		writeEnded = false;
	let readController: ReadableStreamDefaultController<Uint8Array>;
	let writeController: WritableStreamDefaultController;
	const abortError = () =>
		Object.assign(new Error("The operation was aborted"), {
			name: "AbortError",
			code: "ABORT_ERR",
		});
	const fail = (error) => {
		if (!readEnded) {
			readEnded = true;
			readController.error(error);
		}
		if (!writeEnded) {
			writeEnded = true;
			writeController.error(error);
		}
	};
	const readable = new ReadableStream({
		start(controller) {
			readController = controller;
			stream.on("data", (chunk) => {
				if (readEnded) return;
				controller.enqueue(
					new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength),
				);
				if (controller.desiredSize <= 0) stream.pause();
			});
			stream.once("end", () => {
				if (!readEnded) {
					readEnded = true;
					controller.close();
				}
			});
			stream.pause();
		},
		pull() {
			stream.resume();
		},
		cancel(reason) {
			readEnded = true;
			stream.destroy(reason instanceof Error ? reason : abortError());
		},
	});
	const writable = new WritableStream({
		start(controller) {
			writeController = controller;
		},
		async write(chunk) {
			try {
				const bytes =
					intrinsicArrayBufferBytes(chunk) ??
					intrinsicArrayBufferViewBytes(chunk);
				if (!bytes)
					throw Object.assign(
						new TypeError(
							"The chunk argument must be an ArrayBuffer or ArrayBufferView",
						),
						{ code: "ERR_INVALID_ARG_TYPE" },
					);
				await new Promise((resolve, reject) =>
					stream.write(bytes, (error) => (error ? reject(error) : resolve())),
				);
			} catch (error) {
				// Validation and synchronous/callback write failures must settle
				// the independent readable side with the same original failure.
				fail(error);
				stream.destroy(error);
				throw error;
			}
		},
		close() {
			return new Promise((resolve, reject) => {
				stream.once("error", reject);
				stream.end(resolve);
			});
		},
		abort(reason) {
			stream.destroy(reason instanceof Error ? reason : abortError());
		},
	});
	stream.once("error", fail);
	stream.once("finish", () => {
		writeEnded = true;
	});
	stream.once("close", () => {
		if (!readEnded || !writeEnded)
			fail(
				Object.assign(new Error("Premature close"), {
					code: "ERR_STREAM_PREMATURE_CLOSE",
				}),
			);
	});
	return { readable, writable };
}

function compressionSlot(slots, receiver) {
	const pair = slots.get(receiver);
	if (!pair) {
		const error = new TypeError(
			"Value of this must be of type CompressionStream or DecompressionStream",
		);
		error.code = "ERR_INVALID_THIS";
		throw error;
	}
	return pair;
}

class CompressionStream {
	constructor(format) {
		compressionSlots.set(this, compressionPair(format, false));
	}
	get readable() {
		return compressionSlot(compressionSlots, this).readable;
	}
	get writable() {
		return compressionSlot(compressionSlots, this).writable;
	}
}
class DecompressionStream {
	constructor(format) {
		decompressionSlots.set(this, compressionPair(format, true));
	}
	get readable() {
		return compressionSlot(decompressionSlots, this).readable;
	}
	get writable() {
		return compressionSlot(decompressionSlots, this).writable;
	}
}
Object.defineProperty(CompressionStream.prototype, Symbol.toStringTag, {
	value: "CompressionStream",
	configurable: true,
});
Object.defineProperty(DecompressionStream.prototype, Symbol.toStringTag, {
	value: "DecompressionStream",
	configurable: true,
});
globalThis.CompressionStream = CompressionStream;
globalThis.DecompressionStream = DecompressionStream;
