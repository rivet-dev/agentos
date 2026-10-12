fn javascript_execution_v8_readline_callbacks_contract() {
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
import readline, {
	Interface,
	emitKeypressEvents,
	cursorTo,
	moveCursor,
	clearLine,
	clearScreenDown,
} from "node:readline";
import { createRequire } from "node:module";
import { PassThrough, Writable } from "node:stream";
const require = createRequire("/entry.mjs");
const deadline = setTimeout(() => {
	throw new Error("readline callback contract exceeded3s");
}, 3000);
try {
	for (const [name, value] of Object.entries({
		Interface,
		emitKeypressEvents,
		cursorTo,
		moveCursor,
		clearLine,
		clearScreenDown,
	})) {
		assert.equal(typeof value, "function");
		assert.equal(value.name, name);
		assert.equal(value, readline[name]);
		assert.equal(value, require("readline")[name]);
		assert.equal(value, require("node:readline")[name]);
	}
	for (const how of ["call", "new", "create", "inherit"]) {
		const input = new PassThrough();
		let r;
		if (how === "call") r = Interface(input);
		if (how === "new") r = new Interface(input, undefined, undefined, false);
		if (how === "create")
			r = readline.createInterface({ input, terminal: false });
		if (how === "inherit") {
			function Derived() {
				Interface.call(this, { input, terminal: false });
			}
			Object.setPrototypeOf(Derived.prototype, Interface.prototype);
			r = new Derived();
			assert(r instanceof Derived);
		}
		assert(r instanceof Interface);
		assert.equal(Interface.name, "Interface");
		const lines = [];
		r.on("line", (line) => lines.push(line));
		input.write("one\n");
		assert.deepEqual(lines, ["one"]);
		r.close();
		assert.equal(input.isPaused(), true);
		input.destroy();
	}
	// Prototype delegation preserves normal subclass overrides and super calls.
	for (const Base of [Interface, readline.promises.Interface]) {
		const input = new PassThrough();
		let closes = 0,
			pauses = 0;
		class Derived extends Base {
			close() {
				closes++;
				return super.close();
			}
			pause() {
				pauses++;
				return super.pause();
			}
		}
		const r = new Derived({ input, terminal: false });
		r.close();
		assert.equal(closes, 1);
		assert.equal(pauses, 1);
		assert.equal(input.isPaused(), true);
		input.destroy();
	}
	const writes = [],
		writeFailure = new Error("synthetic-write-failure");
	let callbackFailure;
	const sink = {
		write(data, callback) {
			writes.push(data);
			process.nextTick(() => callback?.(writeFailure));
			return false;
		},
	};
	assert.equal(
		cursorTo(sink, 2, 3, (error) => (callbackFailure = error)),
		false,
	);
	assert.equal(moveCursor(sink, -2, 1), false);
	assert.equal(clearLine(sink, -1), false);
	assert.equal(clearScreenDown(sink), false);
	await new Promise((resolve) => process.nextTick(resolve));
	assert.equal(callbackFailure, writeFailure);
	assert.deepEqual(writes, [
		"\x1b[4;3H",
		"\x1b[2D\x1b[1B",
		"\x1b[1K",
		"\x1b[0J",
	]);
	let noOp = false;
	assert.equal(
		moveCursor(null, 0, 0, function (error) {
			assert.equal(this, undefined);
			assert.equal(arguments.length, 1);
			assert.equal(error, null);
			noOp = true;
		}),
		true,
	);
	assert.equal(noOp, false);
	await new Promise((resolve) => process.nextTick(resolve));
	assert.equal(noOp, true);
	assert.throws(() => cursorTo(sink, NaN), { code: "ERR_INVALID_ARG_VALUE" });
	assert.throws(() => cursorTo(sink, "x", 2), {
		code: "ERR_INVALID_CURSOR_POS",
	});
	assert.throws(() => clearLine(null, 0, 3), { code: "ERR_INVALID_ARG_TYPE" });
	assert.throws(
		() =>
			cursorTo(
				{
					write() {
						throw writeFailure;
					},
				},
				1,
			),
		(error) => error === writeFailure,
	);
	const asyncChunks = [];
	const asyncSink = new Writable({
		highWaterMark: 1,
		write(chunk, encoding, callback) {
			asyncChunks.push(chunk.toString());
			setImmediate(callback);
		},
	});
	let complete;
	const completion = new Promise((resolve) => (complete = resolve));
	assert.equal(
		cursorTo(asyncSink, 0, () => complete()),
		false,
	);
	await completion;
	assert.deepEqual(asyncChunks, ["\x1b[1G"]);
	asyncSink.destroy();
	const input = new PassThrough(),
		keys = [];
	emitKeypressEvents(input, { escapeCodeTimeout: 5 });
	assert.equal(input.listenerCount("data"), 0);
	const keyListener = (text, key) => keys.push([text, key.name]);
	input.on("keypress", keyListener);
	const initialDataListeners = input.listenerCount("data");
	emitKeypressEvents(input);
	assert.equal(input.listenerCount("data"), initialDataListeners);
	input.write(Buffer.from([0xc3]));
	input.write(Buffer.from([0xa9]));
	input.write("\x1b[");
	input.write("D");
	assert.deepEqual(keys, [
		["é", undefined],
		[undefined, "left"],
	]);
	input.removeListener("keypress", keyListener);
	input.write("ignored");
	assert.equal(input.listenerCount("data"), 0);
	input.on("keypress", keyListener);
	input.write("z");
	assert.deepEqual(keys.at(-1), ["z", "z"]);
	const expectedError = new Error("synthetic-key-listener");
	const throwing = () => {
		throw expectedError;
	};
	input.removeListener("keypress", keyListener);
	input.on("keypress", throwing);
	assert.throws(
		() => input.emit("data", Buffer.from("a")),
		(error) => error === expectedError,
	);
	input.removeListener("keypress", throwing);
	input.on("keypress", keyListener);
	input.write("b");
	assert.deepEqual(keys.at(-1), ["b", "b"]);
	input.destroy();
	for (const publicFirst of [true, false]) {
		const input = new PassThrough(),
			output = new PassThrough();
		output.isTTY = true;
		output.columns = 80;
		output.on("data", () => {});
		input.setRawMode = () => input;
		const seen = [],
			lines = [];
		let r;
		const observe = (text, key) => seen.push([text, key.name, r?.line]);
		if (publicFirst) {
			emitKeypressEvents(input);
			input.on("keypress", observe);
		}
		r = readline.createInterface({ input, output, terminal: true });
		if (!publicFirst) {
			input.on("keypress", observe);
			emitKeypressEvents(input);
		}
		r.on("line", (line) => lines.push(line));
		input.write("a");
		assert.equal(seen.length, 1);
		assert.equal(seen[0][2], publicFirst ? "" : "a");
		input.write("\r");
		assert.deepEqual(lines, ["a"]);
		r.close();
		input.resume();
		input.write("z");
		assert.equal(seen.length, 3);
		assert.equal(seen[2][0], "z");
		input.destroy();
		output.destroy();
	}
	const endInput = new PassThrough(),
		endKeys = [];
	emitKeypressEvents(endInput, { escapeCodeTimeout: 5 });
	endInput.on("keypress", (text, key) => endKeys.push([text, key.name]));
	endInput.end("\x1b");
	await new Promise((resolve) => setTimeout(resolve, 15));
	assert.deepEqual(endKeys, [[undefined, "escape"]]);
	endInput.destroy();
	const utfInput = new PassThrough(),
		utfKeys = [];
	emitKeypressEvents(utfInput);
	utfInput.on("keypress", (text) => utfKeys.push(text));
	utfInput.end(Buffer.from([0x61, 0xe2]));
	await new Promise((resolve) => setImmediate(resolve));
	assert.deepEqual(utfKeys, ["a"]);
	utfInput.destroy();
	// Wrapping must retain the original chunk boundary while sharing the key parser.
	const originalTerm = process.env.TERM;
	process.env.TERM = "xterm";
	for (const publicFirst of [false, true]) {
		const wrapInput = new PassThrough(),
			wrapOutput = new PassThrough();
		wrapOutput.isTTY = true;
		wrapOutput.columns = 4;
		wrapInput.setRawMode = () => wrapInput;
		let wrappedText = "",
			wrappedAnswer;
		wrapOutput.on("data", (chunk) => (wrappedText += chunk.toString()));
		if (publicFirst) emitKeypressEvents(wrapInput);
		const wrapRl = readline.createInterface({
			input: wrapInput,
			output: wrapOutput,
			terminal: true,
		});
		wrapRl.question("question> ", (answer) => (wrappedAnswer = answer));
		wrapInput.write("ab\x7fc\r");
		assert.equal(wrappedAnswer, "ac");
		assert.equal(
			wrappedText,
			publicFirst
				? "\u001b[1G\u001b[0Jquestion> \u001b[3Ga\u001b[2A\u001b[1G\u001b[0Jquestion> ab \u001b[1G\u001b[3A\u001b[1G\u001b[0Jquestion> a\u001b[4G\u001b[2A\u001b[1G\u001b[0Jquestion> ac \u001b[1G\r\n"
				: "\u001b[1G\u001b[0Jquestion> \u001b[3Gab\u001b[2A\u001b[1G\u001b[0Jquestion> a\u001b[4Gc\r\n",
		);
		wrapRl.close();
		wrapInput.destroy();
		wrapOutput.destroy();
	}
	assert.equal(typeof _getPendingTimerCount, "function");
	// Closing the last Interface inside a key callback must not rearm tail work.
	for (const external of [false, true]) {
		const input = new PassThrough(),
			output = new PassThrough();
		output.isTTY = true;
		output.columns = 80;
		output.on("data", () => {});
		input.setRawMode = () => input;
		const rl = readline.createInterface({
			input,
			output,
			terminal: true,
			escapeCodeTimeout: 100,
		});
		const keys = [],
			lines = [];
		if (external) input.on("keypress", (text, key) => keys.push(key.name));
		rl.on("line", (line) => {
			lines.push(line);
			rl.close();
		});
		const timersBefore =
			typeof _getPendingTimerCount === "function"
				? _getPendingTimerCount()
				: null;
		input.emit("data", Buffer.from("a\r\x1b"));
		assert.deepEqual(lines, ["a"]);
		assert.equal(rl.closed, true);
		if (timersBefore !== null)
			assert.equal(_getPendingTimerCount(), timersBefore + Number(external));
		if (external) {
			input.emit("data", Buffer.from("[D"));
			assert.deepEqual(keys, ["a", "return", "left"]);
		}
		if (timersBefore !== null)
			assert.equal(_getPendingTimerCount(), timersBefore);
		input.destroy();
		output.destroy();
	}
	if (originalTerm === undefined) delete process.env.TERM;
	else process.env.TERM = originalTerm;
	console.log("readline-callback-contract-complete");
} finally {
	clearTimeout(deadline);
}
"#,
            )),
        })
        .expect("start JavaScript execution");

    let result = execution.wait().expect("wait for JavaScript execution");
    let stdout = String::from_utf8(result.stdout).expect("stdout utf8");
    let stderr = String::from_utf8(result.stderr).expect("stderr utf8");
    assert_eq!(result.exit_code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(stdout.trim(), "readline-callback-contract-complete");
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");
}
