// `Uint8Array` hex and base64 methods from tc39/proposal-arraybuffer-base64.
// V8 130 has no native version, so the guest gets this one. A later V8 with
// native methods keeps its own: `defineMissingMethods` skips them.
//
// These act as built-ins, so they read the view through captured intrinsics.
// A subclass's `length`, or a guest patch of `slice` or `Buffer`, cannot change
// the bytes they see or the type they return.

// Guest code can replace these globals, as test environments often do.
// biome-ignore lint/suspicious/noShadowRestrictedNames: keeping the originals is the point.
const { ArrayBuffer, Number, SyntaxError, TypeError, Uint8Array } = globalThis;

const BASE64_CHARS =
	"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64URL_CHARS = `${BASE64_CHARS.slice(0, 62)}-_`;
const HEX_CHARS = "0123456789abcdef";
const EQUALS_CODE = 61;

// Encoders write character codes to a chunk and turn each full chunk into one
// flat string. Appending a piece per byte instead allocates a heap node per byte
// until V8 flattens the string, which exhausts the VM's heap on a few MiB.
// This is a multiple of the 2 and 4 characters that one encoding step writes.
// Each code is one call argument, so a larger chunk fails sooner near the stack
// limit.
const ENCODE_CHUNK_CHARS = 0x800;

function createCharCodes(chars) {
	return Uint8Array.from(chars, (char) => char.charCodeAt(0));
}

function createDecodeTable(chars) {
	const table = new Int8Array(128).fill(-1);
	for (let value = 0; value < chars.length; value++) {
		table[chars.charCodeAt(value)] = value;
	}
	return table;
}

const BASE64_CODES = createCharCodes(BASE64_CHARS);
const BASE64URL_CODES = createCharCodes(BASE64URL_CHARS);
const HEX_CODES = createCharCodes(HEX_CHARS);
const BASE64_VALUES = createDecodeTable(BASE64_CHARS);
const BASE64URL_VALUES = createDecodeTable(BASE64URL_CHARS);
const HEX_VALUES = createDecodeTable(HEX_CHARS);
HEX_VALUES.set([10, 11, 12, 13, 14, 15], "A".charCodeAt(0));

const NO_OPTIONS = Object.freeze(Object.create(null));
const apply = Reflect.apply;
const fromCharCode = String.fromCharCode;
const charCodeAt = Function.prototype.call.bind(String.prototype.charCodeAt);
const typedArrayPrototype = Object.getPrototypeOf(Uint8Array.prototype);
const typedArrayIntrinsic = (name) =>
	Object.getOwnPropertyDescriptor(typedArrayPrototype, name);
const getTypedArrayName = typedArrayIntrinsic(Symbol.toStringTag).get;
const getTypedArrayLength = typedArrayIntrinsic("length").get;
const typedArrayValues = typedArrayIntrinsic("values").value;
const typedArraySet = typedArrayIntrinsic("set").value;

function assertUint8Array(value, method) {
	if (apply(getTypedArrayName, value, []) !== "Uint8Array") {
		throw new TypeError(
			`Uint8Array.prototype.${method} requires a Uint8Array receiver`,
		);
	}
}

// `values` is the shortest built-in that throws a TypeError when the buffer is
// detached or the view is out of bounds. Those views otherwise read as empty.
function readableLength(view) {
	apply(typedArrayValues, view, []);
	return apply(getTypedArrayLength, view, []);
}

function readOptions(options) {
	if (options === undefined) return NO_OPTIONS;
	if (
		(typeof options === "object" && options !== null) ||
		typeof options === "function"
	) {
		return options;
	}
	throw new TypeError(
		"Uint8Array base64 options must be an object or undefined",
	);
}

function readAlphabet(options) {
	const alphabet = options.alphabet;
	if (alphabet === undefined) return "base64";
	if (alphabet !== "base64" && alphabet !== "base64url") {
		throw new TypeError(
			'Uint8Array base64 option "alphabet" must be "base64" or "base64url"',
		);
	}
	return alphabet;
}

function readLastChunkHandling(options) {
	const handling = options.lastChunkHandling;
	if (handling === undefined) return "loose";
	if (
		handling !== "loose" &&
		handling !== "strict" &&
		handling !== "stop-before-partial"
	) {
		throw new TypeError(
			'Uint8Array base64 option "lastChunkHandling" must be "loose", "strict", or "stop-before-partial"',
		);
	}
	return handling;
}

function assertString(value, method) {
	if (typeof value !== "string") {
		throw new TypeError(`Uint8Array ${method} requires a string`);
	}
}

// Forgiving-base64 whitespace. It excludes the vertical tab that `\s` accepts.
function isBase64Whitespace(code) {
	return code === 9 || code === 10 || code === 12 || code === 13 || code === 32;
}

function skipBase64Whitespace(string, start) {
	let index = start;
	while (
		index < string.length &&
		isBase64Whitespace(charCodeAt(string, index))
	) {
		index++;
	}
	return index;
}

// Each chunk becomes one flat string, so an encoding allocates few heap nodes.
function charsToString(storage, count) {
	return apply(fromCharCode, null, new Uint8Array(storage, 0, count));
}

// Flatten a rope now, before a GC can promote it. V8 130 aborts the whole process,
// not only this guest, when a large rope that survived a GC fails to flatten.
function flatten(string) {
	charCodeAt(string, 0);
	return string;
}

function encodeHex(bytes, length) {
	const storage = new ArrayBuffer(
		length * 2 < ENCODE_CHUNK_CHARS ? length * 2 : ENCODE_CHUNK_CHARS,
	);
	const chars = new Uint8Array(storage);
	let encoded = "";
	let count = 0;
	for (let index = 0; index < length; index++) {
		if (count === ENCODE_CHUNK_CHARS) {
			encoded += charsToString(storage, count);
			count = 0;
		}
		const byte = bytes[index];
		chars[count++] = HEX_CODES[byte >>> 4];
		chars[count++] = HEX_CODES[byte & 15];
	}
	return flatten(encoded + charsToString(storage, count));
}

function encodeBase64(bytes, length, codes, omitPadding) {
	const fullChunksEnd = length - (length % 3);
	const paddedLength = ((length + 2 - ((length + 2) % 3)) / 3) * 4;
	const storage = new ArrayBuffer(
		paddedLength < ENCODE_CHUNK_CHARS ? paddedLength : ENCODE_CHUNK_CHARS,
	);
	const chars = new Uint8Array(storage);
	let encoded = "";
	let count = 0;
	for (let index = 0; index < fullChunksEnd; index += 3) {
		if (count === ENCODE_CHUNK_CHARS) {
			encoded += charsToString(storage, count);
			count = 0;
		}
		const bits =
			(bytes[index] << 16) | (bytes[index + 1] << 8) | bytes[index + 2];
		chars[count++] = codes[bits >>> 18];
		chars[count++] = codes[(bits >>> 12) & 63];
		chars[count++] = codes[(bits >>> 6) & 63];
		chars[count++] = codes[bits & 63];
	}
	const remainder = length - fullChunksEnd;
	if (remainder > 0) {
		if (count === ENCODE_CHUNK_CHARS) {
			encoded += charsToString(storage, count);
			count = 0;
		}
		const bits =
			remainder === 1
				? bytes[fullChunksEnd] << 16
				: (bytes[fullChunksEnd] << 16) | (bytes[fullChunksEnd + 1] << 8);
		chars[count++] = codes[bits >>> 18];
		chars[count++] = codes[(bits >>> 12) & 63];
		if (remainder === 2) chars[count++] = codes[(bits >>> 6) & 63];
		if (!omitPadding) {
			if (remainder === 1) chars[count++] = EQUALS_CODE;
			chars[count++] = EQUALS_CODE;
		}
	}
	return flatten(encoded + charsToString(storage, count));
}

function invalidBase64() {
	return new SyntaxError("Uint8Array received an invalid base64 string");
}

function writeFinalBase64Chunk(
	target,
	written,
	bits,
	chunkLength,
	rejectExtraBits,
) {
	const extraBits = chunkLength === 2 ? bits & 15 : bits & 3;
	if (rejectExtraBits && extraBits !== 0) throw invalidBase64();
	if (chunkLength === 2) {
		target[written] = bits >>> 4;
		return written + 1;
	}
	target[written] = bits >>> 10;
	target[written + 1] = (bits >>> 2) & 255;
	return written + 2;
}

// Follows the spec's FromBase64 steps, but writes each chunk directly to
// `target`. A SyntaxError therefore keeps the chunks before it, which
// `setFromBase64` requires. `bits` holds the values of the current chunk.
function decodeBase64Into(
	target,
	string,
	alphabet,
	lastChunkHandling,
	maxLength,
) {
	if (maxLength === 0) return { read: 0, written: 0 };
	const values = alphabet === "base64url" ? BASE64URL_VALUES : BASE64_VALUES;
	const length = string.length;
	let read = 0;
	let written = 0;
	let bits = 0;
	let chunkLength = 0;
	let index = 0;

	for (;;) {
		index = skipBase64Whitespace(string, index);

		if (index === length) {
			if (chunkLength > 0) {
				if (lastChunkHandling === "stop-before-partial")
					return { read, written };
				if (lastChunkHandling === "strict" || chunkLength === 1)
					throw invalidBase64();
				written = writeFinalBase64Chunk(
					target,
					written,
					bits,
					chunkLength,
					false,
				);
			}
			return { read: length, written };
		}

		const code = charCodeAt(string, index++);

		if (code === EQUALS_CODE) {
			if (chunkLength < 2) throw invalidBase64();
			index = skipBase64Whitespace(string, index);
			if (chunkLength === 2) {
				if (index === length) {
					if (lastChunkHandling === "stop-before-partial")
						return { read, written };
					throw invalidBase64();
				}
				if (charCodeAt(string, index) === EQUALS_CODE) {
					index = skipBase64Whitespace(string, index + 1);
				}
			}
			if (index < length) throw invalidBase64();
			written = writeFinalBase64Chunk(
				target,
				written,
				bits,
				chunkLength,
				lastChunkHandling === "strict",
			);
			return { read: length, written };
		}

		const value = code < 128 ? values[code] : -1;
		if (value < 0) throw invalidBase64();
		const remaining = maxLength - written;
		if (
			(remaining === 1 && chunkLength === 2) ||
			(remaining === 2 && chunkLength === 3)
		) {
			return { read, written };
		}
		bits = (bits << 6) | value;
		chunkLength++;

		if (chunkLength === 4) {
			target[written++] = bits >>> 16;
			target[written++] = (bits >>> 8) & 255;
			target[written++] = bits & 255;
			bits = 0;
			chunkLength = 0;
			read = index;
			if (written === maxLength) return { read, written };
		}
	}
}

// The result size for most input, so it needs no second buffer. Whitespace
// before the end, or a partial chunk that `stop-before-partial` drops, makes it
// too large. It is never too small: it only counts `=` and whitespace as
// characters when data follows them.
function base64ByteCapacity(string) {
	let end = string.length;
	while (end > 0) {
		const code = charCodeAt(string, end - 1);
		if (code !== EQUALS_CODE && !isBase64Whitespace(code)) break;
		end--;
	}
	const partialChars = end % 4;
	return (
		((end - partialChars) / 4) * 3 + (partialChars > 1 ? partialChars - 1 : 0)
	);
}

function assertEvenHexLength(string) {
	if (string.length % 2 !== 0) {
		throw new SyntaxError(
			"Uint8Array received a hex string with an odd length",
		);
	}
}

// As with base64, a SyntaxError keeps the bytes before it.
function decodeHexInto(target, string, maxLength) {
	const length = string.length;
	let read = 0;
	let written = 0;
	while (read < length && written < maxLength) {
		const highCode = charCodeAt(string, read);
		const lowCode = charCodeAt(string, read + 1);
		const high = highCode < 128 ? HEX_VALUES[highCode] : -1;
		const low = lowCode < 128 ? HEX_VALUES[lowCode] : -1;
		if ((high | low) < 0) {
			throw new SyntaxError(
				"Uint8Array received a string with a non-hex character",
			);
		}
		target[written++] = (high << 4) | low;
		read += 2;
	}
	return { read, written };
}

function defineMissingMethods(target, methods) {
	for (const [name, value] of Object.entries(methods)) {
		if (typeof target[name] === "function") continue;
		Object.defineProperty(target, name, {
			configurable: true,
			writable: true,
			value,
		});
	}
}

// Methods are object-literal shorthand so they have no [[Construct]]. A default
// parameter keeps a method's `length` as the spec requires.
defineMissingMethods(Uint8Array.prototype, {
	toHex() {
		assertUint8Array(this, "toHex");
		return encodeHex(this, readableLength(this));
	},
	toBase64(options = undefined) {
		assertUint8Array(this, "toBase64");
		const opts = readOptions(options);
		const alphabet = readAlphabet(opts);
		const omitPadding = !!opts.omitPadding;
		// After the option reads: an option getter can detach the buffer.
		const length = readableLength(this);
		return encodeBase64(
			this,
			length,
			alphabet === "base64url" ? BASE64URL_CODES : BASE64_CODES,
			omitPadding,
		);
	},
	setFromHex(string) {
		assertUint8Array(this, "setFromHex");
		assertString(string, "setFromHex");
		const length = readableLength(this);
		assertEvenHexLength(string);
		return decodeHexInto(this, string, length);
	},
	setFromBase64(string, options = undefined) {
		assertUint8Array(this, "setFromBase64");
		assertString(string, "setFromBase64");
		const opts = readOptions(options);
		const alphabet = readAlphabet(opts);
		const lastChunkHandling = readLastChunkHandling(opts);
		// After the option reads: an option getter can detach the buffer.
		return decodeBase64Into(
			this,
			string,
			alphabet,
			lastChunkHandling,
			readableLength(this),
		);
	},
});

defineMissingMethods(Uint8Array, {
	fromHex(string) {
		assertString(string, "fromHex");
		assertEvenHexLength(string);
		flatten(string);
		const outputLength = string.length / 2;
		const output = new Uint8Array(outputLength);
		decodeHexInto(output, string, outputLength);
		return output;
	},
	fromBase64(string, options = undefined) {
		assertString(string, "fromBase64");
		const opts = readOptions(options);
		const alphabet = readAlphabet(opts);
		const lastChunkHandling = readLastChunkHandling(opts);
		const capacity = base64ByteCapacity(string);
		const storage = new ArrayBuffer(capacity);
		const { written } = decodeBase64Into(
			new Uint8Array(storage),
			string,
			alphabet,
			lastChunkHandling,
			Number.POSITIVE_INFINITY,
		);
		if (written === capacity) return new Uint8Array(storage);
		const output = new Uint8Array(written);
		apply(typedArraySet, output, [new Uint8Array(storage, 0, written)]);
		return output;
	},
});
