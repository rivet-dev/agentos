import { randomUUID } from "node:crypto";
import { expect, test } from "vitest";
import { destroyActor, localClient } from "./hosted-local-fixture.js";

test.runIf(process.env.AGENTOS_HOSTED_TEST_REQUIRE_LOCAL === "1")(
  "public TS generic coordinator handle retains typed preload policy errors and state",
  async () => {
    const client = localClient();
    const failures: unknown[] = [];
    let id: string | undefined;
    try {
      // This is the shipped AgentOsClient's inherited public generic method,
      // not a fabricated coordinator entry in the generated agentOS registry.
      const coordinator = client.getOrCreate("agentOS-preload-coordinator", ["preload-client-ts", randomUUID()], { createWithInput: {} });
      id = await coordinator.resolve();
      const before = await coordinator.action({ name: "preload.status", args: [null] });
      const url = `https://packages.gameinc.io/${"x".repeat(4096)}.aospkg`;
      let rejected: unknown;
      try {
        await coordinator.action({ name: "preload.replaceBaseline", args: [{ protocolVersion: 1, artifacts: [{ url, digest: `sha256:${"a".repeat(64)}`, size: 1, packageFormatVersion: 1 }] }] });
      } catch (error) { rejected = error; }
      expect(rejected).toMatchObject({ group: "user", code: "limit_exceeded", metadata: { budget: {
        setting: "preloadPolicy.maxUrlBytes", observed: Buffer.byteLength(url), limit: 4096, units: "bytes", remediation: expect.stringContaining("--preload-policy-json"),
      } } });
      const metadata = (rejected as { metadata: { budget: object } }).metadata;
      expect(Object.keys(metadata)).toEqual(["budget"]);
      expect(Object.keys(metadata.budget).sort()).toEqual(["limit", "observed", "remediation", "setting", "units"]);
      expect(await coordinator.action({ name: "preload.status", args: [null] })).toEqual(before);
    } catch (error) { failures.push(error); }
    finally {
      if (id) try { await destroyActor(id); } catch (error) { failures.push(error); }
      try { await client.dispose(); } catch (error) { failures.push(error); }
    }
    if (failures.length) throw new AggregateError(failures, "public TS preload policy/cleanup");
  }, 90_000,
);
