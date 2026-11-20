import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { fileURLToPath } from "node:url";
import vm from "node:vm";
import { build } from "esbuild";
import { createRequire } from "node:module";
import { withOwnedEsbuild } from "./with-owned-esbuild.mjs";
const require = createRequire(import.meta.url);

// Exercise the actual runner helpers without starting a runner or duplicating its parser.
const source = await readFile(new URL("../../../crates/executor-v8-runtime/assets/runners/wasm-runner.mjs", import.meta.url), "utf8");
const start = "function readVarUint(";
const end = "function decodeBase64ToUint8Array(";
assert.equal(source.split(start).length, 2);
assert.equal(source.split(end).length, 2);
const helpers = source.slice(source.indexOf(start), source.indexOf(end));
const bundle = await withOwnedEsbuild(require.resolve("esbuild"), () => build({
		bundle: true,
		format: "iife",
		write: false,
		stdin: {
			contents: "import { require_buffer } from './bridge-src/vendor/buffer.ts'; globalThis.guestBuffer = require_buffer().Buffer;",
			resolveDir: fileURLToPath(new URL("../", import.meta.url)),
		},
	}));
function load(guest) {
	const context = vm.createContext({ Uint8Array, ArrayBuffer, Buffer });
	if (guest) vm.runInContext(bundle.outputFiles[0].text, context);
	vm.runInContext(`const WASM_PAGE_BYTES = 65536; ${guest ? "Buffer = guestBuffer;" : ""}\n${helpers}\nglobalThis.rewrite = enforceMemoryLimit;`, context);
	return { rewrite: context.rewrite, Buffer: guest ? context.guestBuffer : Buffer };
}
const header = Uint8Array.of(0, 97, 115, 109, 1, 0, 0, 0);
function leb(value) {
	const bytes = [];
	do {
		let byte = value % 128;
		value = Math.floor(value / 128);
		if (value) byte |= 128;
		bytes.push(byte);
	} while (value);
	return Uint8Array.from(bytes);
}
function join(...parts) {
	const result = new Uint8Array(parts.reduce((size, part) => size + part.length, 0));
	let offset = 0;
	for (const part of parts) { result.set(part, offset); offset += part.length; }
	return result;
}
function section(id, payload) { return join(Uint8Array.of(id), leb(payload.length), payload); }
function memory(initial, maximum) {
	return section(5, join(Uint8Array.of(1, maximum === undefined ? 0 : 1), leb(initial), maximum === undefined ? new Uint8Array() : leb(maximum)));
}
function assertBytes(actual, expected) { assert.deepEqual(Uint8Array.from(actual), Uint8Array.from(expected)); }

for (const guest of [false, true]) {
	const lane = load(guest);
	const label = guest ? "guest Buffer" : "native Buffer";
	test(`${label}: preserves untouched payloads, caps memory, and owns its output`, () => {
		const payload = new Uint8Array(256 * 1024);
		for (let index = 0; index < payload.length; index++) payload[index] = (index * 17 + 3) & 255;
		payload[0] = 0; // Empty custom-section name, followed by opaque data.
		const custom = section(0, payload);
		for (const [initial, maximum, cap, expectedMaximum] of [[1, undefined, 2, 2], [1, 7, 3, 3], [1, 2, 7, 2], [0, undefined, 0, 0], [127, 16384, 128, 128]]) {
			const image = join(header, custom, memory(initial, maximum), custom);
			const backing = new Uint8Array(image.length + 13);
			backing.set(image, 7);
			for (const input of [backing.subarray(7, 7 + image.length), lane.Buffer.from(backing.buffer, 7, image.length)]) {
				const result = lane.rewrite(input, cap);
				assert.ok(lane.Buffer.isBuffer(result));
				assertBytes(result, join(header, custom, memory(initial, expectedMaximum), custom));
				assertBytes(input, image);
				input[0] = 255;
				assert.equal(result[0], 0);
				input[0] = 0;
				result[1] = 0;
				assert.equal(input[1], 97);
			}
		}
	});
	test(`${label}: noninteger limits preserve passthrough identity`, () => {
		const image = join(header, memory(1));
		for (const limit of [undefined, null, NaN, Infinity, -Infinity, 1.5, "2"]) assert.strictEqual(lane.rewrite(image, limit), image);
	});
	test(`${label}: malformed inputs retain concrete errors and ordering`, () => {
		for (const [image, expected] of [
			[new Uint8Array(), "module is not a valid WebAssembly binary"],
			[join(header, Uint8Array.of(0, 128)), "WebAssembly section size truncated"],
			[join(header, Uint8Array.of(0), new Uint8Array(10).fill(128)), "WebAssembly section size exceeds varuint limit"],
			[join(header, Uint8Array.of(0, 2, 0)), "section extends past end of module"],
			[join(header, section(5, Uint8Array.of(1))), "WebAssembly memory flags truncated"],
			[join(header, section(5, Uint8Array.of(1, 0))), "WebAssembly memory minimum truncated"],
			[join(header, section(5, Uint8Array.of(1, 1, 1))), "WebAssembly memory maximum truncated"],
			[join(header, section(5, Uint8Array.of(1, 2, 0)), Uint8Array.of(0, 128)), "configured WebAssembly memory limit does not support memory flags 2"],
			[join(header, memory(3), Uint8Array.of(0, 128)), "initial WebAssembly memory of 196608 bytes exceeds the configured limit of 131072 bytes"],
			[join(header, section(5, Uint8Array.of(1, 0, 1, 0))), "memory section parsing did not consume the full section"],
		]) {
			assert.throws(() => lane.rewrite(image, 2), error => error.name === "Error" && error.message === expected);
		}
	});
	test(`${label}: real WebAssembly memory growth honors cap and existing maximum`, () => {
		const type = section(1, Uint8Array.of(1, 96, 1, 127, 1, 127));
		const functions = section(3, Uint8Array.of(1, 0));
		const exports = section(7, Uint8Array.of(2, 6, 109, 101, 109, 111, 114, 121, 2, 0, 4, 103, 114, 111, 119, 0, 0));
		const code = section(10, Uint8Array.of(1, 6, 0, 32, 0, 64, 0, 11));
		const instantiate = (initial, maximum, cap) => new WebAssembly.Instance(new WebAssembly.Module(lane.rewrite(join(header, type, functions, memory(initial, maximum), exports, code), cap))).exports;
		const instance = instantiate(1, undefined, 2);
		new Uint8Array(instance.memory.buffer)[31] = 73;
		assert.equal(instance.grow(1), 1);
		assert.equal(instance.memory.buffer.byteLength, 131072);
		assert.equal(new Uint8Array(instance.memory.buffer)[31], 73);
		assert.equal(instance.grow(1), -1);
		assert.equal(instance.grow(0), 2);
		assert.throws(() => instance.memory.grow(1), RangeError);
		assert.equal(instantiate(1, 1, 4).grow(1), -1);
		assert.throws(() => instantiate(3, undefined, 2), /initial WebAssembly memory/);
	});
	test(`${label}: many small sections preserve bytes and parser permissiveness`, () => {
		const custom = new Uint8Array(10_000 * 3);
		const repeatedMemory = new Uint8Array(10_000 * 3);
		for (let index = 0; index < 10_000; index++) {
			custom.set([0, 1, 0], index * 3);
			repeatedMemory.set([5, 1, 0], index * 3);
		}
		for (const withMemory of [false, true]) {
			const image = join(header, custom, withMemory ? memory(1) : new Uint8Array(), custom);
			const result = lane.rewrite(image, 2);
			assertBytes(result, join(header, custom, withMemory ? memory(1, 2) : new Uint8Array(), custom));
			new WebAssembly.Module(result);
		}
		const repeated = join(header, repeatedMemory);
		assertBytes(lane.rewrite(repeated, 2), repeated);
		assert.equal(WebAssembly.validate(lane.rewrite(repeated, 2)), false);
	});
}
