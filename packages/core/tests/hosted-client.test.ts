import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { describe, expect, test } from "vitest";
import {
	AGENTOS_CONTRACT_HASH,
	createAgentOsClient,
} from "../src/hosted-client.js";
import * as api from "../src/index.js";

describe("hosted client public API", () => {
	test("exports the hosted client used by the checked quickstart", () => {
		expect(api).toHaveProperty("createAgentOsClient");
		expect(Reflect.get(api, "createAgentOsClient")).toBe(createAgentOsClient);
		expect(AGENTOS_CONTRACT_HASH).toMatch(/^sha256:[0-9a-f]{64}$/);
	});

	test("constructs the real synchronous client without making an actor request", async () => {
		const client = createAgentOsClient({
			endpoint: "http://127.0.0.1:1",
			namespace: "default",
			token: "development-only",
			disableMetadataLookup: true,
		});
		try {
			expect(client).not.toBeInstanceOf(Promise);
			const vm = client.agentOS.getOrCreate(["contract-shape"]);
			expect(typeof vm.filesystem.writeFile).toBe("function");
			expect(typeof vm.process.run).toBe("function");
			expect(typeof vm.network.preview.create).toBe("function");
		} finally {
			await client.dispose();
		}
	});

	test("does not load RivetKit when importing the built embedded API", () => {
		const index = fileURLToPath(new URL("../dist/index.js", import.meta.url));
		const loader = `export async function resolve(specifier, context, nextResolve) {
  if (specifier === 'rivetkit' || specifier.startsWith('rivetkit/')) throw Error('eager hosted ESM transport');
  return nextResolve(specifier, context);
}`;
		const result = spawnSync(
			process.execPath,
			[
				"--experimental-loader",
				`data:text/javascript,${encodeURIComponent(loader)}`,
				"--input-type=module",
				"-e",
				`import { createRequire, syncBuiltinESMExports } from 'node:module';
const require = createRequire(import.meta.url);
const Module = require('node:module');
const original = Module._load;
Module._load = function(name, ...args) {
  if (name === 'rivetkit/client' || name === 'rivetkit') throw Error('eager hosted transport');
  return original.call(this, name, ...args);
};
syncBuiltinESMExports();
const api = await import(${JSON.stringify(pathToFileURL(index).href)});
if (typeof api.AgentOs !== 'function' || typeof api.createAgentOsClient !== 'function') throw Error('missing API');`,
			],
			{ timeout: 10_000, maxBuffer: 64 * 1024, encoding: "utf8" },
		);
		expect(result.status, result.error?.message ?? result.stderr).toBe(0);
	});
});
