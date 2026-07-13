import { AgentOs } from "@rivet-dev/agentos-core";
import { allowAll } from "@rivet-dev/agentos-core/test/runtime";
import assert from "node:assert/strict";
import { copyFileSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import pkg from "@xterm/headless";
const { Terminal } = pkg;

const SNAP_DIR = "/home/nathan/progress/agent-os/2026-06-28-just-shell-fix/vim-snapshots";
const VIM_COMMAND_DIR = "/home/nathan/.herdr/workspaces/agent-os/workspace-brave-forest-1e98/.local-cmds";
const VIM_ARGS = JSON.parse(process.env.VIM_ARGS || '["-N","-u","NONE","-i","NONE","-n","--cmd","set noesckeys"]');
const VIM_ENV = JSON.parse(process.env.VIM_ENV || '{"TERM":"xterm"}');
const ESC = Uint8Array.of(0x1b);

// Materialize the raw `.local-cmds/vim` wasm into a self-contained agentOS
// package directory (`bin/vim` + package.json + agentos-package.json), matching
// the package model consumed by `AgentOs.create({ software: [<dir>] })`.
function materializeVimPackage() {
  const packageDir = mkdtempSync(join(tmpdir(), "agentos-vim-pkg-"));
  mkdirSync(join(packageDir, "bin"));
  copyFileSync(resolve(VIM_COMMAND_DIR, "vim"), join(packageDir, "bin", "vim"));
  writeFileSync(join(packageDir, "package.json"), JSON.stringify({ name: "vim", version: "0.0.0" }));
  writeFileSync(join(packageDir, "agentos-package.json"), JSON.stringify({ name: "vim" }));
  return packageDir;
}

const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true });
let writes = Promise.resolve();
const screen = () => {
  const b = term.buffer.active;
  const lines = [];
  for (let y = 0; y < 24; y++) {
    const ln = b.getLine(y);
    lines.push((ln ? ln.translateToString(true) : "").replace(/\s+$/, ""));
  }
  return lines.join("\n").replace(/\n+$/, "\n");
};
let n = 0;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const settle = async (ms = 700) => {
  await sleep(ms);
  await writes;
  await sleep(20);
  await writes;
};
const waitForScreen = async (predicate, label, timeoutMs = 20_000) => {
  const deadline = Date.now() + timeoutMs;
  let latest = screen();
  while (Date.now() < deadline) {
    await settle(250);
    latest = screen();
    if (predicate(latest)) return latest;
  }
  throw new Error(`timed out waiting for ${label}\n\n${latest}`);
};
const snap = async (label, ms = 700) => {
  await settle(ms);
  const nn = String(n).padStart(2, "0");
  const current = screen();
  writeFileSync(`${SNAP_DIR}/${nn}.txt`, `## ${nn} — ${label}\n## (vim args: ${JSON.stringify(VIM_ARGS)})\n----- screen 80x24 -----\n${current}\n`);
  console.log(`snap ${nn}: ${label}`);
  n++;
  return current;
};

mkdirSync(SNAP_DIR, { recursive: true });

const vm = await AgentOs.create({
  permissions: allowAll,
  software: [materializeVimPackage()],
});
await vm.mkdir("/work", { recursive: true });
const { shellId } = vm.openShell({ command: "vim", args: VIM_ARGS, cols: 80, rows: 24, cwd: "/work", env: VIM_ENV });
vm.onShellData(shellId, (d) => {
  const bytes = Buffer.from(d);
  writes = writes.then(() => new Promise((resolve) => term.write(bytes, resolve)));
});

await waitForScreen((s) => s.includes("VIM - Vi IMproved") && !s.includes("Press ENTER"), "vim startup");
const startup = await snap("startup (vim launched, no file)", 300);
assert.match(startup, /VIM - Vi IMproved/);
assert.doesNotMatch(startup, /Press ENTER/);

const SEQ = [
  [":", "type : (enter command-line)"], ["e", "e"], [" ", "space"],
  ["h", "h"], ["e", "e"], ["l", "l"], ["l", "l"], ["o", "o"], [".", "."], ["t", "t"], ["x", "x"], ["t", "t"],
  ["\r", "Enter -> run :e hello.txt (open new file)"],
  ["i", "i (enter INSERT mode)"],
  ["h", "h"], ["e", "e"], ["l", "l"], ["l", "l"], ["o", "o"], [" ", "space"], ["w", "w"], ["o", "o"], ["r", "r"], ["l", "l"], ["d", "d"],
  [ESC, "ESC (back to NORMAL)", 900],
  [":", "type : (command-line)"], ["w", "w"], ["\r", "Enter -> run :w (write file)"],
];
const snapshots = [];
for (const [key, label, delayMs] of SEQ) {
  await vm.writeShell(shellId, key);
  snapshots.push(await snap(label, delayMs ?? 650));
}

const opened = snapshots[12] ?? "";
assert.match(opened, /"hello\.txt" \[New\]/);
assert.doesNotMatch(opened, /No Name/);

const insert = snapshots[13] ?? "";
assert.match(insert, /-- INSERT --/);

const typed = snapshots[24] ?? "";
assert.match(typed, /hello world/);

const normal = snapshots[25] ?? "";
assert.doesNotMatch(normal, /-- INSERT --/);
assert.match(normal, /hello world/);

const written = snapshots.at(-1) ?? "";
assert.match(written, /"hello\.txt" \[New\] 1L, 12B written/);
assert.doesNotMatch(written, /E32/);
assert.doesNotMatch(written, /Press ENTER/);
assert.doesNotMatch(written, /\^\[/);

await vm.writeShell(shellId, ":q\r");
await settle(1500);

// read the file back from the VM
let fileContent = "<read failed>";
try { fileContent = Buffer.from(await vm.readFile("/work/hello.txt")).toString("utf8"); } catch (e) { fileContent = "ERR: " + (e?.message || e); }
writeFileSync(`${SNAP_DIR}/FILE.txt`, `# /work/hello.txt after :w\n${JSON.stringify(fileContent)}\n\n---raw---\n${fileContent}`);
assert.equal(fileContent, "hello world\n");
console.log("FILE /work/hello.txt =", JSON.stringify(fileContent));
await vm.dispose().catch(() => {});
process.exit(0);
