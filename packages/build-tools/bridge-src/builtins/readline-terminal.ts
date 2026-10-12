import { guestBridgeLimits } from "../guest-bridge-limits.js";
import { stringDecoderStdlibModuleNs } from "../prelude.js";
import { BUFFER_MAX_STRING_LENGTH } from "./buffer-constants.js";
import { installBuiltinUtilFormatWithOptions } from "./console.js";
import { process_default } from "./process.js";
import {
	charLengthAt,
	charLengthLeft,
	emitKeys,
	isFullWidthCodePoint,
	type ReadlineKey,
} from "./readline-keys.js";
import { clearTimeout2, setTimeout2 } from "./timers.js";

const StringDecoder =
	stringDecoderStdlibModuleNs.StringDecoder ??
	stringDecoderStdlibModuleNs.default?.StringDecoder;

const DEFAULT_HISTORY_SIZE = 30;
const ESCAPE = "\x1b";
const NONPRINTING = /[\p{Cc}\p{Mark}\p{Default_Ignorable_Code_Point}]/u;
const EMOJI_PRESENTATION = /\p{Emoji_Presentation}/u;

export function readlineCursorTo(x, y) {
	return y == null ? `\x1b[${x + 1}G` : `\x1b[${y + 1};${x + 1}H`;
}

export function readlineMoveCursor(dx, dy) {
	return (
		(dx < 0 ? `\x1b[${-dx}D` : dx > 0 ? `\x1b[${dx}C` : "") +
		(dy < 0 ? `\x1b[${-dy}A` : dy > 0 ? `\x1b[${dy}B` : "")
	);
}

export function readlineClearLine(dir) {
	return dir < 0 ? "\x1b[1K" : dir > 0 ? "\x1b[0K" : "\x1b[2K";
}

export function readlineClearScreenDown() {
	return "\x1b[0J";
}

function limitError(message, code = "ERR_STRING_TOO_LONG") {
	return Object.assign(new RangeError(message), { code });
}

function createHistory(options) {
	const maxHistorySize = guestBridgeLimits().maxReadlineHistoryEntries;
	const size = options.historySize ?? DEFAULT_HISTORY_SIZE;
	if (!Number.isInteger(size) || size < 0 || size > maxHistorySize) {
		throw limitError(
			`readline historySize must be an integer from 0 to ${maxHistorySize}: observed=${size}; lower historySize or raise limits.jsRuntime.maxReadlineHistoryEntries when creating the VM`,
			"ERR_OUT_OF_RANGE",
		);
	}
	if (size >= maxHistorySize * 0.8)
		console.warn(
			`limits.jsRuntime.maxReadlineHistoryEntries approaching limit: observed=${size} maximum=${maxHistorySize}; lower historySize or raise limits.jsRuntime.maxReadlineHistoryEntries when creating the VM`,
		);
	const initial = options.history ?? [];
	if (!Array.isArray(initial))
		throw Object.assign(new TypeError("readline history must be an array"), {
			code: "ERR_INVALID_ARG_TYPE",
		});
	const entries = initial.slice(0, size);
	let units = 0;
	let warned = false;
	const check = (length) => {
		if (length > BUFFER_MAX_STRING_LENGTH) {
			throw limitError(
				"readline terminal history exceeds Buffer.constants.MAX_STRING_LENGTH; reduce historySize or line length",
			);
		}
		if (!warned && length >= BUFFER_MAX_STRING_LENGTH * 0.8) {
			warned = true;
			console.warn(
				"readline terminal history approaching Buffer.constants.MAX_STRING_LENGTH; reduce historySize or line length",
			);
		}
	};
	for (const entry of entries) {
		if (typeof entry !== "string") {
			throw Object.assign(
				new TypeError("readline history entries must be strings"),
				{
					code: "ERR_INVALID_ARG_TYPE",
				},
			);
		}
		units += entry.length;
		check(units);
	}
	return {
		entries,
		add(line) {
			if (!size || !line.trim() || entries[0] === line) return false;
			const duplicate = options.removeHistoryDuplicates
				? entries.indexOf(line)
				: -1;
			const remove =
				duplicate >= 0 ? duplicate : entries.length === size ? size - 1 : -1;
			const nextUnits =
				units + line.length - (remove >= 0 ? entries[remove].length : 0);
			check(nextUnits);
			if (remove >= 0) entries.splice(remove, 1);
			entries.unshift(line);
			units = nextUnits;
			return true;
		},
	};
}

export function readlineDisplayPosition(text, columns = Infinity, tabSize = 8) {
	const util = installBuiltinUtilFormatWithOptions(
		globalThis.__agentOsBuiltinUtilModule,
	);
	let offset = 0;
	let rows = 0;
	for (const character of util.stripVTControlCharacters(text)) {
		if (character === "\n") {
			rows += Math.ceil(offset / columns) || 1;
			offset = 0;
		} else if (character === "\t") {
			offset += tabSize - (offset % tabSize);
		} else if (!NONPRINTING.test(character)) {
			const wide =
				isFullWidthCodePoint(character.codePointAt(0)) ||
				EMOJI_PRESENTATION.test(character);
			if (wide && (offset + 1) % columns === 0) offset++;
			offset += wide ? 2 : 1;
		}
	}
	const cols = offset % columns;
	return { rows: rows + (offset - cols) / columns, cols };
}

export function createReadlineTerminal({
	input,
	output,
	options = {},
	callbackMode = false,
	onLine,
	onSignal,
	onClose,
	onHistory,
	getPrompt,
}) {
	const dumbInput = callbackMode && process_default.env.TERM === "dumb";
	const decoder = new StringDecoder("utf8");
	const history = createHistory(options);
	const tabSize = options.tabSize ?? 8;
	if (!Number.isInteger(tabSize) || tabSize < 1 || tabSize > 0xffffffff) {
		throw limitError(
			"readline tabSize must be a positive unsigned integer",
			"ERR_OUT_OF_RANGE",
		);
	}
	const crlfDelay = Math.max(100, options.crlfDelay || 100);
	const escapeCodeTimeout = options.escapeCodeTimeout ?? 500;
	if (!Number.isFinite(escapeCodeTimeout)) {
		throw limitError(
			"readline escapeCodeTimeout must be finite",
			"ERR_INVALID_ARG_VALUE",
		);
	}
	let line = "";
	let cursor = 0;
	let previousRows = 0;
	let historyIndex = -1;
	let historyPrefix = null;
	let returnAt = null;
	let closed = false;
	let started = false;
	let rawMode = false;
	let resizeListener = false;
	let escapeTimer;
	let lineWarned = false;
	let chunkWarned = false;
	let finalInputCharacter = true;

	const outputText = (text) => output?.write?.(text);
	const position = (text) =>
		readlineDisplayPosition(text, output?.columns || Infinity, tabSize);
	const displayText = (end) => {
		const prompt = String(getPrompt());
		if (prompt.length + end > BUFFER_MAX_STRING_LENGTH)
			throw limitError(
				"readline terminal display exceeds Buffer.constants.MAX_STRING_LENGTH; shorten the prompt or line",
			);
		return prompt + line.slice(0, end);
	};
	const getCursorPos = () => position(displayText(cursor));

	function refresh() {
		if (closed || !output?.write) return;
		const displayed = displayText(line.length);
		const positionAtEnd = position(displayed);
		const positionAtCursor = getCursorPos();
		if (previousRows > 0) outputText(readlineMoveCursor(0, -previousRows));
		outputText(readlineCursorTo(0));
		outputText(readlineClearScreenDown());
		outputText(displayed);
		if (positionAtEnd.cols === 0) outputText(" ");
		outputText(readlineCursorTo(positionAtCursor.cols));
		const rowsBack = positionAtEnd.rows - positionAtCursor.rows;
		if (rowsBack > 0) outputText(readlineMoveCursor(0, -rowsBack));
		previousRows = positionAtCursor.rows;
	}

	function move(next) {
		const nextCursor = Math.min(Math.max(next, 0), line.length);
		if (nextCursor === cursor) return;
		const before = getCursorPos();
		cursor = nextCursor;
		const after = getCursorPos();
		if (before.rows !== after.rows) refresh();
		else if (before.cols !== after.cols)
			outputText(readlineMoveCursor(after.cols - before.cols, 0));
	}

	function checkLine(length) {
		if (length > BUFFER_MAX_STRING_LENGTH)
			throw limitError(
				"readline terminal line exceeds Buffer.constants.MAX_STRING_LENGTH; submit or shorten the line",
			);
		if (!lineWarned && length >= BUFFER_MAX_STRING_LENGTH * 0.8) {
			lineWarned = true;
			console.warn(
				"readline terminal line approaching Buffer.constants.MAX_STRING_LENGTH; submit or shorten the line",
			);
		}
	}

	function insert(text) {
		if (!text) return;
		checkLine(line.length + text.length);
		const atEnd = cursor === line.length;
		const previous = atEnd && finalInputCharacter ? getCursorPos() : null;
		line = atEnd
			? line + text
			: line.slice(0, cursor) + text + line.slice(cursor);
		cursor += text.length;
		if (!finalInputCharacter || previous?.rows === getCursorPos().rows)
			outputText(text);
		else refresh();
	}

	function remove(start, end) {
		if (start === end) return;
		line = line.slice(0, start) + line.slice(end);
		cursor = start;
		refresh();
	}

	function resetLine() {
		line = "";
		cursor = 0;
		previousRows = 0;
		historyIndex = -1;
		historyPrefix = null;
		lineWarned = false;
	}

	function submit() {
		const answer = line;
		if (history.add(answer)) onHistory?.(history.entries.slice());
		move(line.length);
		outputText("\r\n");
		resetLine();
		onLine(answer);
	}

	function historyMove(direction) {
		const { entries } = history;
		if (
			!entries.length ||
			(direction > 0 ? historyIndex === entries.length : historyIndex === -1)
		) {
			return;
		}

		const search = historyPrefix ?? "";
		let index = historyIndex + direction;
		while (
			index >= 0 &&
			index < entries.length &&
			(!entries[index].startsWith(search) || entries[index] === line)
		) {
			index += direction;
		}

		// Keep the exhausted Up and Down positions as distinct sentinels.
		historyIndex = index;
		line = index < 0 || index === entries.length ? search : entries[index];
		cursor = line.length;
		refresh();
	}

	function insertText(text) {
		if (typeof text !== "string" || !text) return;
		const newline = /\r?\n|\r/g;
		let start = 0;
		for (let match = newline.exec(text); match; match = newline.exec(text)) {
			insert(text.slice(start, match.index));
			submit();
			if (closed) return;
			start = newline.lastIndex;
		}
		insert(text.slice(start));
	}

	function wordLeft() {
		const reversed = Array.from(line.slice(0, cursor)).reverse().join("");
		return cursor - /^\s*(?:[^\w\s]+|\w+)?/.exec(reversed)[0].length;
	}

	function wordRight() {
		const match = /^(?:\s+|[^\w\s]+|\w+)\s*/.exec(line.slice(cursor));
		return cursor + (match?.[0].length ?? 0);
	}

	function submitReturn(name) {
		if (name === "return") {
			returnAt = Date.now();
			submit();
			return true;
		}
		if (name === "enter") {
			const paired = returnAt !== null && Date.now() - returnAt <= crlfDelay;
			returnAt = null;
			if (!paired) submit();
			return true;
		}
		return false;
	}

	function handleDumbKey(text, key: ReadlineKey) {
		const name = key.name;
		if (name === "escape") return;
		if (returnAt !== null && name !== "enter") returnAt = null;
		if (key.ctrl && name === "c") {
			onSignal("SIGINT");
			return;
		}
		if (key.ctrl && name === "d") {
			onClose();
			return;
		}
		if (submitReturn(name) || typeof text !== "string" || !text) return;
		checkLine(line.length + text.length);
		line += text;
		cursor += text.length;
		outputText(text);
	}

	function handleKey(text, key: ReadlineKey | null | undefined) {
		if (closed) return;
		key ??= {};
		if (dumbInput) {
			handleDumbKey(text, key);
			return;
		}
		const name = key.name;
		const prefixNavigation =
			(name === "up" || name === "down") &&
			!key.ctrl &&
			!key.meta &&
			!key.shift;
		if (prefixNavigation) {
			historyPrefix ??= line.slice(0, cursor);
		} else if (historyPrefix !== null) {
			historyPrefix = null;
			if (historyIndex === history.entries.length) historyIndex = -1;
		}
		if (name === "escape") return;
		if (key.ctrl && key.shift) {
			if (name === "backspace") {
				if (cursor === 0) refresh();
				else remove(0, cursor);
			} else if (name === "delete") {
				if (cursor === line.length) refresh();
				else remove(cursor, line.length);
			}
			return;
		}
		if (key.ctrl) {
			switch (name) {
				case "c":
					onSignal("SIGINT");
					break;
				case "d":
					if (!line) onClose();
					else
						remove(
							cursor,
							Math.min(cursor + charLengthAt(line, cursor), line.length),
						);
					break;
				case "h":
					remove(cursor - charLengthLeft(line, cursor), cursor);
					break;
				case "a":
					move(0);
					break;
				case "e":
					move(line.length);
					break;
				case "b":
					move(cursor - charLengthLeft(line, cursor));
					break;
				case "f":
					move(cursor + charLengthAt(line, cursor));
					break;
				case "u":
					remove(0, cursor);
					break;
				case "k":
					remove(cursor, line.length);
					break;
				case "w":
				case "backspace":
					remove(wordLeft(), cursor);
					break;
				case "delete":
					remove(cursor, wordRight());
					break;
				case "left":
					move(wordLeft());
					break;
				case "right":
					move(wordRight());
					break;
				case "l":
					outputText(readlineCursorTo(0, 0));
					outputText(readlineClearScreenDown());
					previousRows = 0;
					refresh();
					break;
				case "n":
					historyMove(-1);
					break;
				case "p":
					historyMove(1);
					break;
			}
			return;
		}
		if (key.meta) {
			switch (name) {
				case "b":
				case "left":
					move(wordLeft());
					break;
				case "f":
				case "right":
					move(wordRight());
					break;
				case "backspace":
					remove(wordLeft(), cursor);
					break;
				case "d":
				case "delete":
					remove(cursor, wordRight());
					break;
			}
			return;
		}
		if (returnAt !== null && name !== "enter") returnAt = null;
		if (submitReturn(name)) return;
		switch (name) {
			case "backspace":
				remove(cursor - charLengthLeft(line, cursor), cursor);
				break;
			case "delete":
				remove(
					cursor,
					Math.min(cursor + charLengthAt(line, cursor), line.length),
				);
				break;
			case "left":
				move(cursor - charLengthLeft(line, cursor));
				break;
			case "right":
				move(cursor + charLengthAt(line, cursor));
				break;
			case "home":
				move(0);
				break;
			case "end":
				move(line.length);
				break;
			case "up":
				historyMove(1);
				break;
			case "down":
				historyMove(-1);
				break;
			default:
				insertText(text);
		}
	}

	function newKeys() {
		const keys = emitKeys(handleKey);
		keys.next();
		return keys;
	}
	let keys = newKeys();

	function cancelEscape() {
		if (escapeTimer !== undefined) {
			const timer = escapeTimer;
			escapeTimer = undefined;
			clearTimeout2(timer);
		}
	}

	function keyCharacter(character) {
		try {
			keys.next(character);
		} catch (error) {
			keys = newKeys();
			throw error;
		}
	}

	function feed(text) {
		if (!text || closed) return;
		cancelEscape();
		const previous = finalInputCharacter;
		try {
			let consumed = 0;
			for (const character of text) {
				consumed += character.length;
				finalInputCharacter = consumed === text.length;
				keyCharacter(character);
				if (closed) return;
			}
		} finally {
			finalInputCharacter = previous;
		}
		if (text.endsWith(ESCAPE)) {
			escapeTimer = setTimeout2(() => {
				escapeTimer = undefined;
				if (!closed) keyCharacter("");
			}, escapeCodeTimeout);
		}
	}

	function checkChunk(chunk) {
		const length = typeof chunk === "string" ? chunk.length : chunk?.byteLength;
		if (length > BUFFER_MAX_STRING_LENGTH)
			throw limitError(
				"readline terminal input chunk exceeds Buffer.constants.MAX_STRING_LENGTH; split the input chunk",
			);
		if (!chunkWarned && length >= BUFFER_MAX_STRING_LENGTH * 0.8) {
			chunkWarned = true;
			console.warn(
				"readline terminal input chunk approaching Buffer.constants.MAX_STRING_LENGTH; split the input chunk",
			);
		}
	}

	function close() {
		if (closed) return;
		closed = true;
		try {
			cancelEscape();
		} finally {
			try {
				if (resizeListener)
					(output.off ?? output.removeListener).call(output, "resize", refresh);
			} finally {
				if (rawMode) input.setRawMode(false);
			}
		}
	}

	return {
		start() {
			if (started || closed) return;
			started = true;
			try {
				if (
					typeof input?.setRawMode === "function" &&
					(input !== process_default.stdin || input.isTTY)
				) {
					input.setRawMode(true);
					rawMode = true;
				}
				if (
					typeof output?.on === "function" &&
					typeof (output.off ?? output.removeListener) === "function"
				) {
					resizeListener = true;
					output.on("resize", refresh);
				}
			} catch (error) {
				close();
				throw error;
			}
		},
		writeInput(chunk) {
			if (closed) return;
			checkChunk(chunk);
			feed(typeof chunk === "string" ? chunk : decoder.write(chunk));
		},
		write(data, key, finalCharacter = true) {
			if (closed) return;
			checkChunk(data);
			const previous = finalInputCharacter;
			finalInputCharacter = finalCharacter;
			try {
				handleKey(data, key);
			} finally {
				finalInputCharacter = previous;
			}
		},
		prompt(preserveCursor = false) {
			if (closed) return;
			if (process_default.env.TERM === "dumb") {
				outputText(getPrompt());
				return;
			}
			if (!preserveCursor) cursor = 0;
			refresh();
		},
		end() {
			if (closed) return "";
			try {
				feed(decoder.end());
			} finally {
				cancelEscape();
			}
			return line;
		},
		close,
		getCursorPos,
		resetLine,
		get line() {
			return line;
		},
		get cursor() {
			return cursor;
		},
		get history() {
			return history.entries.slice();
		},
	};
}
