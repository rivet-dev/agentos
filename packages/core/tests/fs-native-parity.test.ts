import { spawnSync } from "node:child_process";
import {
	copyFileSync,
	existsSync,
	mkdirSync,
	mkdtempSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, test } from "vitest";
import type { AgentOs } from "../src/index.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, "../../..");
const SECURE_EXEC_C_ROOT = resolve(
	REPO_ROOT,
	"../secure-exec/registry/native/c",
);
const WASM_PROBE_BINARY = resolve(SECURE_EXEC_C_ROOT, "build/fs_probe");
const NATIVE_PROBE_BINARY = resolve(
	SECURE_EXEC_C_ROOT,
	"build/native/fs_probe",
);
const PATCHED_LIBC = resolve(
	SECURE_EXEC_C_ROOT,
	"sysroot/lib/wasm32-wasi/libc.a",
);
const SIDECAR_BINARY = resolve(REPO_ROOT, "target/debug/agentos-sidecar");

function hasCommand(command: string): boolean {
	try {
		return (
			spawnSync(command, ["--version"], { encoding: "utf8" }).status === 0
		);
	} catch {
		return false;
	}
}

// This test builds the fs_probe fixtures (native + wasm) and the workspace
// sidecar on the fly, so it can only run with the secure-exec sibling checkout
// present (absent on release pins, where prepare-build performs no clone) plus
// make + cargo. The wasm C toolchain is the WASI SDK clang invoked by the
// sibling Makefile (not a system `clang` on PATH), so it is not probed here.
// Skip cleanly otherwise instead of hard-failing the run, matching
// vim-native-parity.test.ts.
const CAN_RUN =
	existsSync(join(SECURE_EXEC_C_ROOT, "Makefile")) &&
	hasCommand("make") &&
	hasCommand("cargo");

function runChecked(
	command: string,
	args: string[],
	options: { cwd: string; label: string },
): void {
	const result = spawnSync(command, args, {
		cwd: options.cwd,
		encoding: "utf8",
		env: { ...process.env, AGENTOS_WASM_SNAPSHOT_RUNNER: "off" },
	});
	if (result.status !== 0) {
		throw new Error(
			[
				`${options.label} failed`,
				`cwd=${options.cwd}`,
				`command=${command} ${args.join(" ")}`,
				`status=${result.status}`,
				result.stdout,
				result.stderr,
			]
				.filter(Boolean)
				.join("\n"),
		);
	}
}

function ensureFsProbeBuilt(): void {
	if (!existsSync(PATCHED_LIBC)) {
		runChecked("make", ["sysroot"], {
			cwd: SECURE_EXEC_C_ROOT,
			label: "failed to build patched wasi-libc sysroot",
		});
	}
	runChecked("make", ["build/native/fs_probe", "build/fs_probe"], {
		cwd: SECURE_EXEC_C_ROOT,
		label: "failed to build fs_probe parity fixtures",
	});
}

function ensureWorkspaceSidecarBuilt(): void {
	runChecked("node", ["scripts/secure-exec-dep.mjs", "prepare-build"], {
		cwd: REPO_ROOT,
		label: "failed to prepare secure-exec workspace dependency",
	});
	runChecked("cargo", ["build", "-q", "-p", "agentos-sidecar"], {
		cwd: REPO_ROOT,
		label: "failed to build workspace agentos-sidecar",
	});
	process.env.AGENTOS_SIDECAR_BIN = SIDECAR_BINARY;
	process.env.AGENTOS_WASM_SNAPSHOT_RUNNER = "off";
}

function materializeFsProbePackage(): string {
	const pkgDir = mkdtempSync(join(tmpdir(), "agentos-fs-probe-pkg-"));
	mkdirSync(join(pkgDir, "bin"));
	copyFileSync(WASM_PROBE_BINARY, join(pkgDir, "bin", "fs_probe"));
	writeFileSync(
		join(pkgDir, "package.json"),
		JSON.stringify({ name: "fs-probe-fixture", version: "0.0.0" }),
	);
	writeFileSync(
		join(pkgDir, "agentos-package.json"),
		JSON.stringify({ name: "fs-probe-fixture" }),
	);
	return pkgDir;
}

function normalizeProbeOutput(output: string): string {
	return output
		.replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "")
		.replace(/\r\n/g, "\n")
		.replace(/\r/g, "\n")
		.trimEnd();
}

function runNativeProbe(): string {
	const scratchDir = mkdtempSync(join(tmpdir(), "fs-probe-native-"));
	try {
		const result = spawnSync(NATIVE_PROBE_BINARY, [scratchDir], {
			encoding: "utf8",
		});
		expect(result.status, result.stderr || result.stdout).toBe(0);
		return normalizeProbeOutput(result.stdout);
	} finally {
		rmSync(scratchDir, { force: true, recursive: true });
	}
}

describe.skipIf(!CAN_RUN)("filesystem native parity", () => {
	let vm: AgentOs | undefined;
	let shellId: string | undefined;
	let unsubscribeShellData: (() => void) | undefined;
	let probePkgDir: string | undefined;

	afterEach(async () => {
		if (unsubscribeShellData) {
			unsubscribeShellData();
			unsubscribeShellData = undefined;
		}
		if (probePkgDir) {
			rmSync(probePkgDir, { force: true, recursive: true });
			probePkgDir = undefined;
		}
		if (vm && shellId) {
			try {
				vm.closeShell(shellId);
			} catch {
				// The probe may already have exited.
			}
		}
		shellId = undefined;
		if (vm) {
			await vm.dispose();
			vm = undefined;
		}
	});

	test("fs_probe output matches native Linux through openShell", async () => {
		ensureWorkspaceSidecarBuilt();
		ensureFsProbeBuilt();
		const expected = runNativeProbe();
		const { AgentOs } = await import("../src/index.js");

		probePkgDir = materializeFsProbePackage();
		vm = await AgentOs.create({
			defaultSoftware: false,
			software: [probePkgDir],
		});

		let rawOutput = "";
		({ shellId } = vm.openShell({
			command: "fs_probe",
			args: ["/tmp/fs-probe-vm"],
			cols: 120,
			rows: 40,
		}));
		unsubscribeShellData = vm.onShellData(shellId, (data) => {
			rawOutput +=
				typeof data === "string" ? data : Buffer.from(data).toString("utf8");
		});

		const status = await vm.waitShell(shellId);
		const actual = normalizeProbeOutput(rawOutput);
		expect(status, actual).toBe(0);
		expect(actual).toBe(expected);
		// Generous timeout: the first run may build the wasi-libc sysroot and the
		// debug sidecar from cold, which can exceed a 2-minute budget. Warm runs
		// are near-instant (make/cargo no-ops).
	}, 600_000);
});
