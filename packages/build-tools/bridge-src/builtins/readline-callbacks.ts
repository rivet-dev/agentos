/*! @license
 * Adapted from Node.js v24.18.0 lib/internal/readline/callbacks.js and
 * lib/internal/readline/emitKeypressEvents.js.
 * Copyright Node.js contributors. All rights reserved.
 * Copyright Joyent, Inc. and other Node contributors. All rights reserved.
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to
 * deal in the Software without restriction, including without limitation the
 * rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
 * sell copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in
 * all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
 * FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
 * IN THE SOFTWARE.
 */
import { stringDecoderStdlibModuleNs } from "../prelude.js";
import { BUFFER_MAX_STRING_LENGTH } from "./buffer-constants.js";
import { emitKeys } from "./readline-keys.js";
import { process_default } from "./process.js";
import { setTimeout2, clearTimeout2 } from "./timers.js";

const StringDecoder =
	stringDecoderStdlibModuleNs.StringDecoder ??
	stringDecoderStdlibModuleNs.default?.StringDecoder;
const kClearLine = "\x1b[2K",
	kClearScreenDown = "\x1b[0J",
	kClearToLineBeginning = "\x1b[1K",
	kClearToLineEnd = "\x1b[0K";
const CSI = (strings, ...values) =>
	"\x1b[" + String.raw({ raw: strings }, ...values);
class ERR_INVALID_ARG_VALUE extends TypeError {
	code = "ERR_INVALID_ARG_VALUE";
	constructor(name, value) {
		super(`The argument '${name}' is invalid. Received ${String(value)}`);
	}
}
class ERR_INVALID_CURSOR_POS extends TypeError {
	code = "ERR_INVALID_CURSOR_POS";
	constructor() {
		super("Cannot set cursor row without setting its column");
	}
}
function validateFunction(value, name) {
	if (typeof value !== "function")
		throw Object.assign(
			new TypeError(`The "${name}" argument must be of type function`),
			{ code: "ERR_INVALID_ARG_TYPE" },
		);
}

function cursorTo(stream, x, y, callback) {
	if (callback !== undefined) {
		validateFunction(callback, "callback");
	}

	if (typeof y === "function") {
		callback = y;
		y = undefined;
	}

	if (Number.isNaN(x)) throw new ERR_INVALID_ARG_VALUE("x", x);
	if (Number.isNaN(y)) throw new ERR_INVALID_ARG_VALUE("y", y);

	if (stream == null || (typeof x !== "number" && typeof y !== "number")) {
		if (typeof callback === "function")
			process_default.nextTick(callback, null);
		return true;
	}

	if (typeof x !== "number") throw new ERR_INVALID_CURSOR_POS();

	const data = typeof y !== "number" ? CSI`${x + 1}G` : CSI`${y + 1};${x + 1}H`;
	return stream.write(data, callback);
}

/**
 * moves the cursor relative to its current location
 */

function moveCursor(stream, dx, dy, callback) {
	if (callback !== undefined) {
		validateFunction(callback, "callback");
	}

	if (stream == null || !(dx || dy)) {
		if (typeof callback === "function")
			process_default.nextTick(callback, null);
		return true;
	}

	let data = "";

	if (dx < 0) {
		data += CSI`${-dx}D`;
	} else if (dx > 0) {
		data += CSI`${dx}C`;
	}

	if (dy < 0) {
		data += CSI`${-dy}A`;
	} else if (dy > 0) {
		data += CSI`${dy}B`;
	}

	return stream.write(data, callback);
}

/**
 * clears the current line the cursor is on:
 *   -1 for left of the cursor
 *   +1 for right of the cursor
 *    0 for the entire line
 */

function clearLine(stream, dir, callback) {
	if (callback !== undefined) {
		validateFunction(callback, "callback");
	}

	if (stream === null || stream === undefined) {
		if (typeof callback === "function")
			process_default.nextTick(callback, null);
		return true;
	}

	const type =
		dir < 0 ? kClearToLineBeginning : dir > 0 ? kClearToLineEnd : kClearLine;
	return stream.write(type, callback);
}

/**
 * clears the screen from the current position of the cursor down
 */

function clearScreenDown(stream, callback) {
	if (callback !== undefined) {
		validateFunction(callback, "callback");
	}

	if (stream === null || stream === undefined) {
		if (typeof callback === "function")
			process_default.nextTick(callback, null);
		return true;
	}

	return stream.write(kClearScreenDown, callback);
}

// One decoder/parser and at most one existing bounded escape timer per stream.
// Weak ownership does not keep otherwise unreachable streams alive.
const keypressOwners = new WeakMap();
function isFinalReadlineKeypress(stream, consumer) {
	const owner = keypressOwners.get(stream);
	return owner?.consumer === consumer ? owner.finalInputCharacter : true;
}
function releaseReadlineKeypressConsumer(stream) {
	keypressOwners.get(stream)?.release();
}
function emitKeypressEvents(stream, iface = {}) {
	if (keypressOwners.has(stream)) return;
	const decoder = new StringDecoder("utf8");
	const { escapeCodeTimeout = 500 } = iface;
	const createParser = () => {
		const parser = emitKeys((text, key) => stream.emit("keypress", text, key));
		parser.next();
		return parser;
	};
	let parser = createParser();
	const resetParser = () => {
		parser = createParser();
	};
	let timeoutId: ReturnType<typeof setTimeout2> | undefined;
	let active = false;
	let ended = false;
	let closed = false;
	let warned = false;
	const cancelEscape = () => {
		if (timeoutId !== undefined) {
			clearTimeout2(timeoutId);
			timeoutId = undefined;
		}
	};
	const triggerEscape = () => {
		timeoutId = undefined;
		parser.next("");
	};
	function onData(input) {
		if (stream.listenerCount("keypress") === 0) {
			deactivate();
			return;
		}
		const length = typeof input === "string" ? input.length : input?.byteLength;
		if (length > BUFFER_MAX_STRING_LENGTH)
			throw Object.assign(
				new RangeError(
					"readline keypress input exceeds Buffer.constants.MAX_STRING_LENGTH; split the input chunk",
				),
				{ code: "ERR_STRING_TOO_LONG" },
			);
		if (!warned && length >= BUFFER_MAX_STRING_LENGTH * 0.8) {
			warned = true;
			console.warn(
				"readline keypress input approaching Buffer.constants.MAX_STRING_LENGTH; split the input chunk",
			);
		}
		const string = typeof input === "string" ? input : decoder.write(input);
		if (!string) return;
		cancelEscape();
		iface.isCompletionEnabled = false;
		let lengthSeen = 0;
		const previousFinalCharacter = owner.finalInputCharacter;
		try {
			for (const character of string) {
				lengthSeen += character.length;
				owner.finalInputCharacter = lengthSeen === string.length;
				if (owner.finalInputCharacter) iface.isCompletionEnabled = true;
				try {
					parser.next(character);
					// A key listener can close the last Interface during this chunk.
					if (!active) break;
					if (lengthSeen === string.length && character === "\x1b")
						timeoutId = setTimeout2(triggerEscape, escapeCodeTimeout);
				} catch (error) {
					resetParser();
					throw error;
				}
			}
		} finally {
			owner.finalInputCharacter = previousFinalCharacter;
		}
	}
	function activate() {
		if (active || closed) return;
		active = true;
		stream.removeListener("newListener", onNewListener);
		stream.on("data", onData);
	}
	function deactivate() {
		if (!active) return;
		active = false;
		stream.removeListener("data", onData);
		stream.on("newListener", onNewListener);
	}
	function onNewListener(event) {
		if (event === "keypress") activate();
	}
	function onEnd() {
		ended = true;
	}
	function onClose() {
		closed = true;
		active = false;
		if (!ended) cancelEscape();
		stream.removeListener("end", onEnd);
		stream.removeListener("data", onData);
		stream.removeListener("newListener", onNewListener);
	}
	const owner = {
		consumer: iface,
		finalInputCharacter: true,
		release() {
			if (stream.listenerCount("keypress") === 0) {
				cancelEscape();
				deactivate();
			}
		},
	};
	keypressOwners.set(stream, owner);
	stream.once("end", onEnd);
	stream.once("close", onClose);
	if (stream.listenerCount("keypress") > 0) activate();
	else stream.on("newListener", onNewListener);
}

// Preserve the standard function names through the minified native bridge.
for (const [name, callback] of Object.entries({
	cursorTo,
	moveCursor,
	clearLine,
	clearScreenDown,
	emitKeypressEvents,
})) {
	Object.defineProperty(callback, "name", { value: name, configurable: true });
}

export {
	isFinalReadlineKeypress,
	releaseReadlineKeypressConsumer,
	cursorTo,
	moveCursor,
	clearLine,
	clearScreenDown,
	emitKeypressEvents,
};
