import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, delimiter } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";

for (const mode of ["cjs", "mjs"]) {
  test(`canonical timers preserve actual module owners: ${mode}`, async () => {
    const artifact = fileURLToPath(new URL(`./fixtures/timers/canonical-timers.${mode}`, import.meta.url));
    const native = JSON.parse(execFileSync(process.execPath, [artifact], { encoding: "utf8", timeout: 20_000, maxBuffer: 65_536,
      env: { ...process.env, PATH: dirname(process.execPath) + delimiter + (process.env.PATH ?? "") } }));
    const failures: unknown[] = [];
    let vm: AgentOs | undefined;
    let sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>> | undefined;
    try {
      sidecar = await AgentOs.createSidecar();
      vm = await AgentOs.create({ defaultSoftware: false, sidecar: { kind: "explicit", handle: sidecar }, permissions: { fs: "allow", network: "deny", childProcess: "deny", process: "allow", env: "allow", hostFunction: "deny" } });
      const path = `/tmp/canonical-timers.${mode}`;
      await vm.writeFile(path, readFileSync(artifact));
      const result = await vm.execArgv("node", [path], { timeout: 20_000 });
      expect(result.exitCode, result.stderr).toBe(0);
      const guest = JSON.parse(result.stdout);
      expect(guest.same).toEqual(native.same);
      expect(guest.stableAfterGlobalReplacement).toBe(true);
      expect(guest.promiseErrors).toEqual([]);
      expect([...guest.trace].sort()).toEqual([...native.trace].sort());
    } catch (error) { failures.push(error); }
    finally {
      if (vm) try { await vm.dispose(); } catch (error) { failures.push(error); }
      if (sidecar) try { await sidecar.dispose(); } catch (error) { failures.push(error); }
    }
    if (failures.length) throw new AggregateError(failures, `canonical timer execution/cleanup: ${mode}`);
  }, 60_000);
}
