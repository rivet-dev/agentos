import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

// Regression coverage for the guest fs bridge (bridge-src/builtins/fs.ts).
// appendFile* used to read the existing file as a UTF-8 string, stringify the
// appended data and rewrite the whole file. Every non-UTF-8 byte already on disk
// was rewritten as U+FFFD, a plain Uint8Array was appended as its comma-joined
// decimal text, and concurrent appends overwrote each other. Linux appends the
// raw bytes at end-of-file. vm.readFile reads through the sidecar rather than the
// guest bridge, so it is an independent oracle for what actually landed on disk.
describe("guest fs.appendFile preserves raw bytes", () => {
	let vm: AgentOs;

	beforeAll(async () => {
		vm = await AgentOs.create();
	}, 30_000);

	afterAll(async () => {
		await vm.dispose();
	});

	test("appendFileSync keeps existing non-UTF-8 bytes", async () => {
		await vm.writeFile("/tmp/append-sync.bin", new Uint8Array([0x00, 0xff]));
		await vm.javascript.execute(`
			import { appendFileSync } from "node:fs";
			import { Buffer } from "node:buffer";
			appendFileSync("/tmp/append-sync.bin", Buffer.from([0xfe]));
		`);
		expect([...(await vm.readFile("/tmp/append-sync.bin"))]).toEqual([
			0x00, 0xff, 0xfe,
		]);
	});

	test("appendFileSync appends a Uint8Array as bytes, not text", async () => {
		await vm.writeFile("/tmp/append-u8.bin", new Uint8Array([]));
		await vm.javascript.execute(`
			import { appendFileSync } from "node:fs";
			appendFileSync("/tmp/append-u8.bin", new Uint8Array([1, 2, 3]));
		`);
		expect([...(await vm.readFile("/tmp/append-u8.bin"))]).toEqual([1, 2, 3]);
	});

	test("fs.promises.appendFile keeps existing non-UTF-8 bytes", async () => {
		await vm.writeFile("/tmp/append-async.bin", new Uint8Array([0x00, 0xff]));
		await vm.javascript.execute(`
			import { promises } from "node:fs";
			import { Buffer } from "node:buffer";
			await promises.appendFile("/tmp/append-async.bin", Buffer.from([0xfe]));
		`);
		expect([...(await vm.readFile("/tmp/append-async.bin"))]).toEqual([
			0x00, 0xff, 0xfe,
		]);
	});

	test("appending a UTF-8 string still concatenates text", async () => {
		await vm.writeFile("/tmp/append-text.txt", "hello");
		await vm.javascript.execute(`
			import { appendFileSync } from "node:fs";
			appendFileSync("/tmp/append-text.txt", " world");
		`);
		expect(
			new TextDecoder().decode(await vm.readFile("/tmp/append-text.txt")),
		).toBe("hello world");
	});

	test("concurrent fs.promises.appendFile calls all land", async () => {
		await vm.writeFile("/tmp/append-concurrent.bin", new Uint8Array([]));
		await vm.javascript.execute(`
			import { promises } from "node:fs";
			import { Buffer } from "node:buffer";
			await Promise.all(
				Array.from({ length: 10 }, (_, i) =>
					promises.appendFile("/tmp/append-concurrent.bin", Buffer.from([0x41 + i])),
				),
			);
		`);
		const landed = [...(await vm.readFile("/tmp/append-concurrent.bin"))].sort(
			(a, b) => a - b,
		);
		expect(landed).toEqual(Array.from({ length: 10 }, (_, i) => 0x41 + i));
	});

	test("fs.promises.appendFile rejects an unknown encoding with ERR_INVALID_ARG_VALUE", async () => {
		await vm.javascript.execute(`
			import { promises, writeFileSync } from "node:fs";
			let code = "no-error";
			try {
				await promises.appendFile("/tmp/append-encoding.txt", "x", "not-an-encoding");
			} catch (err) {
				code = String(err?.code);
			}
			writeFileSync("/tmp/append-encoding.code", code);
		`);
		expect(
			new TextDecoder().decode(await vm.readFile("/tmp/append-encoding.code")),
		).toBe("ERR_INVALID_ARG_VALUE");
	});
});
