import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";
import { createInMemoryFileSystem } from "../src/test/runtime.js";

const root = fileURLToPath(new URL("../../../", import.meta.url));
const compiler = resolve(root, "toolchain/c/scripts/wasi-clang.sh");
const sysroot = resolve(root, "toolchain/c/sysroot");
const fixture = fileURLToPath(
	new URL("./fixtures/tmpfile_probe.c", import.meta.url),
);
const hasToolchain =
	existsSync(resolve(root, "toolchain/c/vendor/wasi-sdk/bin/clang")) &&
	existsSync(join(sysroot, "lib/wasm32-wasi/libc.a"));

describe.skipIf(!hasToolchain)("WASM unnamed temporary files", () => {
	let directory: string;
	let image: Uint8Array;
	let native: string;

	beforeAll(() => {
		directory = mkdtempSync(join(tmpdir(), "agentos-tmpfile-probe-"));
		const wasm = join(directory, "probe.wasm");
		native = join(directory, "probe-native");
		for (const [command, args] of [
			[
				compiler,
				[
					"--target=wasm32-wasip1",
					`--sysroot=${sysroot}`,
					"-O2",
					fixture,
					"-o",
					wasm,
				],
			],
			["cc", ["-O2", fixture, "-o", native]],
		] as const) {
			const result = spawnSync(command, args, {
				encoding: "utf8",
				timeout: 30_000,
			});
			expect(result.status, result.error?.message ?? result.stderr).toBe(0);
		}
		image = readFileSync(wasm);
	});

	afterAll(() => {
		if (directory) rmSync(directory, { recursive: true, force: true });
	});

	async function runProbe(scenario: string, customMount = false) {
		const mounted = createInMemoryFileSystem();
		await mounted.chmod("/", 0o777);
		const vm = await AgentOs.create({
			defaultSoftware: false,
			...(customMount
				? {
						mounts: [{ path: "/custom", driver: mounted }],
					}
				: {}),
		});
		try {
			await vm.filesystem.writeFile("/tmp/tmpfile-probe", image);
			const chmod = await vm.process.execFile(
				"node",
				["-e", "require('node:fs').chmodSync('/tmp/tmpfile-probe', 0o755)"],
				{ timeoutMs: 5_000, output: { capture: "all" } },
			);
			expect(chmod.exitCode, chmod.stderr).toBe(0);
			const result = await vm.process.execFile(
				"/tmp/tmpfile-probe",
				[customMount ? "/custom" : "/tmp/tmpfile-case", scenario],
				{ timeoutMs: 5_000, output: { capture: "all" } },
			);
			expect(result.exitCode, result.stderr).toBe(0);
			expect(result.error).toBeUndefined();
			expect(result.stderr).toBe("");
			return result.stdout;
		} finally {
			await vm.dispose();
		}
	}

	test.each([
		"namespace",
		"lifetime",
	])("matches native Linux %s behavior", async (scenario) => {
		const reference = spawnSync(
			native,
			[join(directory, `native-${scenario}`), scenario],
			{
				encoding: "utf8",
				timeout: 5_000,
			},
		);
		expect(reference.status, reference.stderr).toBe(0);
		expect(await runProbe(scenario)).toBe(reference.stdout);
	}, 20_000);

	test("returns EOPNOTSUPP on a custom mount while ordinary files work", async () => {
		expect(await runProbe("unsupported", true)).toBe(
			"EOPNOTSUPP; ordinary file remains supported\n",
		);
	}, 20_000);
});
