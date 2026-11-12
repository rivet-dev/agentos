import {
	isFinalReadlineKeypress,
	releaseReadlineKeypressConsumer,
	cursorTo,
	moveCursor,
	clearLine,
	clearScreenDown,
	emitKeypressEvents,
} from "./readline-callbacks.js";
import { TextDecoder } from "../polyfills/index.js";
import { BUFFER_MAX_STRING_LENGTH } from "./buffer-constants.js";
import { eventsModule } from "./events.js";
import { process_default } from "./process.js";
import {
	createReadlineTerminal,
	readlineDisplayPosition,
} from "./readline-terminal.js";

// Node readline uses this watermark for its async line iterator.
const READLINE_ITERATOR_HIGH_WATER_MARK = 1024;

// Both public entrypoints share the existing line decoder and input lifetime.
function createReadlineInterface(
	options = {},
	promiseMode = false,
	target = null,
) {
	const input = options.input ?? null;
	const output = options.output ?? null;
	const terminal =
		options.terminal === undefined ? !!output?.isTTY : !!options.terminal;
	let terminalEditor = null;
	let paused = false;
	let iteratorBackpressure = false;
	let waiterWarned = false;
	let closed = false;
	let ended = false;
	let pendingQuestion = null;
	let promptText = options.prompt ?? "> ";
	let savedPrompt = promptText;
	let constructorAbort = null;
	let iteratorActive = false;
	let iteratorFinished = false;
	let iteratorError = null;
	let lineBuffer = "";
	let trailingCR = null;
	const crlfDelay = Math.max(100, options.crlfDelay ?? 100);
	const queuedLines = [];
	const pendingLines = [];
	const pendingQuestionResolves = [];
	const textDecoder = new TextDecoder();
	const emit = (event, ...args) => api.emit(event, ...args);
	const enqueueLine = (line) => {
		if (promiseMode && !iteratorActive) return;
		if (pendingQuestionResolves.length > 0) {
			const resolve = pendingQuestionResolves.shift();
			resolve(line);
			return;
		}
		if (pendingLines.length > 0) {
			const waiter = pendingLines.shift();
			waiter.resolve({ done: false, value: line });
			if (pendingLines.length < READLINE_ITERATOR_HIGH_WATER_MARK * 0.8)
				waiterWarned = false;
			return;
		}
		queuedLines.push(line);
		if (
			promiseMode &&
			queuedLines.length >= READLINE_ITERATOR_HIGH_WATER_MARK
		) {
			iteratorBackpressure = true;
			api.pause();
		}
	};
	const emitLine = (line) => {
		if ((promiseMode || terminal) && pendingQuestion) {
			const question = pendingQuestion;
			pendingQuestion = null;
			promptText = savedPrompt;
			question.answer(line);
			return;
		}
		emit("line", line);
		if (!promiseMode) enqueueLine(line);
	};
	const flushBufferedLines = () => {
		let newline = lineBuffer.search(/[\n\u2028\u2029]/);
		while (
			newline !== -1 &&
			(!promiseMode || queuedLines.length < READLINE_ITERATOR_HIGH_WATER_MARK)
		) {
			const line = lineBuffer.slice(0, newline);
			lineBuffer = lineBuffer.slice(newline + 1);
			emitLine(line);
			newline = lineBuffer.search(/[\n\u2028\u2029]/);
		}
		if (
			ended &&
			(!promiseMode ||
				queuedLines.length < READLINE_ITERATOR_HIGH_WATER_MARK) &&
			lineBuffer.length > 0 &&
			newline === -1
		) {
			const trailingLine = lineBuffer;
			lineBuffer = "";
			// EOF's unterminated fragment is a line event, not an answer to question().
			if (promiseMode) emit("line", trailingLine);
			else emitLine(trailingLine);
		}
		if (ended && lineBuffer.length === 0) api.close();
	};

	const failIterator = (error) => {
		iteratorFinished = true;
		api.off("line", enqueueLine);
		api.off("error", failIterator);
		const waiter = pendingLines.shift();
		if (waiter) waiter.reject(error);
		else iteratorError = error;
		for (const pending of pendingLines.splice(0))
			pending.resolve({ done: true, value: undefined });
	};
	const detachInput = () => {
		if (!input || typeof input.off !== "function") {
			return;
		}
		input.off("data", onData);
		input.off("keypress", onKeypress);
		if (terminalEditor) releaseReadlineKeypressConsumer(input);
		input.off("end", onEnd);
		input.off("error", onError);
	};
	const onError = (error) => emit("error", error);
	const onKeypress = (text, key) =>
		terminalEditor?.write(text, key, isFinalReadlineKeypress(input, api));
	const onData = (chunk) => {
		if (closed) {
			return;
		}
		if (terminalEditor) {
			terminalEditor.writeInput(chunk);
			return;
		}
		if (promiseMode) {
			const incoming =
				typeof chunk === "string" ? chunk.length : (chunk?.byteLength ?? 0);
			if (incoming > BUFFER_MAX_STRING_LENGTH - lineBuffer.length)
				throw Object.assign(
					new RangeError(
						"readline input exceeds Buffer.constants.MAX_STRING_LENGTH; consume pending lines before supplying more input",
					),
					{ code: "ERR_STRING_TOO_LONG" },
				);
		}
		let text =
			typeof chunk === "string"
				? chunk
				: chunk instanceof Uint8Array
					? textDecoder.decode(chunk, { stream: true })
					: chunk != null
						? String(chunk)
						: "";
		if (text.length > 0) {
			// Record CR receipt before line backpressure can defer parsing this chunk.
			const receivedAt = Date.now();
			if (
				trailingCR !== null &&
				text[0] === "\n" &&
				receivedAt - trailingCR <= crlfDelay
			)
				text = text.slice(1);
			trailingCR = text.endsWith("\r") ? receivedAt : null;
			text = text.replace(/\r\n|\r/g, "\n");
		}
		lineBuffer += text;
		flushBufferedLines();
	};
	const onEnd = () => {
		if (ended) {
			return;
		}
		ended = true;
		if (terminalEditor) {
			const residual = terminalEditor.end();
			if (residual) {
				emit("line", residual);
				if (!promiseMode) enqueueLine(residual);
			}
			api.close();
			return;
		}
		const trailing = textDecoder.decode();
		if (trailing) {
			lineBuffer += trailing;
		}
		flushBufferedLines();
	};
	const iterator = {
		next() {
			if (queuedLines.length > 0) {
				const value = queuedLines.shift();
				if (promiseMode && iteratorBackpressure && queuedLines.length === 0) {
					iteratorBackpressure = false;
					flushBufferedLines();
					if (!iteratorBackpressure && !closed) api.resume();
				}
				return Promise.resolve({ done: false, value });
			}
			if (iteratorError) {
				const error = iteratorError;
				iteratorError = null;
				return Promise.reject(error);
			}
			if (closed || ended || iteratorFinished) {
				return Promise.resolve({ done: true, value: void 0 });
			}
			if (pendingLines.length >= READLINE_ITERATOR_HIGH_WATER_MARK) {
				return Promise.reject(
					Object.assign(
						new RangeError(
							"readline pending next() limit 1024 exceeded; await outstanding iterator reads before requesting more",
						),
						{ code: "ERR_RESOURCE_BUDGET_EXCEEDED" },
					),
				);
			}
			if (
				!waiterWarned &&
				pendingLines.length + 1 >= READLINE_ITERATOR_HIGH_WATER_MARK * 0.8
			) {
				waiterWarned = true;
				console.warn(
					"readline pending next() approaching limit 1024; await outstanding iterator reads",
				);
			}
			return new Promise((resolve, reject) =>
				pendingLines.push({ resolve, reject }),
			);
		},
		return() {
			iteratorFinished = true;
			api.off("line", enqueueLine);
			api.off("error", failIterator);
			for (const waiter of pendingLines.splice(0))
				waiter.resolve({ done: true, value: undefined });
			return Promise.resolve({ done: true, value: undefined });
		},
		[Symbol.asyncIterator]() {
			return this;
		},
	};
	const api = Object.assign(target ?? new eventsModule(), {
		pause() {
			if (closed)
				throw Object.assign(new Error("readline was closed"), {
					code: "ERR_USE_AFTER_CLOSE",
				});
			if (paused) return;
			paused = true;
			this.paused = true;
			input?.pause?.();
			emit("pause");
			return this;
		},
		resume() {
			if (closed)
				throw Object.assign(new Error("readline was closed"), {
					code: "ERR_USE_AFTER_CLOSE",
				});
			if (!paused) return;
			paused = false;
			this.paused = false;
			input?.resume?.();
			emit("resume");
			return this;
		},
		input,
		output,
		terminal,
		escapeCodeTimeout: options.escapeCodeTimeout ?? 500,
		getCursorPos() {
			return terminalEditor
				? terminalEditor.getCursorPos()
				: readlineDisplayPosition(
						String(promptText),
						output?.columns ?? Infinity,
						options.tabSize ?? 8,
					);
		},
		getPrompt() {
			return promptText;
		},
		setPrompt(value) {
			promptText = value;
		},
		prompt(preserveCursor) {
			if (paused) this.resume();
			if (terminalEditor) terminalEditor.prompt(preserveCursor);
			else if (output) output.write(String(promptText));
		},
		write(data, key) {
			if (paused) this.resume();
			if (terminalEditor) terminalEditor.write(data, key);
			else if (data != null) onData(data);
		},
		[Symbol.dispose]() {
			this.close();
		},
		close() {
			if (closed) {
				return;
			}
			this.pause();
			closed = true;
			this.closed = true;
			detachInput();
			constructorAbort?.[Symbol.dispose]();
			constructorAbort = null;
			terminalEditor?.close();
			if (promiseMode) {
				api.off("line", enqueueLine);
				api.off("error", failIterator);
			}
			while (pendingQuestionResolves.length > 0) {
				const resolve = pendingQuestionResolves.shift();
				resolve("");
			}
			for (const waiter of pendingLines.splice(0))
				waiter.resolve({ done: true, value: undefined });
			waiterWarned = false;
			emit("close");
		},
		question(prompt, callback) {
			if (promiseMode) {
				const options = callback;
				return new Promise((resolve, reject) => {
					const signal = options?.signal;
					if (
						signal &&
						(typeof signal.aborted !== "boolean" ||
							typeof signal.addEventListener !== "function" ||
							typeof signal.removeEventListener !== "function")
					) {
						reject(
							Object.assign(
								new TypeError(
									'The "options.signal" argument must be an AbortSignal',
								),
								{ code: "ERR_INVALID_ARG_TYPE" },
							),
						);
						return;
					}
					const abortError = () =>
						Object.assign(
							new Error("The operation was aborted", { cause: signal.reason }),
							{ name: "AbortError", code: "ABORT_ERR" },
						);
					if (signal?.aborted) {
						reject(abortError());
						return;
					}
					if (closed) {
						reject(
							Object.assign(new Error("readline was closed"), {
								code: "ERR_USE_AFTER_CLOSE",
							}),
						);
						return;
					}
					let abortSubscription;
					const abort = () => {
						abortSubscription?.[Symbol.dispose]();
						if (pendingQuestion) {
							pendingQuestion = null;
							promptText = savedPrompt;
							terminalEditor?.resetLine();
							output?.write?.("\r\n");
						}
						reject(abortError());
					};
					if (signal)
						abortSubscription = eventsModule.addAbortListener(signal, abort);
					if (pendingQuestion) {
						api.prompt();
						return;
					}
					savedPrompt = promptText;
					promptText = prompt;
					pendingQuestion = {
						cancel() {
							abortSubscription?.[Symbol.dispose]();
							reject(
								Object.assign(new Error("The operation was aborted"), {
									name: "AbortError",
									code: "ABORT_ERR",
								}),
							);
						},
						answer(value) {
							abortSubscription?.[Symbol.dispose]();
							resolve(value);
						},
					};
					api.prompt();
				});
			}

			if (terminalEditor && typeof callback === "function") {
				if (!pendingQuestion) {
					savedPrompt = promptText;
					promptText = prompt;
					pendingQuestion = { answer: callback };
				}
				api.prompt();
				return;
			}
			if (output && typeof output.write === "function" && prompt) {
				output.write(String(prompt));
			}
			const readAnswer = () => {
				if (queuedLines.length > 0) {
					return Promise.resolve(queuedLines.shift());
				}
				if (closed || ended) {
					return Promise.resolve("");
				}
				return new Promise((resolve) => {
					pendingQuestionResolves.push(resolve);
				});
			};
			if (typeof callback === "function") {
				void readAnswer().then((answer) => {
					callback(answer);
				});
				return;
			}
			return readAnswer();
		},
		[Symbol.asyncIterator]() {
			if (promiseMode && !iteratorActive) {
				api.on("line", enqueueLine);
				api.on("error", failIterator);
			}
			iteratorActive = true;
			return iterator;
		},
	});
	const closeFromTerminal = () => {
		const question = pendingQuestion;
		api.close();
		pendingQuestion = null;
		question?.cancel?.();
	};
	if (terminal) {
		terminalEditor = createReadlineTerminal({
			input,
			output,
			options,
			callbackMode: !promiseMode,
			onLine: emitLine,
			getPrompt: () => promptText,
			onSignal: (signal) => {
				if (api.listenerCount(signal) > 0) emit(signal);
				else if (signal === "SIGINT") closeFromTerminal();
			},
			onClose: closeFromTerminal,
			onHistory: (entries) => emit("history", entries),
		});
		Object.defineProperties(api, {
			line: { enumerable: true, get: () => terminalEditor.line },
			cursor: { enumerable: true, get: () => terminalEditor.cursor },
			history: { enumerable: true, get: () => terminalEditor.history },
		});
	}
	try {
		terminalEditor?.start();
		if (options.signal != null) {
			const signal = options.signal;
			if (
				!signal ||
				typeof signal.aborted !== "boolean" ||
				typeof signal.addEventListener !== "function"
			)
				throw Object.assign(
					new TypeError('The "options.signal" argument must be an AbortSignal'),
					{ code: "ERR_INVALID_ARG_TYPE" },
				);
			constructorAbort = eventsModule.addAbortListener(signal, () =>
				api.close(),
			);
		}

		if (input && typeof input.on === "function") {
			if (terminalEditor) {
				input.on("keypress", onKeypress);
				emitKeypressEvents(input, api);
			} else input.on("data", onData);
			input.on("end", onEnd);
			input.on("error", onError);
			if (typeof input.resume === "function") {
				input.resume();
			}
		}
	} catch (error) {
		try {
			api.close();
		} catch (cleanupError) {
			throw new AggregateError(
				[error, cleanupError],
				"readline initialization and cleanup failed",
			);
		}
		throw error;
	}
	return api;
}

// A fixed record per Interface preserves the shared reader closures while
// allowing subclass overrides and super calls through standard prototypes.
const interfaceMethods = new WeakMap();
const interfaceMethodNames = [
	"pause",
	"resume",
	"getCursorPos",
	"getPrompt",
	"setPrompt",
	"prompt",
	"write",
	"close",
	"question",
	Symbol.dispose,
	Symbol.asyncIterator,
];
function adoptInterfaceMethods(api) {
	const methods = Object.create(null);
	for (const name of interfaceMethodNames) methods[name] = api[name];
	interfaceMethods.set(api, methods);
	for (const name of interfaceMethodNames) delete api[name];
	return api;
}
function installInterfacePrototype(prototype, callbackMode = false) {
	for (const name of interfaceMethodNames) {
		const delegate = function (...args) {
			return Reflect.apply(interfaceMethods.get(this)?.[name], this, args);
		};
		Object.defineProperty(delegate, "name", {
			value: typeof name === "string" ? name : `[${name.description}]`,
			configurable: true,
		});
		Object.defineProperty(delegate, "length", {
			value:
				name === "write" || name === "question"
					? 2
					: name === "prompt" || name === "setPrompt"
						? 1
						: 0,
			configurable: true,
		});
		Object.defineProperty(prototype, name, {
			configurable: true,
			writable: true,
			enumerable: callbackMode && name === "question",
			value: delegate,
		});
	}
}
class Interface extends eventsModule {
	constructor(input, output, completer, terminal) {
		super();
		const options =
			input && typeof input.on === "function"
				? { input, output, completer, terminal }
				: input;
		if (!options?.input || typeof options.input.on !== "function")
			throw new TypeError("readline input must be a readable stream");
		const api = createReadlineInterface(options, true);
		return Object.setPrototypeOf(
			adoptInterfaceMethods(api),
			new.target.prototype,
		);
	}
}

installInterfacePrototype(Interface.prototype);

function invalidInteger(
	value,
	name,
	min = Number.MIN_SAFE_INTEGER,
	max = Number.MAX_SAFE_INTEGER,
) {
	if (typeof value !== "number")
		throw Object.assign(
			new TypeError(`The "${name}" argument must be a number`),
			{ code: "ERR_INVALID_ARG_TYPE" },
		);
	if (!Number.isInteger(value) || value < min || value > max)
		throw Object.assign(
			new RangeError(`The "${name}" argument is out of range`),
			{ code: "ERR_OUT_OF_RANGE" },
		);
}

class Readline {
	#stream;
	#autoCommit = false;
	#pending = "";
	#warned = false;
	constructor(stream, options) {
		if (
			!stream ||
			typeof stream.write !== "function" ||
			typeof stream.on !== "function" ||
			stream.writable === false
		)
			throw Object.assign(
				new TypeError('The "stream" argument must be a Writable'),
				{ code: "ERR_INVALID_ARG_TYPE" },
			);
		if (options?.autoCommit != null && typeof options.autoCommit !== "boolean")
			throw Object.assign(
				new TypeError('The "options.autoCommit" argument must be a boolean'),
				{ code: "ERR_INVALID_ARG_TYPE" },
			);
		this.#stream = stream;
		this.#autoCommit = options?.autoCommit ?? false;
	}
	#append(data) {
		if (this.#autoCommit) {
			process_default.nextTick(() => this.#stream.write(data));
			return this;
		}
		// Cursor sequences are ASCII, so string length is also their byte charge.
		if (data.length > BUFFER_MAX_STRING_LENGTH - this.#pending.length)
			throw Object.assign(
				new RangeError(
					"readline pending output exceeds Buffer.constants.MAX_STRING_LENGTH; commit() or rollback() before adding more actions",
				),
				{ code: "ERR_STRING_TOO_LONG" },
			);
		if (
			!this.#warned &&
			this.#pending.length + data.length >= BUFFER_MAX_STRING_LENGTH * 0.8
		) {
			this.#warned = true;
			console.warn(
				"readline pending output approaching Buffer.constants.MAX_STRING_LENGTH; commit() or rollback() pending actions",
			);
		}
		this.#pending += data;
		return this;
	}
	cursorTo(x, y) {
		invalidInteger(x, "x");
		if (y != null) invalidInteger(y, "y");
		return this.#append(
			y == null ? `\x1b[${x + 1}G` : `\x1b[${y + 1};${x + 1}H`,
		);
	}
	moveCursor(dx, dy) {
		if (!dx && !dy) return this;
		invalidInteger(dx, "dx");
		invalidInteger(dy, "dy");
		return this.#append(
			(dx < 0 ? `\x1b[${-dx}D` : dx > 0 ? `\x1b[${dx}C` : "") +
				(dy < 0 ? `\x1b[${-dy}A` : dy > 0 ? `\x1b[${dy}B` : ""),
		);
	}
	clearLine(dir) {
		invalidInteger(dir, "dir", -1, 1);
		return this.#append(dir < 0 ? "\x1b[1K" : dir > 0 ? "\x1b[0K" : "\x1b[2K");
	}
	clearScreenDown() {
		return this.#append("\x1b[0J");
	}
	commit() {
		return new Promise((resolve) => {
			this.#stream.write(this.#pending, resolve);
			this.#pending = "";
			this.#warned = false;
		});
	}
	rollback() {
		this.#pending = "";
		this.#warned = false;
		return this;
	}
}

const builtinReadlinePromisesModule = {
	Interface,
	Readline,
	createInterface: (...args) => new Interface(...args),
};
const CallbackInterface = function Interface(
	input,
	output,
	completer,
	terminal,
) {
	if (!(this instanceof CallbackInterface))
		return new CallbackInterface(input, output, completer, terminal);
	const options =
		input && typeof input.on === "function"
			? { input, output, completer, terminal }
			: input;
	if (!options?.input || typeof options.input.on !== "function")
		throw new TypeError("readline input must be a readable stream");
	eventsModule.call(this);
	const api = createReadlineInterface(options, false, this);
	adoptInterfaceMethods(api);
};
installInterfacePrototype(CallbackInterface.prototype, true);
Object.defineProperty(CallbackInterface, "name", {
	value: "Interface",
	configurable: true,
});
Object.setPrototypeOf(CallbackInterface.prototype, eventsModule.prototype);
Object.setPrototypeOf(CallbackInterface, eventsModule);
const builtinReadlineModule = {
	Interface: CallbackInterface,
	createInterface: (...args) => new CallbackInterface(...args),
	emitKeypressEvents,
	cursorTo,
	moveCursor,
	clearLine,
	clearScreenDown,
	promises: builtinReadlinePromisesModule,
};
export { builtinReadlineModule, builtinReadlinePromisesModule };
