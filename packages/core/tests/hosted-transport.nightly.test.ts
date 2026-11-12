import { randomUUID } from "node:crypto";
import { describe, expect, test } from "vitest";
import { createAgentOsClient } from "../src/hosted-client.js";

// This target requires an explicitly owned local control plane and hosted worker.
// The fixture owner tears down that worker and its temporary state after the run.
describe.runIf(process.env.AGENTOS_HOSTED_TEST_REQUIRE_LOCAL === "1")(
	"native hosted TypeScript transport",
	() => {
		test("preserves binary data, exact integers, typed errors, and desired revisions", async () => {
			const endpoint = process.env.AGENTOS_HOSTED_TEST_ENDPOINT;
			const namespace = process.env.AGENTOS_HOSTED_TEST_NAMESPACE;
			const token = process.env.AGENTOS_HOSTED_TEST_TOKEN;
			if (!endpoint || !namespace || !token) {
				throw new Error(
					"local hosted test requires endpoint, namespace, and token",
				);
			}
			const client = createAgentOsClient({ endpoint, namespace, token });
			try {
				const vm = client.agentOS.getOrCreate([`transport-ts-${randomUUID()}`]);
				const content = new Uint8Array([0, 255, 128, 65, 10]);
				await vm.filesystem.writeFile({ path: "/tmp/transport.bin", content });
				expect(
					await vm.filesystem.readFile({ path: "/tmp/transport.bin" }),
				).toEqual(content);
				const before = await vm.config.get({});
				const after = await vm.config.patch({
					patch: { environment: { HOSTED_TRANSPORT_CONTROL: "checked" } },
					expectedRevision: before.revision,
				});
				expect(BigInt(after.revision)).toBe(BigInt(before.revision) + 1n);
				await expect(
					vm.config.patch({
						patch: {},
						expectedRevision: 9_007_199_254_740_993n,
					}),
				).rejects.toMatchObject({ code: "revision_conflict" });
				await expect(
					vm.process.output.read({
						process: { generation: 9_007_199_254_740_993n, pid: 1 },
						after: 18_446_744_073_709_551_615n,
					}),
				).rejects.toMatchObject({ code: "stale_generation" });
				for (const patch of [
					{ sidecarBinaryPath: "/not-a-host-input" },
					{ bindings: { synthetic: {} } },
					{ software: [{ path: "/not-a-host-package" }] },
				]) {
					await expect(vm.config.patch({ patch })).rejects.toMatchObject({
						code: "invalid_input",
					});
				}
				const output = await vm.process.run({
					command: "node",
					args: ["-e", "console.log('hosted-native-control')"],
					options: { timeoutMs: 10_000, captureStdio: true },
				});
				expect(output.status.exitCode).toBe(0);
				expect(output.stdout).toBe("hosted-native-control\n");
			} finally {
				await client.dispose();
			}
		}, 90_000);
	},
);
