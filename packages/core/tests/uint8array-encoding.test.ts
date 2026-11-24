import { readFileSync } from "node:fs";
import { afterEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const probe = readFileSync(
	new URL("./fixtures/uint8array-encoding-probe.mjs", import.meta.url),
);

async function runGuest(
	vm: AgentOs,
	path: string,
	source: string | Uint8Array,
) {
	await vm.filesystem.writeFile(path, source);
	let stdout = "";
	let stderr = "";
	const { pid } = await vm.process.spawn("node", [path], {
		onStdout: (chunk) => {
			stdout += new TextDecoder().decode(chunk);
		},
		onStderr: (chunk) => {
			stderr += new TextDecoder().decode(chunk);
		},
	});
	const { exitCode } = await vm.process.wait(pid);
	return { exitCode, stdout, stderr };
}

describe("Uint8Array hex and base64 methods in the VM", () => {
	const cleanups: Array<() => Promise<void>> = [];
	afterEach(async () => {
		for (const cleanup of cleanups.splice(0).reverse()) await cleanup();
	});

	test("guest code gets the proposal's results, errors, and partial writes", async () => {
		const vm = await AgentOs.create({ defaultSoftware: false });
		cleanups.push(() => vm.dispose());

		const { exitCode, stdout, stderr } = await runGuest(
			vm,
			"/tmp/probe.mjs",
			probe,
		);

		expect({ exitCode, stderr }).toEqual({ exitCode: 0, stderr: "" });
		expect(JSON.parse(stdout).failures).toEqual([]);
	});

	// Without a flat result, this ordinary code aborts the sidecar and every VM on it.
	test.each([
		["toHex", "new Uint8Array(32 * 1024 * 1024).fill(171).toHex()", "b"],
		["toBase64", "new Uint8Array(48 * 1024 * 1024).toBase64()", "A"],
	])("a large %s result stays usable after garbage collection", async (_, call, last) => {
		const vm = await AgentOs.create({ defaultSoftware: false });
		cleanups.push(() => vm.dispose());

		const result = await runGuest(
			vm,
			"/tmp/encode.mjs",
			`const encoded = ${call};
			const garbage = [];
			for (let index = 0; index < 3e6; index++) garbage.push(index);
			console.log(garbage.length, encoded.at(-1));`,
		);

		expect(result).toEqual({
			exitCode: 0,
			stdout: `3000000 ${last}\n`,
			stderr: "",
		});
	});

	test.each([
		["fromHex", "Uint8Array.fromHex('A'.repeat(150e6))"],
		["fromBase64", "Uint8Array.fromBase64('A'.repeat(150e6))"],
	])("decoding past the heap limit with %s stops only that guest", async (_, call) => {
		const sidecar = await AgentOs.createSidecar();
		cleanups.push(() => sidecar.dispose());
		const options = {
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
		} as const;
		const hostile = await AgentOs.create(options);
		cleanups.push(() => hostile.dispose());
		const neighbor = await AgentOs.create(options);
		cleanups.push(() => neighbor.dispose());

		const { exitCode, stderr } = await runGuest(
			hostile,
			"/tmp/decode.mjs",
			`${call};`,
		);

		expect(exitCode).toBe(1);
		expect(stderr).toContain("limits.jsRuntime.v8HeapLimitMb");
		const after = await runGuest(
			neighbor,
			"/tmp/neighbor.mjs",
			"console.log(new Uint8Array([1, 255]).toHex());",
		);
		expect(after).toEqual({ exitCode: 0, stdout: "01ff\n", stderr: "" });
	});
});
