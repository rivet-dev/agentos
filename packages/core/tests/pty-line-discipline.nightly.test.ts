// pty-line-discipline.nightly.test.ts — terminal (PTY) line-discipline matrix.
//
// ONE shared CASES table is exercised IDENTICALLY through TWO guest runtimes:
//   - wasm-c : a C probe (tests/fixtures/pty/pty_probe.c) compiled to WASM and
//              registered as a VM command.
//   - js-node: a guest-Node probe (tests/fixtures/pty/pty_probe.mjs) launched via
//              the built-in `node` command.
// Both probes implement the same argv-dispatched case set and emit the same
// `#`-prefixed marker protocol, so the host asserts the SAME strings for both.
// The JS-node runtime is part of the default suite. The WASI-C runtime is gated
// behind AGENTOS_CORE_PTY_C=1 because it requires local C/WASI fixtures and is
// used as explicit native TTY validation.
//
// Every enabled case asserts the supported behavior. JavaScript snapshots
// supplement byte/event assertions; the opt-in C runtime shares those assertions.
//
// Uncaught-signal cells await the shell exit and assert its signal status plus
// absence of #DONE. Caught-signal cells assert the handler and normal completion.
//
// Run:
//   cd <repo root>
//   PATH="/home/nathan/.nvm/versions/node/v24.13.0/bin:/tmp/pnpm:/usr/bin:/bin" \
//     pnpm --dir packages/core exec vitest run tests/pty-line-discipline.nightly.test.ts

import { spawnSync } from "node:child_process";
import {
	existsSync,
	mkdirSync,
	mkdtempSync,
	readFileSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { Terminal } from "@xterm/headless";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";
import type { AgentOs } from "../src/index.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, "../../..");
const FIXTURE_DIR = resolve(__dirname, "fixtures/pty");
const C_PROBE_SOURCE = join(FIXTURE_DIR, "pty_probe.c");
const NODE_PROBE_SOURCE = join(FIXTURE_DIR, "pty_probe.mjs");
const NODE_PROBE_GUEST_PATH = "/pty_probe.mjs";

const WASI_SDK = resolve(REPO_ROOT, "toolchain/c/vendor/wasi-sdk");
const SIDECAR_BINARY = resolve(
	REPO_ROOT,
	process.env.CARGO_TARGET_DIR ?? "target",
	"debug/agentos-sidecar",
);

const SETTLE_MS = 80;
const WAIT_TIMEOUT_MS = 15_000;
const TEST_TIMEOUT_MS = 60_000;
const ENABLE_WASM_C_PTY = process.env.AGENTOS_CORE_PTY_C === "1";

// ---------------------------------------------------------------------------
// build / sidecar prerequisites
// ---------------------------------------------------------------------------

// Compile the C probe to WASM directly into a package `bin/` dir. The basename
// MUST be `pty_probe` (no extension): the sidecar registers the command from the
// package `bin/<name>` layout and `openShell({ command: "pty_probe" })` resolves
// it.
function buildCProbe(binDir: string): void {
	const clang = join(WASI_SDK, "bin/clang");
	if (!existsSync(clang)) {
		throw new Error(`wasi-sdk clang not found at ${clang}`);
	}
	const out = join(binDir, "pty_probe"); // basename MUST be `pty_probe`, no ext
	const result = spawnSync(
		clang,
		[
			"--target=wasm32-wasip1",
			`--sysroot=${join(WASI_SDK, "share/wasi-sysroot")}`,
			"-O2",
			"-o",
			out,
			C_PROBE_SOURCE,
		],
		{ encoding: "utf8" },
	);
	if (result.status !== 0) {
		throw new Error(
			["failed to build pty_probe.c", result.stdout, result.stderr]
				.filter(Boolean)
				.join("\n"),
		);
	}
	// Validate wasm magic so a bad build fails loudly here, not in the resolver.
	const magic = readFileSync(out).subarray(0, 4);
	if (
		!(
			magic[0] === 0x00 &&
			magic[1] === 0x61 &&
			magic[2] === 0x73 &&
			magic[3] === 0x6d
		)
	) {
		throw new Error(
			`pty_probe build is not a wasm module (magic=${magic.toString("hex")})`,
		);
	}
}

// Materialize a self-contained agentOS package directory holding the freshly
// built `pty_probe` WASM (`bin/pty_probe` + `package.json` + `agentos-package.json`).
// The sidecar projects `dir` into `/opt/agentos/bin/pty_probe` and registers it as
// a guest command. Returns the package dir to pass as `software: [<dir>]`.
function materializePtyProbePackage(): string {
	const pkgDir = mkdtempSync(join(tmpdir(), "pty-probe-pkg-"));
	const binDir = join(pkgDir, "bin");
	mkdirSync(binDir);
	buildCProbe(binDir);
	writeFileSync(
		join(pkgDir, "package.json"),
		JSON.stringify({ name: "pty-line-discipline-fixture", version: "0.0.0" }),
	);
	writeFileSync(
		join(pkgDir, "agentos-package.json"),
		JSON.stringify({ name: "pty-line-discipline-fixture", version: "1.0.0" }),
	);
	return pkgDir;
}

function ensureSidecarBuilt(): void {
	const configuredSidecar = process.env.AGENTOS_SIDECAR_BIN;
	if (configuredSidecar) {
		if (!existsSync(configuredSidecar)) {
			throw new Error(
				`AGENTOS_SIDECAR_BIN is set to ${configuredSidecar} but the file does not exist`,
			);
		}
		return;
	}
	if (!existsSync(SIDECAR_BINARY)) {
		const build = spawnSync("cargo", ["build", "-q", "-p", "agentos-sidecar"], {
			cwd: REPO_ROOT,
			encoding: "utf8",
		});
		if (build.status !== 0) {
			throw new Error(
				["failed to build agentos-sidecar", build.stdout, build.stderr]
					.filter(Boolean)
					.join("\n"),
			);
		}
	}
	process.env.AGENTOS_SIDECAR_BIN = SIDECAR_BINARY;
}

// ---------------------------------------------------------------------------
// terminal helpers
// ---------------------------------------------------------------------------

function snapshotLines(term: Terminal): string[] {
	const buffer = term.buffer.active;
	const lines: string[] = [];
	for (let row = 0; row < term.rows; row++) {
		const line = buffer.getLine(buffer.viewportY + row);
		lines.push(line ? line.translateToString(true) : "");
	}
	return lines;
}

function terminalSnapshot(label: string, term: Terminal): string {
	const buffer = term.buffer.active;
	const lines = snapshotLines(term).map(
		(line, i) => `${String(i + 1).padStart(2, "0")}|${line}`,
	);
	return [
		`# ${label}`,
		`cols=${term.cols} rows=${term.rows} cursor=${buffer.cursorX},${buffer.cursorY}`,
		...lines,
	].join("\n");
}

function screenText(term: Terminal): string {
	return snapshotLines(term).join("\n");
}

async function settle(): Promise<void> {
	await new Promise((r) => setTimeout(r, SETTLE_MS));
}

// ---------------------------------------------------------------------------
// case context
// ---------------------------------------------------------------------------

interface Runtime {
	name: "wasm-c" | "js-node";
}

interface Ctx {
	runtime: Runtime;
	vm: AgentOs;
	shellId: string;
	term: Terminal;
	expect: typeof expect;
	writeShell(data: string | Uint8Array): Promise<void>;
	resizeShell(cols: number, rows: number): void;
	waitForScreen(text: string, timeoutMs?: number): Promise<void>;
	waitShellStatus(timeoutMs?: number): Promise<number | "timeout" | "error">;
	settle(): Promise<void>;
	screen(): string;
	currentLine(): string;
	markerLine(prefix: string): string | undefined;
	rawHex(): string;
	snapshot(suffix: string): string;
}

interface Case {
	id: string;
	cols?: number;
	rows?: number;
	run(ctx: Ctx): Promise<void>;
}

// ---------------------------------------------------------------------------
// CASES — one shared table, looped over both runtimes
// ---------------------------------------------------------------------------

const CASES: Case[] = [
	{
		// Cooked-mode terminal echo: typing "abc" (no newline) must paint "abc" on
		// screen via the kernel ECHO flag while the program is still blocked in
		// read(). Correct on both runtimes: wasm-c surfaces the kernel echo via the
		// PTY master, and guest-node now routes stdin through the same kernel PTY.
		id: "cooked-echo",
		async run(ctx) {
			await ctx.waitForScreen("#READY tag=echo");
			await ctx.writeShell("abc");
			await ctx.settle();
			ctx.snapshot("echo-while-blocked");
			// Load-bearing honest echo assertion (kernel echo, NOT readback).
			ctx.expect(ctx.screen()).toContain("abc");
			await ctx.writeShell("\n");
			await ctx.waitForScreen("#BYTES tag=echo");
			ctx.snapshot("after-newline");
			ctx
				.expect(ctx.screen())
				.toContain("#BYTES tag=echo n=4 hex=61 62 63 0A text=abc\\n");
			await ctx.waitForScreen("#DONE id=cooked-echo");
		},
	},
	{
		// ECHOCTL: a non-signal control byte (^A 0x01) should echo as caret "^A".
		// The kernel caret-echoes control chars; correct on both runtimes now that
		// guest-node stdin flows through the kernel PTY.
		id: "control-char-echo",
		async run(ctx) {
			await ctx.waitForScreen("#READY tag=ctl");
			await ctx.writeShell("\x01");
			await ctx.settle();
			ctx.snapshot("echo");
			ctx.expect(ctx.screen()).toContain("^A");
			await ctx.writeShell("\n");
			await ctx.waitForScreen("#BYTES tag=ctl");
			ctx.snapshot("report");
			ctx
				.expect(ctx.screen())
				.toContain("#BYTES tag=ctl n=2 hex=01 0A text=\\x01\\n");
		},
	},
	{
		// RAW mode must not echo. Correct on both runtimes: guest-node's stdin is a
		// real PTY now, so setRawMode() succeeds (rc=0) and disables echo just like
		// the wasm-c probe.
		id: "raw-no-echo",
		async run(ctx) {
			await ctx.waitForScreen("#READY tag=raw");
			await ctx.writeShell("abc");
			await ctx.settle();
			const blocked = ctx.snapshot("blocked");
			ctx.expect(blocked).not.toContain("abc");
			ctx.expect(ctx.screen()).toContain("#MODE want=raw rc=0");
			await ctx.writeShell("!");
			await ctx.waitForScreen("#BYTES tag=raw");
			ctx.snapshot("done");
			ctx
				.expect(ctx.screen())
				.toContain("#BYTES tag=raw n=4 hex=61 62 63 21 text=abc!");
			await ctx.waitForScreen("#DONE id=raw-no-echo");
		},
	},
	{
		// VERASE (DEL 0x7f) in cooked mode erases one char (08 20 08 echo) and the
		// delivered line drops it. Correct on both runtimes (guest-node stdin is
		// cooked by the kernel PTY).
		id: "backspace",
		async run(ctx) {
			await ctx.waitForScreen("#READY tag=erase");
			await ctx.writeShell("ab");
			await ctx.writeShell(Uint8Array.of(0x7f));
			await ctx.settle();
			ctx.snapshot("echo");
			await ctx.writeShell("\n");
			await ctx.waitForScreen("#BYTES tag=erase");
			ctx.snapshot("report");
			// Load-bearing: VERASE drops the last buffered char, so the delivered
			// line is "a\n" (n=2), independent of the broken cooked screen echo.
			ctx
				.expect(ctx.markerLine("#BYTES tag=erase"))
				.toBe("#BYTES tag=erase n=2 hex=61 0A text=a\\n");
		},
	},
	{
		// VKILL (^U 0x15) should clear the whole line. Implemented in the kernel
		// (clears the canonical line buffer + echoes the erase). Correct on both
		// runtimes now that guest-node stdin flows through the kernel PTY.
		id: "kill-line",
		async run(ctx) {
			await ctx.waitForScreen("#READY tag=kill");
			await ctx.writeShell("abc");
			await ctx.writeShell("\x15");
			await ctx.settle();
			ctx.snapshot("after-kill");
			ctx.expect(ctx.screen()).not.toContain("abc");
			await ctx.writeShell("\n");
			await ctx.waitForScreen("#BYTES tag=kill");
			ctx.snapshot("report");
			ctx.expect(ctx.screen()).toContain("#BYTES tag=kill n=1 hex=0A text=\\n");
		},
	},
	{
		// VWERASE (^W 0x17) should erase the last word. Implemented in the kernel
		// (erases the trailing non-blank run, keeping the preceding space). Correct
		// on both runtimes now that guest-node stdin flows through the kernel PTY.
		id: "word-erase",
		async run(ctx) {
			await ctx.waitForScreen("#READY tag=werase");
			await ctx.writeShell("foo bar\x17");
			await ctx.settle();
			ctx.snapshot("echo");
			ctx.expect(ctx.screen()).not.toContain("foo bar");
			await ctx.writeShell("\n");
			await ctx.waitForScreen("#BYTES tag=werase");
			ctx.snapshot("report");
			ctx
				.expect(ctx.screen())
				.toContain("#BYTES tag=werase n=5 hex=66 6F 6F 20 0A text=foo \\n");
		},
	},
	{
		// ICANON line buffering: read stays blocked (no #BYTES) until '\n', then
		// the whole line is delivered. EMPIRICALLY this delivery path works for
		// BOTH runtimes (the kernel buffers + delivers the cooked line to guest-node
		// stdin too), so we assert only buffering+delivery, not the broken echo.
		id: "line-buffering",
		async run(ctx) {
			await ctx.waitForScreen("#READY tag=canon");
			await ctx.writeShell("hello");
			await ctx.settle();
			const held = ctx.snapshot("held");
			// Load-bearing: kernel is HOLDING the line (no read returned yet).
			ctx.expect(held).not.toContain("#BYTES tag=canon");
			await ctx.writeShell("\n");
			await ctx.waitForScreen("#BYTES tag=canon");
			ctx.snapshot("delivered");
			ctx
				.expect(ctx.screen())
				.toContain("#BYTES tag=canon n=6 hex=68 65 6C 6C 6F 0A text=hello\\n");
		},
	},
	{
		// ISIG VINTR (^C 0x03) raises SIGINT to the foreground pgid; the byte is
		// neither delivered nor echoed. The C probe has no handler and is
		// terminated. The guest-Node probe installs a handler, which must run and
		// exit cleanly without receiving the control byte as stdin.
		id: "sigint",
		async run(ctx) {
			await ctx.waitForScreen("#READY tag=sigint");
			await ctx.writeShell("\x03");
			const status = await ctx.waitShellStatus();
			ctx.snapshot("after");
			// Correct: VINTR is consumed as a signal, so the byte is neither
			// echoed nor delivered.
			ctx.expect(ctx.screen()).not.toContain("#BYTES tag=sigint");
			ctx.expect(ctx.screen()).not.toContain("^C");
			if (ctx.runtime.name === "js-node") {
				ctx.expect(ctx.screen()).toContain("#SIG name=SIGINT");
				ctx.expect(ctx.screen()).toContain("#DONE id=sigint");
			} else {
				ctx.expect(ctx.screen()).not.toContain("#DONE id=sigint");
			}
			// waitShell resolving proves the signal was acted on instead of the
			// read blocking indefinitely.
			ctx.expect(status).not.toBe("timeout");
			ctx.expect(status).not.toBe("error");
		},
	},
	{
		// ISIG VQUIT raises SIGQUIT. The Node probe catches it and exits zero;
		// the C probe keeps the default terminating disposition.
		id: "sigquit",
		async run(ctx) {
			await ctx.waitForScreen("#READY tag=sigquit");
			await ctx.writeShell(Uint8Array.of(0x1c));
			const status = await ctx.waitShellStatus();
			// The process status and final stderr/output are separate protocol
			// events. Let the terminal consume the trailing output before
			// capturing a deterministic screen.
			await ctx.settle();
			ctx.expect(ctx.screen()).not.toContain("#BYTES tag=sigquit");
			if (ctx.runtime.name === "js-node") {
				ctx.expect(status).toBe(0);
				ctx.expect(ctx.screen()).toContain("#SIG name=SIGQUIT");
				ctx.expect(ctx.screen()).toContain("#DONE id=sigquit");
			} else {
				ctx.expect(status).toBe(131);
				ctx.expect(ctx.screen()).not.toContain("#DONE id=sigquit");
			}
			ctx.snapshot("after");
		},
	},
	{
		// RAW mode (ISIG off): VINTR (0x03) is ordinary data, not a signal. The
		// observable (#BYTES n=1 hex=03) is identical on both runtimes -> green.
		id: "raw-ctrlc-byte",
		async run(ctx) {
			await ctx.waitForScreen("#READY tag=rawc");
			ctx.snapshot("before-input");
			await ctx.writeShell("\x03");
			await ctx.waitForScreen("#BYTES tag=rawc");
			ctx.snapshot("after");
			ctx
				.expect(ctx.screen())
				.toContain("#BYTES tag=rawc n=1 hex=03 text=\\x03");
			ctx.expect(ctx.screen()).not.toContain("^C");
			await ctx.waitForScreen("#DONE id=raw-ctrlc-byte");
		},
	},
	{
		// VSUSP consumes Ctrl-Z as SIGTSTP. The Node probe catches it and exits;
		// the C probe retains the default stop disposition.
		id: "vsusp",
		async run(ctx) {
			await ctx.waitForScreen("#READY tag=susp");
			await ctx.writeShell(Uint8Array.of(0x1a));
			await ctx.settle();
			ctx.expect(ctx.screen()).not.toContain("#BYTES tag=susp");
			if (ctx.runtime.name === "js-node") {
				ctx.expect(await ctx.waitShellStatus()).toBe(0);
				await ctx.waitForScreen("#DONE id=vsusp");
				ctx.expect(ctx.screen()).toContain("#SIG name=SIGTSTP");
			} else {
				ctx.expect(ctx.screen()).not.toContain("#DONE id=vsusp");
			}
			// Not echoed: neither the raw control byte nor an ECHOCTL caret ("^Z").
			ctx.expect(ctx.screen()).not.toContain("^Z");
			ctx.expect(ctx.screen()).not.toContain("\\x1A");
			ctx.snapshot("after");
		},
	},
	{
		// VERASE alias ^H (0x08): the kernel maps both DEL (0x7f) and ^H (0x08) to
		// the erase op, so typing "ab" + ^H + '\n' drops the last buffered char and
		// delivers "a\n". Pure line discipline -> correct on both runtimes.
		id: "erase-ctrl-h",
		async run(ctx) {
			await ctx.waitForScreen("#READY tag=eraseh");
			await ctx.writeShell("ab");
			await ctx.writeShell(Uint8Array.of(0x08));
			await ctx.settle();
			ctx.snapshot("echo");
			await ctx.writeShell("\n");
			await ctx.waitForScreen("#BYTES tag=eraseh");
			ctx.snapshot("report");
			// Load-bearing: ^H erased the last buffered byte exactly like DEL, so the
			// delivered line is "a\n" (n=2), independent of the screen echo.
			ctx
				.expect(ctx.markerLine("#BYTES tag=eraseh"))
				.toBe("#BYTES tag=eraseh n=2 hex=61 0A text=a\\n");
		},
	},
	{
		// VINTR mid-line BOTH flushes the canonical input buffer AND raises SIGINT.
		// Neither probe installs a SIGINT handler in this case, so this
		// cell observes the kill half on both: "abc" is buffered but never
		// delivered (no #BYTES), the probe never completes (no #DONE), and the
		// shell terminates instead of blocking the full 15s. The buffer-flush
		// half is covered by the kernel pty unit tests (canonical buffer cleared
		// on VINTR), which don't need a signal-surviving guest to observe it.
		id: "vintr-buffer",
		async run(ctx) {
			await ctx.waitForScreen("#READY tag=vintrbuf");
			await ctx.writeShell("abc");
			await ctx.settle();
			await ctx.writeShell("\x03");
			const status = await ctx.waitShellStatus();
			ctx.expect(status).toBe(130);
			await ctx.settle();
			// Killed by SIGINT mid-read: "abc" is never delivered (no #BYTES) and
			// the probe never completes (no #DONE). waitShell RESOLVED (not a 15s
			// "timeout"), proving the signal actually terminated it.
			ctx.expect(ctx.screen()).not.toContain("#BYTES tag=vintrbuf");
			ctx.expect(ctx.screen()).not.toContain("#DONE id=vintr-buffer");
			ctx.expect(status).not.toBe("timeout");
			ctx.expect(status).not.toBe("error");
		},
	},
	{
		// OPOST+ONLCR: a raw-written lone LF must become CRLF on the master read
		// path for both runtimes, so the master reads 61 0D 0A 62.
		id: "onlcr",
		async run(ctx) {
			await ctx.waitForScreen("#DONE id=onlcr");
			ctx.expect(ctx.rawHex()).toContain("61 0D 0A 62");
			ctx.expect(ctx.rawHex()).not.toContain("61 0A 62");
			ctx.snapshot("onlcr");
		},
	},
	{
		// ICRNL maps a typed CR (0x0D) to NL (0x0A) in cooked mode: it both
		// terminates the line and is the byte delivered. Correct on both runtimes
		// (guest-node stdin is cooked by the kernel PTY).
		id: "icrnl",
		async run(ctx) {
			await ctx.waitForScreen("#READY tag=icrnl");
			await ctx.writeShell("x");
			await ctx.settle();
			const echoSnap = ctx.snapshot("echo");
			// Read still blocked (no terminator yet): the line is held by canonical
			// mode, so the delivery marker must not be present.
			ctx.expect(echoSnap).not.toContain("#BYTES tag=icrnl");
			await ctx.writeShell("\r");
			await ctx.waitForScreen("#BYTES tag=icrnl");
			ctx.snapshot("final");
			// Load-bearing: the typed CR (0x0D) was mapped by ICRNL to NL (0x0A),
			// terminating the line AND being the byte delivered -> "x\n" (78 0A).
			ctx
				.expect(ctx.screen())
				.toContain("#BYTES tag=icrnl n=2 hex=78 0A text=x\\n");
		},
	},
	{
		// VEOF (^D 0x04) on an empty line in cooked mode -> read() returns 0 (EOF),
		// not echoed. Correct on both runtimes: guest-node reads the kernel PTY,
		// so ^D surfaces as a 0-length read (EOF), not a data byte.
		id: "eof",
		async run(ctx) {
			await ctx.waitForScreen("#READY tag=eof");
			ctx.snapshot("ready");
			await ctx.writeShell(Uint8Array.of(0x04));
			await ctx.waitForScreen("#DONE id=eof");
			ctx.snapshot("done");
			// ^D never echoed on either runtime.
			ctx.expect(ctx.screen()).not.toContain("^D");
			ctx.expect(ctx.screen()).not.toContain("\\x04");
			// Correct: clean EOF, no data byte delivered.
			ctx.expect(ctx.screen()).toContain("#EOF tag=eof n=0");
			ctx.expect(ctx.screen()).not.toContain("#BYTES tag=eof");
		},
	},
	{
		// SIGWINCH / live window size: after the host resizes the PTY, the probe
		// re-queries and must see the NEW size. The sidecar forwards the
		// kernel resize as SIGWINCH to embedded V8 so js-node re-queries it.
		id: "resize-sigwinch",
		cols: 80,
		rows: 24,
		async run(ctx) {
			await ctx.waitForScreen("#SIZE tag=before");
			await ctx.waitForScreen("#READY tag=resize");
			ctx.resizeShell(120, 40);
			if (ctx.runtime.name === "js-node") {
				// Keep the probe alive until the asynchronous SIGWINCH handler has
				// queried the resized PTY. Releasing it first races process.exit(0)
				// against the coalesced signal wake.
				await ctx.waitForScreen("#SIZE tag=after rc=0 cols=120 rows=40");
			}
			// Sentinel '!' + CR (ICRNL -> NL flushes the cooked line). WASM has
			// no signal handler, so it re-queries only after this read completes.
			await ctx.writeShell("!\r");
			await ctx.waitForScreen("#SIZE tag=after rc=0 cols=120 rows=40");
			await ctx.waitForScreen("#DONE id=resize-sigwinch");
			ctx.snapshot("resize");
			ctx
				.expect(ctx.screen())
				.toContain("#SIZE tag=before rc=0 cols=80 rows=24");
		},
	},
	{
		// Cursor Position Report round-trip: probe writes ESC[6n and reads the
		// emulator's ESC[<r>;<c>R reply. Works on BOTH runtimes (guest-node stdin
		// is a non-canonical data stream, so the newline-less reply still flows);
		// only the #MODE marker diverges, which we do not assert.
		id: "cpr",
		async run(ctx) {
			await ctx.waitForScreen("#CPR sent=1");
			await ctx.waitForScreen("#CPRREPLY");
			ctx.snapshot("cpr-reply");
			const m = ctx.screen().match(/#CPRREPLY n=\d+ hex=[0-9A-F ]+ text=(\S+)/);
			ctx.expect(m, "expected a #CPRREPLY marker line").toBeTruthy();
			ctx.expect(m?.[1] ?? "").toMatch(/^\\e\[\d+;\d+R$/);
		},
	},
	{
		// isatty(0/1/2): a PTY slave dup2'd onto the fds reports TTY for all three.
		// Correct on both runtimes: guest-node's TTY config is now derived from the
		// kernel isatty bridge, so process.stdin/stdout/stderr.isTTY are all true.
		id: "isatty",
		cols: 80,
		rows: 24,
		async run(ctx) {
			await ctx.waitForScreen("#TTY ");
			ctx.snapshot("tty");
			ctx.expect(ctx.markerLine("#TTY ")).toBe("#TTY in=1 out=1 err=1");
			await ctx.waitForScreen("#DONE id=isatty");
		},
	},
	{
		// Window size: the kernel slave winsize must match openShell({cols,rows}).
		// Correct on both runtimes: guest-node's process.stdout.columns/rows are now
		// read live from the kernel PTY winsize. Non-default dims so a stale 80x24
		// fallback can't pass by accident.
		id: "winsize",
		cols: 100,
		rows: 37,
		async run(ctx) {
			await ctx.waitForScreen("#SIZE tag=open");
			await ctx.waitForScreen("#DONE id=winsize");
			ctx.snapshot("winsize");
			ctx
				.expect(ctx.screen())
				.toContain("#SIZE tag=open rc=0 cols=100 rows=37");
		},
	},
];

const RUNTIMES: Runtime[] = [{ name: "wasm-c" }, { name: "js-node" }];

// ---------------------------------------------------------------------------
// suite
// ---------------------------------------------------------------------------

describe("PTY line discipline matrix", () => {
	let vm: AgentOs | undefined;

	beforeAll(async () => {
		ensureSidecarBuilt();

		const { AgentOs } = await import("../src/index.js");
		const software = [];
		if (ENABLE_WASM_C_PTY) {
			software.push({ packagePath: materializePtyProbePackage() });
		}
		vm = await AgentOs.create({
			software,
		});
		await vm.writeFile(NODE_PROBE_GUEST_PATH, readFileSync(NODE_PROBE_SOURCE));
	}, 180_000);

	afterAll(async () => {
		if (vm) {
			await vm.dispose();
			vm = undefined;
		}
	});

	for (const rt of RUNTIMES) {
		const runtimeSuite =
			rt.name === "wasm-c" && !ENABLE_WASM_C_PTY ? describe.skip : describe;
		runtimeSuite(rt.name, () => {
			let term: Terminal | undefined;
			let shellId: string | undefined;
			let unsubscribe: (() => void) | undefined;
			let disposeOnData: { dispose(): void } | undefined;
			let rawBytes: number[] = [];

			afterEach(async () => {
				unsubscribe?.();
				unsubscribe = undefined;
				disposeOnData?.dispose();
				disposeOnData = undefined;
				if (vm && shellId) {
					try {
						vm.closeShell(shellId);
					} catch {
						// probe may already have exited
					}
				}
				term?.dispose();
				term = undefined;
				shellId = undefined;
				rawBytes = [];
			});

			for (const c of CASES) {
				registerCase(rt, c, {
					getVm: () => vm,
					setState: (t, sid, un, dod) => {
						term = t;
						shellId = sid;
						unsubscribe = un;
						disposeOnData = dod;
					},
					getRawBytes: () => rawBytes,
				});
			}
		});
	}
});

// ---------------------------------------------------------------------------
// per-cell registration (kept out of the loop body for clarity)
// ---------------------------------------------------------------------------

interface CellHooks {
	getVm(): AgentOs | undefined;
	setState(
		term: Terminal,
		shellId: string,
		unsubscribe: () => void,
		disposeOnData: { dispose(): void },
	): void;
	getRawBytes(): number[];
}

function registerCase(rt: Runtime, c: Case, hooks: CellHooks): void {
	it(
		c.id,
		async () => {
			const vm = hooks.getVm();
			if (!vm) throw new Error("vm not initialized");

			const cols = c.cols ?? 80;
			const rows = c.rows ?? 24;
			const term = new Terminal({ cols, rows, allowProposedApi: true });
			const rawBytes = hooks.getRawBytes();

			const command = rt.name === "wasm-c" ? "pty_probe" : "node";
			const args =
				rt.name === "wasm-c" ? [c.id] : [NODE_PROBE_GUEST_PATH, c.id];

			const { shellId } = vm.openShell({
				command,
				args,
				cols,
				rows,
				env: {
					TERM: "xterm-256color",
					COLUMNS: String(cols),
					LINES: String(rows),
				},
			});

			const unsubscribe = vm.onShellData(shellId, (event) => {
				for (const byte of event.data) rawBytes.push(byte);
				term.write(event.data);
			});
			const disposeOnData = term.onData((data) => {
				vm.writeShell(shellId, data);
			});
			hooks.setState(term, shellId, unsubscribe, disposeOnData);

			const ctx: Ctx = {
				runtime: rt,
				vm,
				shellId,
				term,
				expect,
				async writeShell(data) {
					await vm.writeShell(shellId, data);
				},
				resizeShell(c2, r2) {
					term.resize(c2, r2);
					vm.resizeShell(shellId, c2, r2);
				},
				async waitForScreen(text, timeoutMs = WAIT_TIMEOUT_MS) {
					const deadline = Date.now() + timeoutMs;
					while (Date.now() < deadline) {
						if (screenText(term).includes(text)) {
							await settle();
							return;
						}
						await new Promise((r) => setTimeout(r, 25));
					}
					throw new Error(
						`timed out waiting for ${JSON.stringify(text)}\n${terminalSnapshot(
							"timeout",
							term,
						)}`,
					);
				},
				async waitShellStatus(timeoutMs = WAIT_TIMEOUT_MS) {
					return await Promise.race<number | "timeout" | "error">([
						vm.waitShell(shellId).then(
							(s) => s,
							() => "error" as const,
						),
						new Promise<"timeout">((r) =>
							setTimeout(() => r("timeout"), timeoutMs),
						),
					]);
				},
				settle,
				screen() {
					return screenText(term);
				},
				currentLine() {
					const buffer = term.buffer.active;
					const line = buffer.getLine(buffer.cursorY + buffer.viewportY);
					return (line ? line.translateToString(true) : "").replace(/\s+$/, "");
				},
				markerLine(prefix) {
					for (const line of snapshotLines(term)) {
						const idx = line.indexOf(prefix);
						if (idx !== -1) return line.slice(idx).replace(/\s+$/, "");
					}
					return undefined;
				},
				rawHex() {
					return rawBytes
						.map((b) => b.toString(16).toUpperCase().padStart(2, "0"))
						.join(" ");
				},
				snapshot(suffix) {
					const label = `${rt.name}/${c.id}/${suffix}`;
					const snap = terminalSnapshot(label, term);
					if (rt.name === "js-node") {
						expect(snap).toMatchSnapshot(label);
					}
					return snap;
				},
			};

			await c.run(ctx);
		},
		TEST_TIMEOUT_MS,
	);
}
