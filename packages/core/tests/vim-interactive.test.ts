import {
	copyFileSync,
	existsSync,
	mkdirSync,
	mkdtempSync,
	readFileSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import pkg from "@xterm/headless";
import { afterEach, describe, expect, it } from "vitest";
import { __disposeAllSharedSidecarsForTesting } from "../src/agent-os.js";
import type { AgentOs } from "../src/index.js";
import { allowAll } from "../src/runtime-compat.js";

const { Terminal } = pkg;

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, "../../..");
const VIM_COMMAND_DIR = resolve(REPO_ROOT, ".local-cmds");
const VIM_BINARY = resolve(VIM_COMMAND_DIR, "vim");
const SNAP_DIR =
	"/home/nathan/progress/agent-os/2026-06-28-just-shell-fix/vim-snapshots";

const VIM_ARGS = [
	"-N",
	"-u",
	"NONE",
	"-i",
	"NONE",
	"-n",
	"--cmd",
	"set noesckeys",
];
const ESC = Uint8Array.of(0x1b);
const TEST_TIMEOUT_MS = 180_000;

function assertVimAvailable() {
	if (!existsSync(VIM_BINARY)) {
		throw new Error(`vim wasm fixture not found at ${VIM_BINARY}`);
	}
	const magic = readFileSync(VIM_BINARY).subarray(0, 4);
	expect([...magic]).toEqual([0x00, 0x61, 0x73, 0x6d]);
}

// Materialize the raw `.local-cmds/vim` wasm into a self-contained agentOS
// package directory (`bin/vim` + `package.json` + `agentos-package.json`). The
// sidecar projects it into `/opt/agentos/bin/vim` and registers the command, so
// `openShell({ command: "vim" })` resolves it. Mirrors packages/shell/src/main.ts
// and tests/pty-protocol.test.ts's package materialization.
function materializeVimPackage(): { packageDir: string } {
	const packageDir = mkdtempSync(join(tmpdir(), "agentos-vim-pkg-"));
	const binDir = join(packageDir, "bin");
	mkdirSync(binDir);
	copyFileSync(VIM_BINARY, join(binDir, "vim"));
	writeFileSync(
		join(packageDir, "package.json"),
		JSON.stringify({ name: "vim", version: "0.0.0" }),
	);
	writeFileSync(
		join(packageDir, "agentos-package.json"),
		JSON.stringify({ name: "vim" }),
	);
	return { packageDir };
}

function screen(term: InstanceType<typeof Terminal>): string {
	const buffer = term.buffer.active;
	const lines: string[] = [];
	for (let y = 0; y < term.rows; y++) {
		const line = buffer.getLine(y);
		lines.push((line ? line.translateToString(true) : "").replace(/\s+$/, ""));
	}
	return `${lines.join("\n").replace(/\n+$/, "")}\n`;
}

async function sleep(ms: number) {
	await new Promise((resolve) => setTimeout(resolve, ms));
}

// Requires the vim wasm binary staged locally at `.local-cmds/vim`. CI does not
// build or stage wasm editors, so skip when the fixture is absent rather than
// failing the suite (same policy as brush-interactive).
describe.skipIf(!existsSync(VIM_BINARY))("interactive vim over VM PTY", () => {
	let vm: AgentOs | undefined;

	afterEach(async () => {
		await vm?.dispose().catch(() => {});
		vm = undefined;
	}, 120_000);

	it(
		"renders vim, edits a file, writes it, and records per-keystroke snapshots",
		async () => {
			assertVimAvailable();
			mkdirSync(SNAP_DIR, { recursive: true });

			const { AgentOs } = await import("../src/index.js");
			vm = await AgentOs.create({
				permissions: allowAll,
				software: [materializeVimPackage()],
			});
			await vm.mkdir("/work", { recursive: true });

			const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true });
			let writes = Promise.resolve();
			let snapshotIndex = 0;

			const { shellId } = vm.openShell({
				command: "vim",
				args: VIM_ARGS,
				cols: 80,
				rows: 24,
				cwd: "/work",
				env: { TERM: "xterm" },
			});
			const offData = vm.onShellData(shellId, (data) => {
				const bytes = Buffer.from(data);
				writes = writes.then(
					() => new Promise<void>((resolve) => term.write(bytes, resolve)),
				);
			});

			const settle = async (ms = 700) => {
				await sleep(ms);
				await writes;
				await sleep(20);
				await writes;
			};
			const waitForScreen = async (
				predicate: (current: string) => boolean,
				label: string,
				timeoutMs = 20_000,
			) => {
				const deadline = Date.now() + timeoutMs;
				let current = screen(term);
				while (Date.now() < deadline) {
					await settle(250);
					current = screen(term);
					if (predicate(current)) {
						return current;
					}
				}
				throw new Error(`timed out waiting for ${label}\n\n${current}`);
			};
			const snap = async (label: string, ms = 700) => {
				await settle(ms);
				const nn = String(snapshotIndex).padStart(2, "0");
				writeFileSync(
					resolve(SNAP_DIR, `${nn}.txt`),
					`## ${nn} - ${label}\n## (vim args: ${JSON.stringify(VIM_ARGS)})\n----- screen 80x24 -----\n${screen(term)}`,
				);
				snapshotIndex++;
				return screen(term);
			};

			await waitForScreen(
				(current) =>
					current.includes("VIM - Vi IMproved") &&
					!current.includes("Press ENTER"),
				"vim startup splash",
				20_000,
			);
			const startup = await snap("startup (vim launched, no file)", 300);
			expect(startup).toContain("VIM - Vi IMproved");
			expect(startup).not.toContain("Press ENTER");

			const seq: Array<[string | Uint8Array, string, number?]> = [
				[":", "type : (enter command-line)"],
				["e", "e"],
				[" ", "space"],
				["h", "h"],
				["e", "e"],
				["l", "l"],
				["l", "l"],
				["o", "o"],
				[".", "."],
				["t", "t"],
				["x", "x"],
				["t", "t"],
				["\r", "Enter -> run :e hello.txt (open new file)"],
				["i", "i (enter INSERT mode)"],
				["h", "h"],
				["e", "e"],
				["l", "l"],
				["l", "l"],
				["o", "o"],
				[" ", "space"],
				["w", "w"],
				["o", "o"],
				["r", "r"],
				["l", "l"],
				["d", "d"],
				[ESC, "ESC (back to NORMAL)", 900],
				[":", "type : (command-line)"],
				["w", "w"],
				["\r", "Enter -> run :w (write file)", 1200],
			];

			const snapshots: string[] = [];
			for (const [key, label, delayMs] of seq) {
				await vm.writeShell(shellId, key);
				snapshots.push(await snap(label, delayMs ?? 650));
			}

			const opened = snapshots[12] ?? "";
			expect(opened).toContain('"hello.txt" [New]');
			expect(opened).not.toContain("[No Name]");
			expect(opened).not.toContain("E32");
			expect(opened).not.toContain("VIM - Vi IMprovedversion");

			const insert = snapshots[13] ?? "";
			expect(insert).toContain("-- INSERT --");

			const typed = snapshots[24] ?? "";
			expect(typed).toContain("hello world");
			expect(typed).not.toContain("h1,2");
			expect(typed).not.toContain("e3");

			const normal = snapshots[25] ?? "";
			expect(normal).toContain("hello world");
			expect(normal).not.toContain("-- INSERT --");
			expect(normal).not.toContain("^[");
			expect(normal).not.toContain("E32");

			const written = snapshots.at(-1) ?? "";
			expect(written).toContain('"hello.txt" [New] 1L, 12B written');
			expect(written).not.toContain("Press ENTER");
			expect(written).not.toContain("E32");
			expect(written).not.toContain("No file name");
			expect(written).not.toContain("E212");
			expect(written).not.toContain("^[:");

			await vm.writeShell(shellId, ":q\r");
			await settle(1500);

			const fileContent = Buffer.from(await vm.readFile("/work/hello.txt")).toString(
				"utf8",
			);
			writeFileSync(
				resolve(SNAP_DIR, "FILE.txt"),
				`# /work/hello.txt after :w\n${JSON.stringify(fileContent)}\n\n---raw---\n${fileContent}`,
			);
			expect(fileContent).toBe("hello world\n");

			offData();
			void __disposeAllSharedSidecarsForTesting().catch(() => {});
			vm = undefined;
		},
		TEST_TIMEOUT_MS,
	);
});
