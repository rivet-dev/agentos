import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Registry } from "@rivet-dev/agentos";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import {
	ACTOR_E2E_CONN_PARAMS,
	ACTOR_E2E_NAMESPACE,
	ACTOR_E2E_POOL_NAME,
	ACTOR_E2E_TOKEN,
	type ActorRuntimeHandle,
	startActorRuntime,
} from "../../agentos/tests/helpers/actor-runtime.js";
import { agentOSSandbox } from "../src/index.js";

const RUN_E2E = process.env.AGENTOS_ACTOR_E2E === "1";

describe.skipIf(!RUN_E2E)("agentOSSandbox real Rivet Actor", () => {
	let runtime: ActorRuntimeHandle;
	let storagePath: string;

	beforeAll(async () => {
		storagePath = mkdtempSync(join(tmpdir(), "agentos-flue-actor-e2e-"));
		runtime = await startActorRuntime(storagePath);
	}, 120_000);

	afterAll(async () => {
		await runtime?.stop();
		if (storagePath) rmSync(storagePath, { recursive: true, force: true });
	}, 30_000);

	it("drives filesystem and exec through a real agentOS actor", async () => {
		const registry = {
			startAndWait: async () => {},
			parseConfig: () => ({
				endpoint: runtime.endpoint,
				namespace: ACTOR_E2E_NAMESPACE,
				token: ACTOR_E2E_TOKEN,
				headers: {},
				envoy: { poolName: ACTOR_E2E_POOL_NAME },
			}),
			config: { use: { vm: {} } },
		} as unknown as Registry<any>;
		const sandbox = agentOSSandbox({
			actor: "vm",
			registry,
			params: ACTOR_E2E_CONN_PARAMS,
		});
		const env = await sandbox.createSessionEnv({
			id: `flue-e2e-${Date.now()}`,
		});

		await env.writeFile("from-flue.txt", "real-actor");
		await expect(env.readFile("from-flue.txt")).resolves.toBe("real-actor");
		await expect(env.exec("printf actor-exec")).resolves.toMatchObject({
			exitCode: 0,
			stdout: "actor-exec",
			stderr: "",
		});
	}, 120_000);
});
