import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { dirname, delimiter, resolve, join } from "node:path";
import { afterAll, beforeAll, expect, test } from "vitest";
import { z } from "zod";
import { AgentOs } from "../src/agent-os.js";

let artifact: string;
let bundle: Buffer;
let ownedDirectory: string | undefined;
beforeAll(() => {
  let prepared = process.env.AGENTOS_CHILD_COMPLETION_FIXTURES;
  if (!prepared) {
    ownedDirectory = mkdtempSync(join(tmpdir(), "agentos-child-completion-"));
    prepared = join(ownedDirectory, "prepared");
    execFileSync(process.execPath, [fileURLToPath(new URL("../scripts/prepare-child-completion-fixtures.mjs", import.meta.url)), prepared], {
      timeout: 30_000, maxBuffer: 4 * 1024 * 1024,
      env: { ...process.env, ESBUILD_WORKER_THREADS: "0" },
    });
  }
  artifact = resolve(prepared, "effect-probe.mjs");
  const receipt = JSON.parse(readFileSync(resolve(prepared, "receipt.json"), "utf8"));
  bundle = readFileSync(artifact);
  if (createHash("sha256").update(bundle).digest("hex") !== receipt.sha256) throw new Error("prepared fixture changed");
}, 35_000);
afterAll(() => { if (ownedDirectory) rmSync(ownedDirectory, { recursive: true, force: true }); });

for (const mode of ["effect", "effect-async", "direct", "cancel", "failed-spawn", "late-guest", "handled-spawn-throw"]) {
  test(`child completion preserves real Effect and child ownership: ${mode}`, async () => {
    const native = mode === "late-guest" ? undefined : JSON.parse(execFileSync(process.execPath, [artifact, mode], {
      encoding: "utf8", timeout: 20_000, maxBuffer: 4 * 1024 * 1024,
      env: { ...process.env, PATH: dirname(process.execPath) + delimiter + (process.env.PATH ?? "") },
    }));
    const failures: unknown[] = [];
    let vm: AgentOs | undefined;
    const sidecar = await AgentOs.createSidecar();
    try {
      vm = await AgentOs.create({ defaultSoftware: false, sidecar: { kind: "explicit", handle: sidecar },
        permissions: { fs: "allow", network: "deny", childProcess: "allow", process: "allow", env: "allow", hostFunction: "deny" },
      });
      await vm.writeFile("/tmp/child-completion-probe.mjs", bundle);
      const result = await vm.execArgv("node", ["/tmp/child-completion-probe.mjs", mode], { timeout: 20_000 });
      expect(result.exitCode, result.stderr).toBe(0);
      const guest = JSON.parse(result.stdout);
      if (mode === "direct") {
        const { trace: nativeTrace, ...nativeValue } = native;
        const { trace: guestTrace, ...guestValue } = guest;
        expect(guestValue).toEqual(nativeValue);
        // The probe validates portable spawn/EOF/exit/close partial orders.
        // Arbitrary async consumer commits are awaited by their owning result.
        expect(guestTrace.includes("spawn")).toBe(true);
        expect(nativeTrace.includes("spawn")).toBe(true);
      } else if (mode !== "late-guest") expect(guest).toEqual(native);
      const processes = await vm.process.list();
      expect(processes.filter((process) => process.state === "running")).toEqual([]);
    } catch (error) {
      failures.push(error);
    } finally {
      if (vm) { try { await vm.dispose(); } catch (error) { failures.push(error); } }
      try { await sidecar.dispose(); } catch (error) { failures.push(error); }
    }
    if (failures.length) throw new AggregateError(failures, `child completion failure/cleanup: ${mode}`);
  }, 60_000);
}


test("real host callback results survive child close and scheduled guest continuation", async () => {
  const failures: unknown[] = [];
  let vm: AgentOs | undefined;
  const sidecar = await AgentOs.createSidecar();
  try {
    vm = await AgentOs.create({ defaultSoftware: false, sidecar: { kind: "explicit", handle: sidecar },
      permissions: { fs: "allow", network: "deny", childProcess: "allow", process: "allow", env: "allow", hostFunction: "allow" },
      hostFunctions: { bridge: { ping: { inputSchema: z.object({ value: z.number() }), execute: (input: { value: number }) => ({ value: input.value + 1 }) } } },
    });
    const source = `
      const rows=[];
      for(let i=0;i<4;i++) {
        const result=await bridge.ping({value:i});
        await new Promise(resolve=>setImmediate(()=>Promise.resolve().then(()=>setImmediate(resolve))));
        if(result.value!==i+1)throw new Error("wrong host callback owner/result");
        rows.push(result.value);
      }
      process.stdout.write(JSON.stringify(rows));
    `;
    const result = await vm.execArgv("node", ["--input-type=module", "-e", source], { timeout: 20_000 });
    expect(result.exitCode, result.stderr).toBe(0);
    expect(JSON.parse(result.stdout)).toEqual([1, 2, 3, 4]);
    expect((await vm.process.list()).filter((process) => process.state === "running")).toEqual([]);
  } catch (error) { failures.push(error); }
  finally {
    if (vm) { try { await vm.dispose(); } catch (error) { failures.push(error); } }
    try { await sidecar.dispose(); } catch (error) { failures.push(error); }
  }
  if (failures.length) throw new AggregateError(failures, "host callback completion/cleanup");
}, 60_000);
