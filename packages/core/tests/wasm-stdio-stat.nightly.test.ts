import { type SpawnSyncReturns, spawnSync } from "node:child_process";
import {
	closeSync,
	existsSync,
	mkdtempSync,
	openSync,
	readFileSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { AgentOs, createSnapshotExport } from "../src/index.js";

const root = fileURLToPath(new URL("../../../", import.meta.url));
const compiler = resolve(root, "toolchain/c/scripts/wasi-clang.sh");
const sysroot = resolve(root, "toolchain/c/sysroot");
const fixture = fileURLToPath(
	new URL("./fixtures/stdio_stat_probe.c", import.meta.url),
);
const shell = resolve(root, "software/coreutils/bin/sh");
const hasToolchain =
	existsSync(resolve(root, "toolchain/c/vendor/wasi-sdk/bin/clang")) &&
	existsSync(join(sysroot, "lib/wasm32-wasi/libc.a")) &&
	existsSync(shell);
if (process.env.AGENTOS_REQUIRE_STDIO_STAT === "1" && !hasToolchain)
	throw new Error(
		"stdio stat acceptance requires owned WASM compiler/sysroot and shell",
	);

describe.skipIf(!hasToolchain)("WASM managed stdin metadata", () => {
	let directory: string;
	let native: string;
	let image: Buffer;
	beforeAll(() => {
		directory = mkdtempSync(join(tmpdir(), "agentos-stdio-stat-"));
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
			const built = spawnSync(command, args, {
				encoding: "utf8",
				timeout: 30_000,
			});
			expect(built.status, built.error?.message ?? built.stderr).toBe(0);
		}
		image = readFileSync(wasm);
	});
	afterAll(() => {
		if (directory) rmSync(directory, { recursive: true, force: true });
	});

	for (const mode of ["direct", "alias", "flags"] as const) {
		const alias = mode !== "direct";
		for (const kind of [
			"finite pipe",
			"empty EOF",
			"redirected file",
			"shell pipeline",
		] as const) {
			test(`matches Linux ${mode === "flags" ? "F_SETFL alias" : alias ? "dup/close alias" : "fstat/read/invalid-fd"} behavior for ${kind}`, async () => {
				const input = kind === "empty EOF" ? "" : "one\ntwo";
				const inputPath = join(directory, "input.txt");
				writeFileSync(inputPath, input);
				let reference: SpawnSyncReturns<string>;
				if (kind === "redirected file") {
					const fd = openSync(inputPath, "r");
					try {
						reference = spawnSync(native, alias ? [mode] : [], {
							stdio: [fd, "pipe", "pipe"],
							encoding: "utf8",
							timeout: 5_000,
							maxBuffer: 4096,
						});
					} finally {
						closeSync(fd);
					}
				} else {
					reference = spawnSync(
						"sh",
						[
							"-c",
							`printf "%s" "$1" | "$2"${alias ? ` ${mode}` : ""}`,
							"stdio-stat",
							input,
							native,
						],
						{ encoding: "utf8", timeout: 5_000, maxBuffer: 4096 },
					);
				}
				expect(reference.status, reference.stderr).toBe(0);
				const expected = JSON.parse(reference.stdout);
				expect(expected).toEqual({
					fifo: kind === "redirected file" ? 0 : 1,
					regular: kind === "redirected file" ? 1 : 0,
					size: kind === "redirected file" ? input.length : 0,
					bytes: input.length,
					invalid: 1,
					...(mode === "flags"
						? {
								originalNonblock: 1,
								aliasNonblock: 1,
								clearShared: 1,
								retainedNonblock: 1,
								clearRetained: 1,
							}
						: {}),
					...(alias
						? {
								sameIdentity: 1,
								sameMetadata: 1,
								sameFlags: 1,
								closedBadf: 1,
								pipeOwner: kind === "redirected file" ? -1 : 1,
							}
						: {}),
				});
				const vm = await AgentOs.create({
					defaultSoftware: false,
					permissions: { fs: "allow", childProcess: "allow", process: "allow" },
					rootFilesystem: {
						lowers: [
							createSnapshotExport([
								{ path: "/bin", type: "directory", mode: "0755" },
								{
									path: "/bin/probe",
									type: "file",
									mode: "0755",
									content: image.toString("base64"),
									encoding: "base64",
								},
								{
									path: "/bin/sh",
									type: "file",
									mode: "0755",
									content: readFileSync(shell).toString("base64"),
									encoding: "base64",
								},
								{
									path: "/input.txt",
									type: "file",
									mode: "0644",
									content: input,
									encoding: "utf8",
								},
							]),
						],
					},
				});
				try {
					const shellScript =
						kind === "redirected file"
							? `exec /bin/probe${alias ? ` ${mode}` : ""} < /input.txt`
							: `printf "%s" "$1" | /bin/probe${alias ? ` ${mode}` : ""}`;
					const result = await vm.process.execFile(
						kind === "redirected file" || kind === "shell pipeline"
							? "/bin/sh"
							: "/bin/probe",
						kind === "redirected file" || kind === "shell pipeline"
							? ["-c", shellScript, "stdio-stat", input]
							: alias
								? [mode]
								: [],
						{
							...(kind === "finite pipe" || kind === "empty EOF"
								? { stdin: input }
								: {}),
							timeoutMs: 5_000,
							output: { capture: "all", maxBufferedBytes: 4096 },
						},
					);
					expect(result.exitCode, result.stderr).toBe(0);
					expect(result.error).toBeUndefined();
					expect(result.stderr).toBe("");
					expect(JSON.parse(result.stdout)).toEqual(expected);
				} finally {
					await vm.dispose();
				}
			}, 15_000);
		}
	}
});
