// In-memory fixtures for the inspector's dummy/offline mode (see `mock.ts`).
// Every actor action the tabs call has a handler here returning canned data in
// the SAME shape the real action returns, so the tabs render identically with
// no live actor. The tree below mirrors what `tests/fixtures/demo.mjs` seeds
// into the live VM, so mock mode looks like the real demo. Edit this file to
// test other shapes.
import type {
	PersistedSessionEvent,
	PersistedSessionRecord,
	ProcessInfo,
	ReaddirEntry,
	SoftwareInfo,
	VirtualStat,
} from "./types";

const dir = (name: string): ReaddirEntry => ({ name, isDirectory: true, isSymbolicLink: false });
const file = (name: string): ReaddirEntry => ({ name, isDirectory: false, isSymbolicLink: false });

// One-level-at-a-time tree, keyed by directory path (matches `readdirEntries`).
// Mirrors demo.mjs's seeded layout plus a couple of base VM dirs.
const TREE: Record<string, ReaddirEntry[]> = {
	"/": [dir("root"), dir("workspace"), dir("tmp"), dir("etc"), dir("proc")],
	"/root": [file("README.md"), file("blob.bin"), file("data.json"), dir("logs"), dir("project")],
	"/root/logs": [file("app.log")],
	"/root/project": [file("main.py"), file("package.json"), dir("src"), dir("tests")],
	"/root/project/src": [file("index.ts"), dir("lib")],
	"/root/project/src/lib": [file("math.ts")],
	"/root/project/tests": [file("math.test.ts")],
	"/workspace": [dir("notes")],
	"/workspace/notes": [file("todo.md")],
	"/tmp": [file("agentos-demo.txt")],
	"/etc": [file("hostname"), file("hosts")],
};

// File contents for `readFile` (text). A file in TREE but absent here → empty.
const FILES: Record<string, string> = {
	"/root/README.md":
		"# agentOS demo VM\n\nSeeded by tests/fixtures/demo.mjs so the\ninspector tabs have live data to explore.\n\n- Filesystem: this tree\n- Processes: a few sleeps\n- Software/Mounts: live config\n",
	"/root/data.json": JSON.stringify(
		{ demo: true, items: [1, 2, 3], nested: { a: 1, b: [true, false] } },
		null,
		2,
	),
	"/root/logs/app.log":
		Array.from(
			{ length: 20 },
			(_, i) => `[2026-06-29T17:0${i % 10}:00Z] INFO request ${i} handled in ${10 + i}ms`,
		).join("\n") + "\n",
	"/root/project/package.json": JSON.stringify(
		{ name: "demo-project", version: "1.0.0", scripts: { build: "tsc" } },
		null,
		2,
	),
	"/root/project/main.py":
		"def main():\n\tprint('hello from python')\n\nif __name__ == '__main__':\n\tmain()\n",
	"/root/project/src/index.ts":
		"import { add } from './lib/math';\n\nexport function main(): void {\n\tconsole.log('sum', add(2, 3));\n}\n",
	"/root/project/src/lib/math.ts":
		"export const add = (a: number, b: number) => a + b;\nexport const mul = (a: number, b: number) => a * b;\n",
	"/root/project/tests/math.test.ts":
		"import { add } from '../src/lib/math';\n\ntest('add', () => expect(add(2, 3)).toBe(5));\n",
	"/workspace/notes/todo.md": "# TODO\n\n- [x] ship inspector tabs\n- [ ] sync the look\n",
	"/tmp/agentos-demo.txt": "hello from a dummy agentOS+RivetKit example",
	"/etc/hostname": "agentos-demo-vm\n",
	"/etc/hosts": "127.0.0.1\tlocalhost\n",
};

// Binary file (NUL byte → the viewer shows "Binary file", preview unavailable).
// Same bytes demo.mjs writes to /root/blob.bin.
const BINARY: Record<string, number[]> = {
	"/root/blob.bin": [0, 1, 2, 3, 255, 254, 0, 128, 7, 42, 0, 99],
};

const DAY = 86_400_000;
const now = Date.now();

function statFor(path: string): VirtualStat {
	const isDir = path === "/" || path in TREE;
	const text = FILES[path];
	const bin = BINARY[path];
	const size = isDir ? 0 : bin ? bin.length : text ? new TextEncoder().encode(text).length : 0;
	return { size, mtimeMs: now - DAY, isDirectory: isDir, isSymbolicLink: false };
}

function utf8ToBase64(text: string): string {
	return btoa(String.fromCharCode(...new TextEncoder().encode(text)));
}

const SOFTWARE: SoftwareInfo[] = [
	{ package: "@agentos-software/claude-code", kind: "agent", version: "0.0.0-mock", commands: [] },
	{ package: "@agentos-software/codex", kind: "agent", version: "0.3.0-rc.2", commands: [] },
	{ package: "@agentos-software/coreutils", kind: "wasm-commands", version: "0.3.0-rc.2", commands: ["cat", "cut", "head", "ln", "ls", "tail", "test", "wc"] },
	{ package: "@agentos-software/grep", kind: "wasm-commands", version: "0.3.0-rc.2", commands: ["egrep", "fgrep", "grep"] },
	{ package: "@agentos-software/git", kind: "wasm-commands", version: "0.3.0-rc.2", commands: ["git"] },
	{ package: "@agentos-software/jq", kind: "wasm-commands", version: "0.3.0-rc.2", commands: ["jq"] },
	{ package: "my-local-tool", kind: "tool", version: null, commands: [] },
];

// Mirrors demo.mjs's four spawned processes.
const PROCESSES: ProcessInfo[] = [
	{ pid: 8, command: "sleep", args: ["86400"], running: true, exitCode: null, startedAt: 1751302800000 },
	{ pid: 9, command: "sleep", args: ["7200"], running: true, exitCode: null, startedAt: 1751302801000 },
	{ pid: 10, command: "sleep", args: ["1800"], running: true, exitCode: null, startedAt: 1751302802000 },
	{ pid: 11, command: "sh", args: ["-c", "sleep 99999"], running: true, exitCode: null, startedAt: 1751302803000 },
];

// Mirrors the four mounts in tests/fixtures/agentos-runtime-server.ts.
const MOUNTS = [
	{ path: "/scratch", kind: "memory", readOnly: false, config: null },
	{ path: "/cache", kind: "memory", readOnly: false, config: null },
	{ path: "/data", kind: "memory", readOnly: false, config: null },
	{ path: "/host-tmp", kind: "host_dir", readOnly: true, config: { hostPath: "/tmp" } },
];

// The demo runs no agent, so a real VM has zero persisted sessions. These are
// illustrative so the Transcript tab can be exercised in its populated state.
const SESSIONS: PersistedSessionRecord[] = [
	{ sessionId: "sess-claude-001", agentType: "claude-code", createdAt: now - 2 * DAY, status: "running" },
	{ sessionId: "sess-codex-002", agentType: "codex", createdAt: now - DAY, status: "idle" },
];

function mkUpdate(
	sessionId: string,
	seq: number,
	sessionUpdate: string,
	text: string,
): PersistedSessionEvent {
	return {
		sessionId,
		seq,
		createdAt: now - DAY + seq * 1000,
		event: {
			jsonrpc: "2.0",
			method: "session/update",
			params: { update: { sessionUpdate, content: { type: "text", text } } },
		},
	};
}

function mkToolCall(
	sessionId: string,
	seq: number,
	title: string,
	status: string,
): PersistedSessionEvent {
	return {
		sessionId,
		seq,
		createdAt: now - DAY + seq * 1000,
		event: {
			jsonrpc: "2.0",
			method: "session/update",
			params: { update: { sessionUpdate: "tool_call", title, status } },
		},
	};
}

const TRANSCRIPTS: Record<string, PersistedSessionEvent[]> = {
	"sess-claude-001": [
		mkUpdate("sess-claude-001", 1, "user_message_chunk", "List the files in /root/project."),
		mkUpdate("sess-claude-001", 2, "agent_thought_chunk", "I should read the project directory."),
		mkUpdate("sess-claude-001", 3, "agent_message_chunk", "Here are the files under /root/project:"),
		mkToolCall("sess-claude-001", 4, "readdir", "completed"),
		mkUpdate("sess-claude-001", 5, "agent_message_chunk", "main.py, package.json, src/, tests/."),
	],
	"sess-codex-002": [
		mkUpdate("sess-codex-002", 1, "user_message_chunk", "Run the tests."),
		mkToolCall("sess-codex-002", 2, "shell", "in_progress"),
	],
};

/** Action name → handler. Args mirror the real action arg tuples. */
export const mockActions: Record<string, (args: unknown[]) => unknown> = {
	listSoftware: () => SOFTWARE,
	listProcesses: () => PROCESSES,
	listMounts: () => MOUNTS,
	listPersistedSessions: () => SESSIONS,
	getSessionEvents: ([sessionId]) => TRANSCRIPTS[sessionId as string] ?? [],
	readdirEntries: ([path]) => TREE[path as string] ?? [],
	stat: ([path]) => statFor(path as string),
	readFile: ([path]) => {
		const p = path as string;
		if (BINARY[p]) return BINARY[p];
		return ["$Uint8Array", utf8ToBase64(FILES[p] ?? "")];
	},
};
