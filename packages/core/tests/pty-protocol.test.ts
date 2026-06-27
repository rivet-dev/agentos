import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { Terminal } from "@xterm/headless";
import { afterEach, describe, expect, test } from "vitest";
import type { AgentOs } from "../src/index.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, "../../..");
const SECURE_EXEC_C_ROOT = resolve(
	REPO_ROOT,
	"../secure-exec/registry/native/c",
);
const SIDECAR_BINARY = resolve(REPO_ROOT, "target/debug/agentos-sidecar");
const PTY_PROBE_COMMAND_DIR = resolve(SECURE_EXEC_C_ROOT, "build");
const PTY_PROBE_BINARY = resolve(PTY_PROBE_COMMAND_DIR, "pty_probe");

const SETTLE_MS = 80;
const WAIT_TIMEOUT_MS = 15_000;

function ensurePtyProbeBuilt(): void {
	if (existsSync(PTY_PROBE_BINARY)) {
		return;
	}

	const result = spawnSync("make", ["build/pty_probe"], {
		cwd: SECURE_EXEC_C_ROOT,
		encoding: "utf8",
	});
	if (result.status !== 0) {
		throw new Error(
			[
				"failed to build pty_probe C WASM fixture",
				`cwd=${SECURE_EXEC_C_ROOT}`,
				`status=${result.status}`,
				result.stdout,
				result.stderr,
			]
				.filter(Boolean)
				.join("\n"),
		);
	}
}

function ensureWorkspaceSidecarBuilt(): void {
	if (!existsSync(SIDECAR_BINARY)) {
		const result = spawnSync("cargo", ["build", "-q", "-p", "agentos-sidecar"], {
			cwd: REPO_ROOT,
			encoding: "utf8",
		});
		if (result.status !== 0) {
			throw new Error(
				[
					"failed to build workspace agentos-sidecar",
					`cwd=${REPO_ROOT}`,
					`status=${result.status}`,
					result.stdout,
					result.stderr,
				]
					.filter(Boolean)
					.join("\n"),
			);
		}
	}
	process.env.AGENTOS_SIDECAR_BIN = SIDECAR_BINARY;
}

function terminalSnapshot(label: string, term: Terminal): string {
	const buffer = term.buffer.active;
	const lines: string[] = [];
	for (let row = 0; row < term.rows; row++) {
		const line = buffer.getLine(buffer.viewportY + row);
		lines.push(
			`${String(row + 1).padStart(2, "0")}|${
				line ? line.translateToString(true) : ""
			}`,
		);
	}

	return [
		`# ${label}`,
		`cols=${term.cols} rows=${term.rows} cursor=${buffer.cursorX},${buffer.cursorY}`,
		...lines,
	].join("\n");
}

async function settle(): Promise<void> {
	await new Promise((resolve) => setTimeout(resolve, SETTLE_MS));
}

async function waitForScreen(
	term: Terminal,
	text: string,
	timeoutMs = WAIT_TIMEOUT_MS,
): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		if (terminalSnapshot("wait", term).includes(text)) {
			await settle();
			return;
		}
		await new Promise((resolve) => setTimeout(resolve, 25));
	}
	throw new Error(
		`timed out waiting for ${JSON.stringify(text)}\n${terminalSnapshot(
			"timeout",
			term,
		)}`,
	);
}

describe("PTY protocol snapshots", () => {
	let vm: AgentOs | undefined;
	let term: Terminal | undefined;
	let shellId: string | undefined;
	let unsubscribeShellData: (() => void) | undefined;
	let disposeTerminalData: { dispose(): void } | undefined;

	afterEach(async () => {
		if (unsubscribeShellData) {
			unsubscribeShellData();
			unsubscribeShellData = undefined;
		}
		if (disposeTerminalData) {
			disposeTerminalData.dispose();
			disposeTerminalData = undefined;
		}
		if (vm && shellId) {
			try {
				vm.closeShell(shellId);
			} catch {
				// The probe may already have exited.
			}
		}
		term?.dispose();
		term = undefined;
		shellId = undefined;
		if (vm) {
			await vm.dispose();
			vm = undefined;
		}
	});

	test("C WASM probe snapshots raw, cooked, CPR, resize, and EOF terminal protocol", async () => {
		ensureWorkspaceSidecarBuilt();
		ensurePtyProbeBuilt();
		const { AgentOs } = await import("../src/index.js");

		term = new Terminal({ cols: 80, rows: 18, allowProposedApi: true });
		vm = await AgentOs.create({
			software: [
				{
					name: "pty-probe-fixture",
					type: "wasm-commands",
					commandDir: PTY_PROBE_COMMAND_DIR,
					commands: [{ name: "pty_probe" }],
				},
			],
		});

		({ shellId } = vm.openShell({
			command: "pty_probe",
			cols: term.cols,
			rows: term.rows,
			env: {
				TERM: "xterm-256color",
				COLUMNS: String(term.cols),
				LINES: String(term.rows),
			},
		}));

		unsubscribeShellData = vm.onShellData(shellId, (data) => {
			term?.write(data);
		});
		disposeTerminalData = term.onData((data) => {
			if (vm && shellId) {
				vm.writeShell(shellId, data);
			}
		});

		await waitForScreen(term, "RAW_INPUT>");
		expect(terminalSnapshot("startup through CPR", term)).toMatchSnapshot();

		vm.writeShell(shellId, "A\r\x1b[A\x17!");
		await waitForScreen(term, "COOKED_INPUT>");
		expect(terminalSnapshot("after raw input bytes", term)).toMatchSnapshot();

		vm.writeShell(shellId, "hello cooked\r");
		await waitForScreen(term, "RESIZE_READY>");
		expect(terminalSnapshot("after cooked enter", term)).toMatchSnapshot();

		term.resize(100, 20);
		vm.resizeShell(shellId, 100, 20);
		vm.writeShell(shellId, "resize-now\r");
		await waitForScreen(term, "EOF_READY>");
		expect(terminalSnapshot("after resize trigger", term)).toMatchSnapshot();

		vm.writeShell(shellId, "\x04");
		await waitForScreen(term, "PTY_PROBE done");
		expect(terminalSnapshot("after eof", term)).toMatchSnapshot();

		await expect(vm.waitShell(shellId)).resolves.toBe(0);
	}, 60_000);
});
