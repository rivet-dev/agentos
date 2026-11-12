/*! @license
 * Adapted from Node.js v24.18.0 lib/internal/readline/utils.js and
 * lib/internal/util/inspect.js (the non-ICU full-width fallback).
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

type ReadlineKey = {
	sequence?: string;
	name?: string;
	code?: string;
	ctrl?: boolean;
	meta?: boolean;
	shift?: boolean;
};

const RegExpPrototypeExec = Function.call.bind(RegExp.prototype.exec);
const StringPrototypeCharCodeAt = Function.call.bind(
	String.prototype.charCodeAt,
);
const StringPrototypeCodePointAt = Function.call.bind(
	String.prototype.codePointAt,
);
const StringPrototypeSlice = Function.call.bind(String.prototype.slice);
const StringPrototypeToLowerCase = Function.call.bind(
	String.prototype.toLowerCase,
);
const StringFromCharCode = String.fromCharCode;
const kUTF16SurrogateThreshold = 0x10000;
const kEscape = "\x1b";

function charLengthLeft(str, i) {
	if (i <= 0) return 0;
	if (
		(i > 1 &&
			StringPrototypeCodePointAt(str, i - 2) >= kUTF16SurrogateThreshold) ||
		StringPrototypeCodePointAt(str, i - 1) >= kUTF16SurrogateThreshold
	) {
		return 2;
	}
	return 1;
}

function charLengthAt(str, i) {
	if (str.length <= i) {
		// Pretend to move to the right. This is necessary to autocomplete while
		// moving to the right.
		return 1;
	}
	return StringPrototypeCodePointAt(str, i) >= kUTF16SurrogateThreshold ? 2 : 1;
}

function* emitKeys(emit: (text: string | undefined, key: ReadlineKey) => void) {
	while (true) {
		let ch = yield;
		let s = ch;
		let escaped = false;
		const key: ReadlineKey = {
			sequence: null,
			name: undefined,
			ctrl: false,
			meta: false,
			shift: false,
		};

		if (ch === kEscape) {
			escaped = true;
			s += ch = yield;

			if (ch === kEscape) {
				s += ch = yield;
			}
		}

		if (escaped && (ch === "O" || ch === "[")) {
			// ANSI escape sequence
			let code = ch;
			let modifier = 0;

			if (ch === "O") {
				// ESC O letter
				// ESC O modifier letter
				s += ch = yield;

				if (ch >= "0" && ch <= "9") {
					modifier = (ch >> 0) - 1;
					s += ch = yield;
				}

				code += ch;
			} else if (ch === "[") {
				// ESC [ letter
				// ESC [ modifier letter
				// ESC [ [ modifier letter
				// ESC [ [ num char
				s += ch = yield;

				if (ch === "[") {
					// \x1b[[A
					//      ^--- escape codes might have a second bracket
					code += ch;
					s += ch = yield;
				}

				/*
				 * Here and later we try to buffer just enough data to get
				 * a complete ascii sequence.
				 *
				 * We have basically two classes of ascii characters to process:
				 *
				 *
				 * 1. `\x1b[24;5~` should be parsed as { code: '[24~', modifier: 5 }
				 *
				 * This particular example is featuring Ctrl+F12 in xterm.
				 *
				 *  - `;5` part is optional, e.g. it could be `\x1b[24~`
				 *  - first part can contain one or two digits
				 *  - there is also special case when there can be 3 digits
				 *    but without modifier. They are the case of paste bracket mode
				 *
				 * So the generic regexp is like /^(?:\d\d?(;\d)?[~^$]|\d{3}~)$/
				 *
				 *
				 * 2. `\x1b[1;5H` should be parsed as { code: '[H', modifier: 5 }
				 *
				 * This particular example is featuring Ctrl+Home in xterm.
				 *
				 *  - `1;5` part is optional, e.g. it could be `\x1b[H`
				 *  - `1;` part is optional, e.g. it could be `\x1b[5H`
				 *
				 * So the generic regexp is like /^((\d;)?\d)?[A-Za-z]$/
				 *
				 */
				const cmdStart = s.length - 1;

				// Skip one or two leading digits
				if (ch >= "0" && ch <= "9") {
					s += ch = yield;

					if (ch >= "0" && ch <= "9") {
						s += ch = yield;

						if (ch >= "0" && ch <= "9") {
							s += ch = yield;
						}
					}
				}

				// skip modifier
				if (ch === ";") {
					s += ch = yield;

					if (ch >= "0" && ch <= "9") {
						s += yield;
					}
				}

				/*
				 * We buffered enough data, now trying to extract code
				 * and modifier from it
				 */
				const cmd = StringPrototypeSlice(s, cmdStart);
				let match;

				if (
					(match = RegExpPrototypeExec(
						/^(?:(\d\d?)(?:;(\d))?([~^$])|(\d{3}~))$/,
						cmd,
					))
				) {
					if (match[4]) {
						code += match[4];
					} else {
						code += match[1] + match[3];
						modifier = (match[2] || 1) - 1;
					}
				} else if (
					(match = RegExpPrototypeExec(/^((\d;)?(\d))?([A-Za-z])$/, cmd))
				) {
					code += match[4];
					modifier = (match[3] || 1) - 1;
				} else {
					code += cmd;
				}
			}

			// Parse the key modifier
			key.ctrl = !!(modifier & 4);
			key.meta = !!(modifier & 10);
			key.shift = !!(modifier & 1);
			key.code = code;

			// Parse the key itself
			switch (code) {
				/* xterm/gnome ESC [ letter (with modifier) */
				case "[P":
					key.name = "f1";
					break;
				case "[Q":
					key.name = "f2";
					break;
				case "[R":
					key.name = "f3";
					break;
				case "[S":
					key.name = "f4";
					break;

				/* xterm/gnome ESC O letter (without modifier) */
				case "OP":
					key.name = "f1";
					break;
				case "OQ":
					key.name = "f2";
					break;
				case "OR":
					key.name = "f3";
					break;
				case "OS":
					key.name = "f4";
					break;

				/* xterm/rxvt ESC [ number ~ */
				case "[11~":
					key.name = "f1";
					break;
				case "[12~":
					key.name = "f2";
					break;
				case "[13~":
					key.name = "f3";
					break;
				case "[14~":
					key.name = "f4";
					break;

				/* paste bracket mode */
				case "[200~":
					key.name = "paste-start";
					break;
				case "[201~":
					key.name = "paste-end";
					break;

				/* from Cygwin and used in libuv */
				case "[[A":
					key.name = "f1";
					break;
				case "[[B":
					key.name = "f2";
					break;
				case "[[C":
					key.name = "f3";
					break;
				case "[[D":
					key.name = "f4";
					break;
				case "[[E":
					key.name = "f5";
					break;

				/* common */
				case "[15~":
					key.name = "f5";
					break;
				case "[17~":
					key.name = "f6";
					break;
				case "[18~":
					key.name = "f7";
					break;
				case "[19~":
					key.name = "f8";
					break;
				case "[20~":
					key.name = "f9";
					break;
				case "[21~":
					key.name = "f10";
					break;
				case "[23~":
					key.name = "f11";
					break;
				case "[24~":
					key.name = "f12";
					break;

				/* xterm ESC [ letter */
				case "[A":
					key.name = "up";
					break;
				case "[B":
					key.name = "down";
					break;
				case "[C":
					key.name = "right";
					break;
				case "[D":
					key.name = "left";
					break;
				case "[E":
					key.name = "clear";
					break;
				case "[F":
					key.name = "end";
					break;
				case "[H":
					key.name = "home";
					break;

				/* xterm/gnome ESC O letter */
				case "OA":
					key.name = "up";
					break;
				case "OB":
					key.name = "down";
					break;
				case "OC":
					key.name = "right";
					break;
				case "OD":
					key.name = "left";
					break;
				case "OE":
					key.name = "clear";
					break;
				case "OF":
					key.name = "end";
					break;
				case "OH":
					key.name = "home";
					break;

				/* xterm/rxvt ESC [ number ~ */
				case "[1~":
					key.name = "home";
					break;
				case "[2~":
					key.name = "insert";
					break;
				case "[3~":
					key.name = "delete";
					break;
				case "[4~":
					key.name = "end";
					break;
				case "[5~":
					key.name = "pageup";
					break;
				case "[6~":
					key.name = "pagedown";
					break;

				/* putty */
				case "[[5~":
					key.name = "pageup";
					break;
				case "[[6~":
					key.name = "pagedown";
					break;

				/* rxvt */
				case "[7~":
					key.name = "home";
					break;
				case "[8~":
					key.name = "end";
					break;

				/* rxvt keys with modifiers */
				case "[a":
					key.name = "up";
					key.shift = true;
					break;
				case "[b":
					key.name = "down";
					key.shift = true;
					break;
				case "[c":
					key.name = "right";
					key.shift = true;
					break;
				case "[d":
					key.name = "left";
					key.shift = true;
					break;
				case "[e":
					key.name = "clear";
					key.shift = true;
					break;

				case "[2$":
					key.name = "insert";
					key.shift = true;
					break;
				case "[3$":
					key.name = "delete";
					key.shift = true;
					break;
				case "[5$":
					key.name = "pageup";
					key.shift = true;
					break;
				case "[6$":
					key.name = "pagedown";
					key.shift = true;
					break;
				case "[7$":
					key.name = "home";
					key.shift = true;
					break;
				case "[8$":
					key.name = "end";
					key.shift = true;
					break;

				case "Oa":
					key.name = "up";
					key.ctrl = true;
					break;
				case "Ob":
					key.name = "down";
					key.ctrl = true;
					break;
				case "Oc":
					key.name = "right";
					key.ctrl = true;
					break;
				case "Od":
					key.name = "left";
					key.ctrl = true;
					break;
				case "Oe":
					key.name = "clear";
					key.ctrl = true;
					break;

				case "[2^":
					key.name = "insert";
					key.ctrl = true;
					break;
				case "[3^":
					key.name = "delete";
					key.ctrl = true;
					break;
				case "[5^":
					key.name = "pageup";
					key.ctrl = true;
					break;
				case "[6^":
					key.name = "pagedown";
					key.ctrl = true;
					break;
				case "[7^":
					key.name = "home";
					key.ctrl = true;
					break;
				case "[8^":
					key.name = "end";
					key.ctrl = true;
					break;

				/* misc. */
				case "[Z":
					key.name = "tab";
					key.shift = true;
					break;
				default:
					key.name = "undefined";
					break;
			}
		} else if (ch === "\r") {
			// carriage return
			key.name = "return";
			key.meta = escaped;
		} else if (ch === "\n") {
			// Enter, should have been called linefeed
			key.name = "enter";
			key.meta = escaped;
		} else if (ch === "\t") {
			// tab
			key.name = "tab";
			key.meta = escaped;
		} else if (ch === "\b" || ch === "\x7f") {
			// backspace or ctrl+h
			key.name = "backspace";
			key.meta = escaped;
		} else if (ch === kEscape) {
			// escape key
			key.name = "escape";
			key.meta = escaped;
		} else if (ch === " ") {
			key.name = "space";
			key.meta = escaped;
		} else if (!escaped && ch <= "\x1a") {
			// ctrl+letter
			key.name = StringFromCharCode(
				StringPrototypeCharCodeAt(ch) + StringPrototypeCharCodeAt("a") - 1,
			);
			key.ctrl = true;
		} else if (RegExpPrototypeExec(/^[0-9A-Za-z]$/, ch) !== null) {
			// Letter, number, shift+letter
			key.name = StringPrototypeToLowerCase(ch);
			key.shift = RegExpPrototypeExec(/^[A-Z]$/, ch) !== null;
			key.meta = escaped;
		} else if (escaped) {
			// Escape sequence timeout
			key.name = ch.length ? undefined : "escape";
			key.meta = true;
		}

		key.sequence = s;

		if (s.length !== 0 && (key.name !== undefined || escaped)) {
			/* Named character or sequence */
			emit(escaped ? undefined : s, key);
		} else if (charLengthAt(s, 0) === s.length) {
			/* Single unnamed character, e.g. "." */
			emit(s, key);
		}
		/* Unrecognized or broken escape sequence, don't emit anything */
	}
}

const isFullWidthCodePoint = (code) => {
	// Code points are partially derived from:
	// https://www.unicode.org/Public/UNIDATA/EastAsianWidth.txt
	return (
		code >= 0x1100 &&
		(code <= 0x115f || // Hangul Jamo
			code === 0x2329 || // LEFT-POINTING ANGLE BRACKET
			code === 0x232a || // RIGHT-POINTING ANGLE BRACKET
			// CJK Radicals Supplement .. Enclosed CJK Letters and Months
			(code >= 0x2e80 && code <= 0x3247 && code !== 0x303f) ||
			// Enclosed CJK Letters and Months .. CJK Unified Ideographs Extension A
			(code >= 0x3250 && code <= 0x4dbf) ||
			// CJK Unified Ideographs .. Yi Radicals
			(code >= 0x4e00 && code <= 0xa4c6) ||
			// Hangul Jamo Extended-A
			(code >= 0xa960 && code <= 0xa97c) ||
			// Hangul Syllables
			(code >= 0xac00 && code <= 0xd7a3) ||
			// CJK Compatibility Ideographs
			(code >= 0xf900 && code <= 0xfaff) ||
			// Vertical Forms
			(code >= 0xfe10 && code <= 0xfe19) ||
			// CJK Compatibility Forms .. Small Form Variants
			(code >= 0xfe30 && code <= 0xfe6b) ||
			// Halfwidth and Fullwidth Forms
			(code >= 0xff01 && code <= 0xff60) ||
			(code >= 0xffe0 && code <= 0xffe6) ||
			// Kana Supplement
			(code >= 0x1b000 && code <= 0x1b001) ||
			// Enclosed Ideographic Supplement
			(code >= 0x1f200 && code <= 0x1f251) ||
			// Miscellaneous Symbols and Pictographs 0x1f300 - 0x1f5ff
			// Emoticons 0x1f600 - 0x1f64f
			(code >= 0x1f300 && code <= 0x1f64f) ||
			// CJK Unified Ideographs Extension B .. Tertiary Ideographic Plane
			(code >= 0x20000 && code <= 0x3fffd))
	);
};

export { charLengthAt, charLengthLeft, emitKeys, isFullWidthCodePoint };
export type { ReadlineKey };
