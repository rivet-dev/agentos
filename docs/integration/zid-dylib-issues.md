# Zid × agentOS dylib integration — issue tracker

Living tracker for the Zid team's dylib-preview migration. Covers **their** reported
blockers and **our** (agentOS / secure-exec) bugs surfaced while investigating them.

**Versions under test (their pins):**
- `@rivet-dev/agentos*` / `agentos-core` / `agentos-pi` / `agentos-sidecar`: `0.0.0-integrate-dylib-into-main.815fcda`
- `rivetkit`: `0.0.0-feat-dylib-actor-plugin.c44621f`
- `@agentos-software/common`: `0.3.0-rc.2`; `@secure-exec/core`: `0.3.0`
- Their runtime: Node 22, **linux-x64-gnu prebuilts under OrbStack + Rosetta x86-emulation on macOS** (prod = Railway, native linux-x64).

> Note: the dylib stack lives on the `integrate-dylib-into-main` branch (HEAD = `815fcda`),
> **not** on `main` yet. `815fcda` is what their version is built from.

## How they integrate
They do **not** use the native `agentOs()` actor. They built their own RivetKit JS actor
holding an `AgentOs` core instance (`c.vars.agentOs`) and drive it directly (the removed
`rivetkit/agent-os` actor's replacement). ~12 custom actions, a host toolkit (in-VM Pi
extension → HTTP to host), and server-side `onSessionEvent`.

## Reproduction status (key finding)
Reproduced their exact pattern on **native linux-x64** with their pinned versions, including a
faithful replay of their actor sequence (create → seed writes → their three
`createInMemoryFileSystem` JS-driver mounts → `createSession("pi")` with `cwd:"/workspace"` and
`cwd:"/"`). **Q1–Q4 do NOT reproduce; `createSession("pi")` succeeds end-to-end.** Therefore
their blockers are **environment-specific** — prime suspects: their **custom bundled adapter**
(swapped over the stock `agentos-pi` adapter; the stock adapter works) and **Rosetta
x86-emulation**. Decisive next test for them: run with the **stock adapter** and/or on **native
linux-x64** (e.g. Railway prod), not OrbStack/Rosetta.

Their own repro scripts (`scripts/diag-adapter.mjs`, `scripts/smoke-agentos.mjs`) drive their
rivetkit server; the VM flow they wrap is what was replayed here.

---

## Their reported blockers

| # | Issue | Finding | Status |
|---|---|---|---|
| 1 | **Q0** — native `agentOs()` actor can't host their custom actions / host toolkit / `onSessionEvent`; is wrapping the core class supported? | Yes — core-direct is the documented pattern (all 13 quickstarts). All 4 of their native-actor claims verified TRUE (`actions:{}`, no JS callbacks, callbacks parsed-and-dropped, `toolKits` not serialized). | ✅ Answered |
| 2 | **Q1** — adapter `import @agentclientprotocol/sdk` → `_resolveModule returned non-string` | Not a resolver bug — means "not found in node_modules"; it's their **custom bundled adapter's** node_modules layout. Bundling is a valid fix; else mount the adapter's `node_modules`. | ◐ Diagnosed; error-message fix in **secure-exec PR #114** (diagnostics only) |
| 3 | **Q2** — `chdir` ENOENT for every path incl `/` | Base rootfs **is** provisioned (proven on their version + full flow). Root cause is their custom adapter and/or Rosetta emulation, **not** the SDK. | ✅ Diagnosed (not our bug) |
| 4 | **Q3** — `command not found: sh` | `sh`/`bash` **do** ship (in `@agentos-software/coreutils`, inside `common`); works in repro. Their "common dropped sh" belief is wrong (only the package *description* omits "sh"). | ✅ Diagnosed (not our bug) |
| 5 | **Q4** — host `writeFile`/`mkdir` not visible to guest | Visible core-direct, **no mount required** (proven). Likely write-after-`createSession` ordering / a shadowing mount / env. | ✅ Diagnosed (not our bug) |
| 6 | **Q5** — inherent to the VM model or core-class-specific? | **Neither** — full core-direct path (incl. `createSession("pi")`) reproduced working. | ✅ Answered |

---

## Our bugs / gaps (agentOS + secure-exec)

| # | Issue | Impact | Status | Proposed fix |
|---|---|---|---|---|
| 7 | `toolKit→sidecar` runs Zod **v4** `toJSONSchema()`; throws on Zod **v3** schemas | **Why they dropped `toolKits`** and went host-tools-over-HTTP | 🟡 Open | accept v3 (or convert), or document the constraint |
| 8 | Native actor **silently drops** `onSessionEvent`/`onPermissionRequest`/`onBeforeConnect`/`toolKits` | Silent footgun — users think these are wired | 🟡 Open | throw a clear "unsupported across the native boundary" error (or wire through) |
| 9 | core `AgentOs.create()` ignores the `defaultSoftware` option it documents | Latent; auto-include of `common` is actor-only (`actor.ts:192-200`) | 🟡 Open | honor `defaultSoftware` in core, or fix the JSDoc |
| 10 | `withAutoAgentNodeModulesMount` is **actor-only** — no public helper for core-direct users | Core-direct users with a custom adapter get no node_modules-mount helper (relates to #2) | 🟡 Open | export a public `nodeModulesMount`-style helper / do it in core |
| 11 | Native actor has **no `mountFs`** action and rejects JS-driver mounts (static, serializable Native mounts only) | **Blocks the proxy-actor pattern** from hosting their session/skills JS-driver VFS | 🟡 Open | dynamic `mountFs` (incl. JS-driver) on the native actor |
| 12 | Engine `:6420` vs httpPort `:6421` (`/metadata` 404) | DX confusion they hit | ⚪ Open | document, or client auto-detect |
| 13 | Audit their 11 carried patches for dylib obsolescence — esp. the WASI **read-blocked-as-write** permission typo (their patch 1) | Some patches may be obsolete; the WASI one is a real correctness bug if it survived the move into secure-exec | ⚪ Not audited | audit + confirm against `0.3.0` |
| 14 | Misleading `_resolveModule returned non-string` error (really "not found") | Sent them down the bundling path (#2) | ✅ **secure-exec PR #114 (open)** | reworded + main-only regression test |

**Status key:** ✅ done/answered · ◐ partial · 🟡 our bug, identified, not fixed · ⚪ not started

---

## Reproduction recipe
On native linux-x64 (NOT Rosetta), from public npm:
```
npm i @rivet-dev/agentos-core@0.0.0-integrate-dylib-into-main.815fcda \
      @rivet-dev/agentos-pi@0.0.0-integrate-dylib-into-main.815fcda \
      @agentos-software/common@0.3.0-rc.2
```
```js
import { AgentOs, createInMemoryFileSystem } from "@rivet-dev/agentos-core";
import common from "@agentos-software/common";
import pi from "@rivet-dev/agentos-pi";
const vm = await AgentOs.create({ software: [common, pi] });
console.log((await vm.exec("ls -la / && sh -c 'echo SH_OK' && pwd")).stdout); // base rootfs + sh
await vm.writeFile("/home/user/x.txt", "hi");
console.log((await vm.exec("cat /home/user/x.txt")).stdout);                  // host write visible, no mount
for (const p of ["/home/user/.pi/agent/sessions","/app/skills"]) vm.mountFs(p, createInMemoryFileSystem());
console.log((await vm.createSession("pi", { cwd: "/workspace", env: { HOME:"/home/user" } })).sessionId); // works
await vm.dispose();
```
All of the above succeed → Q1–Q4 are not SDK bugs.
