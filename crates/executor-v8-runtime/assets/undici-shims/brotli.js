import { Transform } from "node:stream";

export function installBrotli(zlib, Buffer) {
	Object.assign(zlib.constants, {
		BROTLI_OPERATION_PROCESS: 0, BROTLI_OPERATION_FLUSH: 1, BROTLI_OPERATION_FINISH: 2,
		BROTLI_PARAM_MODE: 0, BROTLI_PARAM_QUALITY: 1, BROTLI_PARAM_LGWIN: 2,
		BROTLI_MODE_GENERIC: 0, BROTLI_MODE_TEXT: 1, BROTLI_MODE_FONT: 2,
		BROTLI_MIN_QUALITY: 0, BROTLI_MAX_QUALITY: 11, BROTLI_DEFAULT_QUALITY: 11,
		BROTLI_MIN_WINDOW_BITS: 10, BROTLI_MAX_WINDOW_BITS: 24, BROTLI_DEFAULT_WINDOW: 22,
	});
	const inputBytes = input => {
		if (typeof input === "string") return Buffer.from(input);
		if (input instanceof ArrayBuffer) return Buffer.from(input);
		if (ArrayBuffer.isView(input)) return Buffer.from(input.buffer, input.byteOffset, input.byteLength);
		throw Object.assign(new TypeError("Brotli input must be a string or byte view"), { code: "ERR_INVALID_ARG_TYPE" });
	};
	const transform = (decode, input, options = {}) => {
		const params = options.params ?? {};
		const quality = params[1] ?? 11, window = params[2] ?? 22, mode = params[0] ?? 0;
		if (!Number.isInteger(quality) || quality < 0 || quality > 11 || !Number.isInteger(window) || window < 10 || window > 24 || !Number.isInteger(mode) || mode < 0 || mode > 2) throw Object.assign(new RangeError("Invalid Brotli quality, window or mode"), { code: "ERR_OUT_OF_RANGE" });
		const maximum = options.maxOutputLength ?? Number.MAX_SAFE_INTEGER;
		if (!Number.isSafeInteger(maximum) || maximum < 1) throw Object.assign(new RangeError("maxOutputLength must be a positive safe integer"), { code: "ERR_OUT_OF_RANGE" });
		const method = `zlib.brotli${decode ? "Decompress" : "Compress"}Sync`;
		const raw = globalThis[method], args = [inputBytes(input), quality, window, mode, maximum];
		try {
			const result = typeof raw === "function" ? raw(...args) : raw.applySyncPromise(undefined, args);
			return Buffer.from(result);
		} catch (error) {
			if (!error.code) error.code = error.message.match(/ERR_[A-Z_]+/)?.[0] ?? "ERR_BROTLI_COMPRESSION_FAILED";
			throw error;
		}
	};
	class BrotliTransform extends Transform {
		constructor(decode, options = {}) {
			super(options); this.decode = decode; this.options = options; this.chunks = []; this.bytes = 0;
			this.limit = Number(globalThis.process?.env?.AGENTOS_BROTLI_MAX_BUFFERED_BYTES ?? 32 * 1024 * 1024);
			if (!Number.isSafeInteger(this.limit) || this.limit < 1) throw new RangeError("AGENTOS_BROTLI_MAX_BUFFERED_BYTES must be a positive safe integer");
		}
		_transform(chunk, _encoding, callback) {
			const bytes = inputBytes(chunk), requested = this.bytes + bytes.length;
			if (requested > this.limit) {
				callback(Object.assign(new RangeError(`Brotli stream buffers at most ${this.limit} bytes; raise AGENTOS_BROTLI_MAX_BUFFERED_BYTES`), { code: "ERR_AGENTOS_BROTLI_INPUT_LIMIT", configurationPath: "AGENTOS_BROTLI_MAX_BUFFERED_BYTES", configuredLimit: this.limit, requested })); return;
			}
			if (this.bytes < this.limit * 0.8 && requested >= this.limit * 0.8) console.warn("Brotli stream approaches AGENTOS_BROTLI_MAX_BUFFERED_BYTES", { bytes: requested, limit: this.limit });
			this.bytes = requested; this.chunks.push(Buffer.from(bytes)); callback();
		}
		_flush(callback) {
			try { const input = Buffer.concat(this.chunks, this.bytes); this.chunks = []; this.bytes = 0; this.push(transform(this.decode, input, this.options)); callback(); }
			catch (error) { callback(error); }
		}
		_destroy(error, callback) { this.chunks = []; this.bytes = 0; callback(error); }
	}
	zlib.BrotliCompress = class BrotliCompress extends BrotliTransform { constructor(options) { super(false, options); } };
	zlib.BrotliDecompress = class BrotliDecompress extends BrotliTransform { constructor(options) { super(true, options); } };
	zlib.createBrotliCompress = options => new zlib.BrotliCompress(options);
	zlib.createBrotliDecompress = options => new zlib.BrotliDecompress(options);
	for (const [name, decode] of [["brotliCompress", false], ["brotliDecompress", true]]) {
		zlib[`${name}Sync`] = (input, options) => transform(decode, input, options);
		zlib[name] = (input, options, callback) => {
			if (typeof options === "function") { callback = options; options = {}; }
			if (typeof callback !== "function") throw new TypeError("Brotli callback must be a function");
			queueMicrotask(() => { let result; try { result = transform(decode, input, options); } catch (error) { callback(error); return; } callback(null, result); });
		};
	}
}
