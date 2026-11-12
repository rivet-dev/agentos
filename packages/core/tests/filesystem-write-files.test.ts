import type { ChildProcess } from "node:child_process";
import { afterEach, beforeEach, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

let vm: AgentOs;

beforeEach(async () => {
	vm = await AgentOs.create({ defaultSoftware: false });
});

afterEach(async () => {
	await vm.dispose();
});

test("a failed entry reports its own error and every other entry is written", async () => {
	const bytes = new Uint8Array([0, 1, 2, 255]);

	const results = await vm.filesystem.writeFiles([
		{ path: "/tmp/wf/file.txt", content: "first" },
		// The parent is the regular file written by the entry above.
		{ path: "/tmp/wf/file.txt/child.txt", content: "nope" },
		{ path: "/tmp/wf/deep/nested/data.bin", content: bytes },
		{ path: "/proc/not-writable", content: "nope" },
		{ path: "/tmp/wf/last.txt", content: "last" },
	]);

	expect(results.map((result) => [result.path, result.success])).toEqual([
		["/tmp/wf/file.txt", true],
		["/tmp/wf/file.txt/child.txt", false],
		["/tmp/wf/deep/nested/data.bin", true],
		["/proc/not-writable", false],
		["/tmp/wf/last.txt", true],
	]);
	expect(results[1].error).toMatch(/ENOTDIR|EEXIST/);
	expect(results[3].error).toBe("Path is read-only: /proc/not-writable");

	const decode = (value: Uint8Array) => new TextDecoder().decode(value);
	expect(decode(await vm.filesystem.readFile("/tmp/wf/file.txt"))).toBe(
		"first",
	);
	expect(
		new Uint8Array(await vm.filesystem.readFile("/tmp/wf/deep/nested/data.bin")),
	).toEqual(bytes);
	expect(decode(await vm.filesystem.readFile("/tmp/wf/last.txt"))).toBe(
		"last",
	);
}, 60_000);

test("a batch larger than one protocol frame writes every file", async () => {
	// Five 4 MiB files: together they exceed one 16 MiB frame.
	const files = [0, 1, 2, 3, 4].map((seed) => ({
		path: `/tmp/big/${seed}.bin`,
		content: new Uint8Array(4 * 1024 * 1024).map((_, index) => (index + seed) % 251),
	}));

	const results = await vm.filesystem.writeFiles(files);

	expect(results).toEqual(
		files.map((file) => ({ path: file.path, success: true })),
	);
	for (const file of files) {
		const read = Buffer.from(await vm.filesystem.readFile(file.path));
		// Buffer.equals: a deep toEqual over 4 MiB is very slow.
		expect(read.equals(file.content)).toBe(true);
	}
}, 120_000);

test("when the sidecar exits, a batch of 130,000 entries returns one failed result per entry", async () => {
	// A dedicated sidecar keeps the killed process out of the shared pool.
	const sidecar = await AgentOs.createSidecar();
	const isolated = await AgentOs.create({
		defaultSoftware: false,
		sidecar: { kind: "explicit", handle: sidecar },
	});
	try {
		// Reaches into internals only to find the process to kill.
		const child = (
			isolated as unknown as {
				_sidecarClient: { protocolClient: { child: ChildProcess } };
			}
		)._sidecarClient.protocolClient.child;
		const exited = new Promise((resolve) => child.once("exit", resolve));
		child.kill("SIGKILL");
		await exited;

		// More results than `push(...results)` can spread onto the call stack.
		const files = Array.from({ length: 130_000 }, (_, index) => ({
			path: `/tmp/many/${index}`,
			content: "x",
		}));
		const results = await isolated.filesystem.writeFiles(files);

		expect(results.length).toBe(files.length);
		expect(results.every((result) => !result.success && result.error)).toBe(
			true,
		);
		expect(results[129_999].path).toBe("/tmp/many/129999");
	} finally {
		await isolated.dispose().catch(() => {});
		await sidecar.dispose().catch(() => {});
	}
}, 60_000);
