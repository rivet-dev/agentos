fn javascript_execution_v8_readline_promises_contract() {
    let temp = tempdir().expect("create temp dir");
    let mut engine = support::javascript_engine();
    let context = engine.create_context(CreateJavascriptContextRequest {
        vm_id: String::from("vm-js"),
        bootstrap_module: None,
        compile_cache_root: None,
    });

    let execution = engine
        .start_execution(StartJavascriptExecutionRequest {
            limits: Default::default(),
            argv0: None,
            guest_runtime: Default::default(),
            vm_id: String::from("vm-js"),
            context_id: context.context_id,
            argv: vec![String::from("./entry.mjs")],
            env: BTreeMap::new(),
            cwd: temp.path().to_path_buf(),
            wasm_module_bytes: None,
            inline_code: Some(String::from(
                r#"
import assert from "node:assert/strict";
import readline, { promises } from "node:readline";
import rp, {
	Interface,
	Readline,
	createInterface,
} from "node:readline/promises";
import { createRequire } from "node:module";
import { PassThrough, Writable } from "node:stream";
const require = createRequire("/entry.mjs");
let contractPhase = "exports and question lifecycle";
const contractDeadline = setTimeout(() => {
	throw new Error(`readline contract exceeded 3s during ${contractPhase}`);
}, 3000);
try {
	assert.equal(require("readline/promises"), require("node:readline/promises"));
	assert.equal(rp, require("readline/promises"));
	assert.equal(readline.promises, rp);
	assert.equal(promises, rp);
	assert.equal(createInterface, rp.createInterface);
	assert.equal(Interface, rp.Interface);
	assert.equal(Readline, rp.Readline);
	const input = new PassThrough(),
		output = new PassThrough();
	let written = "";
	output.on("data", (chunk) => (written += chunk.toString()));
	const rl = createInterface({ input, output, terminal: false });
	assert(rl instanceof Interface);
	const lines = [];
	rl.on("line", (line) => lines.push(line));
	const answer = rl.question("answer?");
	input.write("alpha\n");
	assert.equal(await answer, "alpha");
	assert.deepEqual(lines, []);
	const controller = new AbortController();
	const reason = { synthetic: true };
	const aborted = rl.question("abort?", { signal: controller.signal }).then(
		() => assert.fail("answered aborted question"),
		(error) => {
			assert.equal(error.name, "AbortError");
			assert.equal(error.code, "ABORT_ERR");
			assert.equal(error.cause, reason);
		},
	);
	controller.abort(reason);
	await aborted;
	const next = rl.question("next?");
	input.write("beta\n");
	assert.equal(await next, "beta");
	const pre = new AbortController();
	pre.abort("already");
	await assert.rejects(rl.question("never", { signal: pre.signal }), {
		code: "ABORT_ERR",
		cause: "already",
	});
	let settled = false;
	rl.question("pending?").then(
		() => (settled = true),
		() => (settled = true),
	);
	rl.close();
	await Promise.resolve();
	assert.equal(settled, false);
	assert.equal(input.listenerCount("data"), 0);
	assert.equal(input.listenerCount("end"), 0);
	await assert.rejects(rl.question("closed"), { code: "ERR_USE_AFTER_CLOSE" });
	assert.equal(written, "answer?abort?\r\nnext?pending?");
	input.destroy();
	output.destroy();
	contractPhase = "cursor commit";
	const chunks = [];
	const sink = new Writable({
		write(chunk, encoding, callback) {
			chunks.push(chunk.toString());
			callback();
		},
	});
	const cursor = new Readline(sink);
	assert.equal(
		cursor.cursorTo(2, 3).moveCursor(-2, 1).clearLine(0).clearScreenDown(),
		cursor,
	);
	await cursor.commit();
	cursor.cursorTo(0).rollback();
	await cursor.commit();
	assert.deepEqual(chunks, ["\x1b[4;3H\x1b[2D\x1b[1B\x1b[2K\x1b[0J", ""]);
	sink.destroy();

	// Ordinary EventEmitter methods, pause/resume, original error and cleanup.
	contractPhase = "Ordinary EventEmitter methods, pause/resume, original error and cleanup.";
	const errorInput = new PassThrough(),
		errorRl = createInterface({ input: errorInput });
	const eventsSeen = [];
	errorRl.on("pause", () => eventsSeen.push("pause"));
	errorRl.on("resume", () => eventsSeen.push("resume"));
	assert.equal(errorRl.pause(), errorRl);
	assert.equal(errorInput.isPaused(), true);
	errorRl.pause();
	assert.equal(errorRl.resume(), errorRl);
	errorRl.resume();
	assert.deepEqual(eventsSeen, ["pause", "resume"]);
	let receivedError;
	const originalError = new Error("synthetic-readline-input");
	errorRl.on("error", (error) => (receivedError = error));
	errorInput.emit("error", originalError);
	assert.equal(receivedError, originalError);
	let eventValue;
	errorRl.once("custom", (value) => (eventValue = value));
	assert.equal(errorRl.listenerCount("custom"), 1);
	errorRl.emit("custom", 42);
	assert.equal(eventValue, 42);
	assert.equal(errorRl.listenerCount("custom"), 0);
	errorRl.close();
	assert.equal(errorInput.listenerCount("error"), 0);
	errorInput.destroy();
	// Concurrent next calls remain FIFO and every waiter settles at EOF.
	contractPhase = "Concurrent next calls remain FIFO and every waiter settles at EOF.";
	const concurrentInput = new PassThrough(),
		concurrentRl = createInterface({ input: concurrentInput }),
		concurrent = concurrentRl[Symbol.asyncIterator]();
	const pending = [concurrent.next(), concurrent.next(), concurrent.next()];
	concurrentInput.end("first\nsecond\n");
	assert.deepEqual(await Promise.all(pending), [
		{ value: "first", done: false },
		{ value: "second", done: false },
		{ value: undefined, done: true },
	]);
	concurrentInput.destroy();
	// Pausing at the iterator watermark must retain the remainder of an admitted chunk.
	contractPhase = "Pausing at the iterator watermark must retain the remainder of an admitted chunk.";
	const bulkInput = new PassThrough(),
		bulkRl = createInterface({ input: bulkInput }),
		bulk = bulkRl[Symbol.asyncIterator]();
	const expected = Array.from({ length: 1027 }, (_, i) => String(i));
	bulkInput.end(expected.join("\n"));
	const actual = [];
	for await (const line of bulk) actual.push(line);
	assert.deepEqual(actual, expected);
	assert.equal(bulkInput.listenerCount("data"), 0);
	assert.equal(bulkInput.listenerCount("error"), 0);
	bulkInput.destroy();

	// EOF partial fragments are line events, not question answers.
	contractPhase = "EOF partial fragments are line events, not question answers.";
	const eofInput = new PassThrough(),
		eofRl = createInterface({ input: eofInput });
	let eofAnswer = "pending";
	const eofLines = [];
	eofRl.on("line", (line) => eofLines.push(line));
	eofRl.question("x").then((answer) => (eofAnswer = answer));
	eofInput.end("partial");
	await new Promise((resolve) => setImmediate(resolve));
	assert.equal(eofAnswer, "pending");
	assert.deepEqual(eofLines, ["partial"]);
	eofRl.close();
	eofInput.destroy();
	// CR, CRLF split across chunks, and final CR follow the same nonterminal parser.
	contractPhase = "CR, CRLF split across chunks, and final CR follow the same nonterminal parser.";
	const crInput = new PassThrough(),
		crRl = createInterface({
			input: crInput,
			terminal: false,
			crlfDelay: Infinity,
		});
	const crLines = [];
	crRl.on("line", (line) => crLines.push(line));
	crInput.write("a\r");
	crInput.write("b\r");
	crInput.end("\nc\r");
	await new Promise((resolve) => setImmediate(resolve));
	assert.deepEqual(crLines, ["a", "b", "c"]);
	crRl.close();
	crInput.destroy();
	// Idempotent pause/resume no-ops and terminal closed errors.
	contractPhase = "Idempotent pause/resume no-ops and terminal closed errors.";
	const pauseInput = new PassThrough(),
		pauseRl = createInterface({ input: pauseInput });
	pauseRl.pause();
	assert.equal(pauseRl.pause(), undefined);
	pauseRl.resume();
	assert.equal(pauseRl.resume(), undefined);
	pauseRl.close();
	assert.throws(() => pauseRl.pause(), { code: "ERR_USE_AFTER_CLOSE" });
	assert.throws(() => pauseRl.resume(), { code: "ERR_USE_AFTER_CLOSE" });
	pauseInput.destroy();
	// Returning an iterator detaches that iterator without closing its Interface.
	contractPhase = "Returning an iterator detaches that iterator without closing its Interface.";
	const returnInput = new PassThrough(),
		returnRl = createInterface({ input: returnInput });
	const afterReturn = [];
	returnRl.on("line", (line) => afterReturn.push(line));
	await returnRl[Symbol.asyncIterator]().return();
	returnInput.write("after\n");
	await new Promise((resolve) => setImmediate(resolve));
	assert.notEqual(returnRl.closed, true);
	assert.deepEqual(afterReturn, ["after"]);
	assert.equal(returnInput.listenerCount("data"), 1);
	returnRl.close();
	returnInput.destroy();

	// Public nonterminal Interface methods and constructor abort lifetime.
	contractPhase = "Public nonterminal Interface methods and constructor abort lifetime.";
	const methodInput = new PassThrough(),
		methodOutput = new PassThrough();
	let methodWritten = "";
	methodOutput.on("data", (x) => (methodWritten += x.toString()));
	const methodRl = createInterface({
		input: methodInput,
		output: methodOutput,
		terminal: false,
	});
	assert.equal(methodRl.input, methodInput);
	assert.equal(methodRl.output, methodOutput);
	assert.equal(methodRl.getPrompt(), "> ");
	methodRl.setPrompt("custom> ");
	assert.equal(methodRl.getPrompt(), "custom> ");
	methodRl.prompt();
	assert.equal(methodWritten, "custom> ");
	const injected = methodRl.question("write?");
	methodRl.write("synthetic\n");
	assert.equal(await injected, "synthetic");
	assert.equal(methodRl.getPrompt(), "custom> ");
	methodRl[Symbol.dispose]();
	assert.equal(methodInput.listenerCount("data"), 0);
	methodInput.destroy();
	methodOutput.destroy();
	const sigInput = new PassThrough(),
		constructorController = new AbortController();
	constructorController.signal.addEventListener("abort", (event) =>
		event.stopImmediatePropagation(),
	);
	const sigRl = createInterface({
		input: sigInput,
		signal: constructorController.signal,
	});
	constructorController.abort("close");
	assert.equal(sigRl.closed, true);
	assert.equal(sigInput.isPaused(), true);
	assert.equal(sigInput.listenerCount("data"), 0);
	sigInput.destroy();
	// Protected abort listener remains effective after an ordinary listener stops propagation.
	contractPhase = "Protected abort listener remains effective after an ordinary listener stops propagation.";
	const protectInput = new PassThrough(),
		protectRl = createInterface({ input: protectInput }),
		protectController = new AbortController();
	protectController.signal.addEventListener("abort", (event) =>
		event.stopImmediatePropagation(),
	);
	const protectQuestion = protectRl.question("x", {
		signal: protectController.signal,
	});
	protectController.abort("protected-reason");
	protectInput.write("not-an-answer\n");
	await assert.rejects(protectQuestion, {
		code: "ABORT_ERR",
		cause: "protected-reason",
	});
	protectRl.close();
	protectInput.destroy();
	const repeatInput = new PassThrough(),
		repeatOutput = new PassThrough();
	let repeatedPrompt = "";
	repeatOutput.on("data", (chunk) => (repeatedPrompt += chunk.toString()));
	const repeatRl = createInterface({
		input: repeatInput,
		output: repeatOutput,
		terminal: false,
	});
	repeatRl.question("first");
	repeatRl.question("second");
	assert.equal(repeatedPrompt, "firstfirst");
	repeatRl.close();
	repeatInput.destroy();
	repeatOutput.destroy();

	// Prelude-owned protected listeners preserve normal receiver and scrub bootstrap access.
	contractPhase = "Prelude-owned protected listeners preserve normal receiver and scrub bootstrap access.";
	assert.equal(
		Object.hasOwn(globalThis, "__agentOsAddProtectedEventListener"),
		false,
	);
	const { addAbortListener } = require("node:events");
	const receiverController = new AbortController();
	let abortReceiver, abortEvent;
	addAbortListener(receiverController.signal, function (event) {
		abortReceiver = this;
		abortEvent = event;
	});
	receiverController.abort();
	assert.equal(abortReceiver, receiverController.signal);
	assert.equal(abortEvent.target, receiverController.signal);
	let preAbortedThis = "unset",
		preAbortedEvent = "unset",
		preAbortedArgumentCount = -1;
	addAbortListener(receiverController.signal, function (event) {
		preAbortedThis = this;
		preAbortedEvent = event;
		preAbortedArgumentCount = arguments.length;
	});
	await Promise.resolve();
	assert.equal(preAbortedThis, undefined);
	assert.equal(preAbortedEvent, undefined);
	assert.equal(preAbortedArgumentCount, 0);
	// CR timing belongs to receipt, even while an admitted chunk exceeds the line watermark.
	contractPhase = "CR timing belongs to receipt, even while an admitted chunk exceeds the line watermark.";
	const timedInput = new PassThrough(),
		timedRl = createInterface({
			input: timedInput,
			terminal: false,
			crlfDelay: 100,
		}),
		timedIterator = timedRl[Symbol.asyncIterator]();
	timedInput.write("x\n".repeat(1024) + "tail\r");
	await new Promise((resolve) => setTimeout(resolve, 140));
	timedInput.end("\n");
	const timedLines = [];
	for await (const line of timedIterator) timedLines.push(line);
	assert.equal(timedLines.length, 1026);
	assert.deepEqual(timedLines.slice(-3), ["x", "tail", ""]);
	timedRl.close();
	timedInput.destroy();

	// Terminal selection and editing share the public Interface lifetime.
	contractPhase = "Terminal selection and editing share the public Interface lifetime.";
	const savedTerm = process.env.TERM;
	process.env.TERM = "xterm-256color";
	function terminalStreams() {
		const input = new PassThrough(),
			output = new PassThrough(),
			raw = [];
		output.isTTY = true;
		output.columns = 80;
		output.on("data", () => {});
		input.setRawMode = (mode) => {
			raw.push(mode);
			return input;
		};
		return { input, output, raw };
	}
	for (const chunks of [
		["ab\x7fc\r"],
		["abcd", "\x1b[D", "X\r"],
		[Buffer.from([0xc3]), Buffer.from([0xa9]), "\r"],
	]) {
		const io = terminalStreams(),
			r = createInterface({ input: io.input, output: io.output }),
			events = [];
		r.on("line", (line) => events.push(line));
		const answer = r.question("question> ");
		for (const chunk of chunks) io.input.write(chunk);
		assert.equal(
			await answer,
			chunks.length === 1 ? "ac" : typeof chunks[0] === "string" ? "abcXd" : "é",
		);
		assert.deepEqual(events, []);
		assert.deepEqual(r.getCursorPos(), { cols: 2, rows: 0 });
		r.close();
		assert.deepEqual(io.raw, [true, false]);
		assert.equal(io.input.listenerCount("error"), 0);
		assert.equal(io.output.listenerCount("resize"), 0);
		io.input.destroy();
		io.output.destroy();
	}
	const explicitIO = terminalStreams(),
		explicitRl = createInterface({
			input: explicitIO.input,
			output: explicitIO.output,
			terminal: true,
		});
	const explicitAnswer = explicitRl.question("edit> ");
	explicitRl.write("abcd");
	explicitRl.write(undefined, { name: "left" });
	explicitRl.write("X");
	explicitRl.write(undefined, { name: "return" });
	assert.equal(await explicitAnswer, "abcXd");
	explicitRl.close();
	explicitIO.input.destroy();
	explicitIO.output.destroy();
	// Terminal abort resets editing state and constructor abort retires raw/resize owners.
	contractPhase = "Terminal abort resets editing state and constructor abort retires raw/resize owners.";
	const abortIO = terminalStreams(),
		abortController = new AbortController(),
		abortRl = createInterface({
			input: abortIO.input,
			output: abortIO.output,
			signal: abortController.signal,
		});
	const questionController = new AbortController(),
		terminalQuestion = abortRl.question("abort> ", {
			signal: questionController.signal,
		});
	abortIO.input.write("partial");
	questionController.abort("terminal-reason");
	await assert.rejects(terminalQuestion, {
		code: "ABORT_ERR",
		cause: "terminal-reason",
	});
	assert.equal(abortRl.line, "");
	abortController.abort();
	assert.deepEqual(abortIO.raw, [true, false]);
	assert.equal(abortIO.output.listenerCount("resize"), 0);
	abortIO.input.destroy();
	abortIO.output.destroy();
	// Ctrl-P uses ordinary history; Up has an exhausted-search sentinel.
	contractPhase = "Ctrl-P uses ordinary history; Up has an exhausted-search sentinel.";
	const historyIO = terminalStreams(),
		historyRl = createInterface({
			input: historyIO.input,
			output: historyIO.output,
			history: ["new", "old"],
		});
	historyRl.write("draft");
	historyRl.write(undefined, { name: "p", ctrl: true });
	assert.equal(historyRl.line, "new");
	historyRl.close();
	historyIO.input.destroy();
	historyIO.output.destroy();
	const sentinelIO = terminalStreams(),
		sentinelRl = createInterface({
			input: sentinelIO.input,
			output: sentinelIO.output,
			history: ["new", "old"],
		});
	const historyStates = [];
	for (const name of ["up", "up", "up", "down"]) {
		sentinelRl.write(undefined, { name });
		historyStates.push(sentinelRl.line);
	}
	assert.deepEqual(historyStates, ["new", "old", "", "old"]);
	sentinelRl.close();
	sentinelIO.input.destroy();
	sentinelIO.output.destroy();
	// Callback and promises interfaces deliberately differ for dumb terminal editing in Node.
	contractPhase = "Callback and promises interfaces deliberately differ for dumb terminal editing in Node.";
	process.env.TERM = "dumb";
	for (const [module, expectedLine] of [
		[readline, "ab\x7fc"],
		[rp, "ac"],
	]) {
		const io = terminalStreams(),
			r = module.createInterface({
				input: io.input,
				output: io.output,
				terminal: true,
			}),
			lines = [];
		r.on("line", (line) => lines.push(line));
		io.input.write("ab\x7fc\r");
		assert.deepEqual(lines, [expectedLine]);
		r.close();
		assert.deepEqual(io.raw, [true, false]);
		io.input.destroy();
		io.output.destroy();
	}
	const callbackAbortIO = terminalStreams(),
		callbackAbortController = new AbortController(),
		callbackAbortRl = readline.createInterface({
			input: callbackAbortIO.input,
			output: callbackAbortIO.output,
			terminal: true,
			signal: callbackAbortController.signal,
		});
	callbackAbortController.abort();
	assert.equal(callbackAbortRl.closed, true);
	assert.deepEqual(callbackAbortIO.raw, [true, false]);
	assert.equal(callbackAbortIO.output.listenerCount("resize"), 0);
	callbackAbortIO.input.destroy();
	callbackAbortIO.output.destroy();
	const terminalEofIO = terminalStreams(),
		terminalEofRl = createInterface({
			input: terminalEofIO.input,
			output: terminalEofIO.output,
			terminal: true,
		});
	const terminalEofLines = [];
	let terminalEofAnswer = "pending";
	terminalEofRl.on("line", (line) => terminalEofLines.push(line));
	terminalEofRl.question("eof> ").then((answer) => (terminalEofAnswer = answer));
	terminalEofIO.input.end("fragment");
	await new Promise((resolve) => setImmediate(resolve));
	assert.deepEqual(terminalEofLines, ["fragment"]);
	assert.equal(terminalEofAnswer, "pending");
	assert.deepEqual(terminalEofIO.raw, [true, false]);
	terminalEofIO.input.destroy();
	terminalEofIO.output.destroy();
	// Terminal EOF/interrupt keys reject a question; direct close retains Node's pending behavior.
	contractPhase = "Terminal EOF/interrupt keys reject a question; direct close retains Node's pending behavior.";
	for (const key of ["d", "c"]) {
		const io = terminalStreams();
		const r = createInterface({
			input: io.input,
			output: io.output,
			terminal: true,
		});
		const question = r.question("cancel> ");
		r.write(undefined, { name: key, ctrl: true });
		await assert.rejects(question, { name: "AbortError", code: "ABORT_ERR" });
		assert.equal(r.closed, true);
		assert.deepEqual(io.raw, [true, false]);
		assert.equal(io.output.listenerCount("resize"), 0);
		io.input.destroy();
		io.output.destroy();
	}
	if (savedTerm === undefined) delete process.env.TERM;
	else process.env.TERM = savedTerm;
	// Callback terminal EOF also reaches its iterator without answering a question.
	contractPhase = "Callback terminal EOF also reaches its iterator without answering a question.";
	const callbackEofIO = terminalStreams();
	const callbackEofRl = readline.createInterface({
		input: callbackEofIO.input,
		output: callbackEofIO.output,
		terminal: true,
	});
	const callbackEofEvents = [];
	let callbackEofAnswered = false;
	callbackEofRl.on("line", (line) => callbackEofEvents.push(line));
	callbackEofRl.question("eof? ", () => {
		callbackEofAnswered = true;
	});
	const callbackEofIterator = callbackEofRl[Symbol.asyncIterator]();
	const callbackEofNext = callbackEofIterator.next();
	callbackEofIO.input.end("fragment");
	assert.deepEqual(await callbackEofNext, { value: "fragment", done: false });
	assert.deepEqual(await callbackEofIterator.next(), {
		value: undefined,
		done: true,
	});
	assert.deepEqual(callbackEofEvents, ["fragment"]);
	assert.equal(callbackEofAnswered, false);
	assert.deepEqual(callbackEofIO.raw, [true, false]);
	callbackEofIO.input.destroy();
	callbackEofIO.output.destroy();
	// Constructor cancellation pauses nonterminal callback input as well as detaching it.
	contractPhase = "Constructor cancellation pauses nonterminal callback input as well as detaching it.";
	const plainAbortInput = new PassThrough();
	const plainAbortController = new AbortController();
	const plainAbortRl = readline.createInterface({
		input: plainAbortInput,
		terminal: false,
		signal: plainAbortController.signal,
	});
	plainAbortController.abort();
	assert.equal(plainAbortRl.closed, true);
	assert.equal(plainAbortInput.isPaused(), true);
	assert.equal(plainAbortInput.listenerCount("data"), 0);
	assert.equal(plainAbortInput.listenerCount("error"), 0);
	plainAbortInput.destroy();
	const manualCloseInput = new PassThrough();
	const manualCloseRl = readline.createInterface({
		input: manualCloseInput,
		terminal: false,
	});
	manualCloseRl.close();
	assert.equal(manualCloseInput.isPaused(), true);
	assert.equal(manualCloseInput.listenerCount("data"), 0);
	manualCloseInput.destroy();
	console.log("readline-contract-complete");
} finally {
	clearTimeout(contractDeadline);
}
"#,
            )),
        })
        .expect("start JavaScript execution");

    let result = execution.wait().expect("wait for JavaScript execution");
    let stdout = String::from_utf8(result.stdout).expect("stdout utf8");
    let stderr = String::from_utf8(result.stderr).expect("stderr utf8");
    assert_eq!(result.exit_code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(stdout.trim(), "readline-contract-complete");
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");
}
