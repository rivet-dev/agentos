import { AgentOs } from "@rivet-dev/agentos-core";
import { afterAll, describe, expect, test } from "vitest";
import { createContext, evaluate, execute, init } from "../src/index.js";
import { install } from "../src/npm.js";
import {
	check,
	evaluate as evaluateTypeScript,
	execute as executeTypeScript,
} from "../src/typescript.js";

// Like the core PR suite, skip the default software bundle: these tests run
// JavaScript only, and CI does not assemble the bundle's WASM packages.
const bare = { defaultSoftware: false } as const;

// The shared sidecar keeps piped stdio open, which blocks the vitest worker
// from exiting unless it is disposed.
afterAll(async () => {
	await (await AgentOs.getSharedSidecar()).dispose();
});

describe("secure-exec", () => {
	test("init starts the shared sidecar", async () => {
		await init();
		const sidecar = await AgentOs.getSharedSidecar();
		expect(sidecar.describe()).toMatchObject({
			state: "ready",
			activeVmCount: 0,
		});
	});

	test("runs each call in its own VM", async () => {
		await execute("globalThis.leaked = true", bare);
		const result = await evaluate<string>("typeof globalThis.leaked", bare);
		expect(result).toMatchObject({ outcome: "succeeded", value: "undefined" });

		const sidecar = await AgentOs.getSharedSidecar();
		expect(sidecar.describe().activeVmCount).toBe(0);
	});

	test("forwards execution options and returns guest failures", async () => {
		const result = await evaluate<number>("inputs.a + inputs.b", {
			inputs: { a: 40, b: 2 },
			...bare,
		});
		expect(result).toMatchObject({ outcome: "succeeded", value: 42 });

		const output = await execute(`console.log("hello")`, {
			output: { capture: "all" },
			...bare,
		});
		expect(output).toMatchObject({ outcome: "succeeded", stdout: "hello\n" });

		const failed = await evaluate(`JSON.parse("not json")`, bare);
		expect(failed.outcome).toBe("failed");

		const timedOut = await execute("while (true) {}", {
			timeoutMs: 500,
			...bare,
		});
		expect(timedOut.outcome).toBe("timed_out");
	});

	test("denies the network by default and merges a partial policy", async () => {
		const listen = `(async () => {
			const { createServer } = await import("node:net");
			const server = createServer();
			await new Promise((resolve, reject) => {
				server.once("error", reject);
				server.listen(0, "127.0.0.1", resolve);
			});
			server.close();
			return true;
		})()`;

		const denied = await evaluate(listen, bare);
		expect(denied.outcome).toBe("failed");

		// Granting only the network must leave process spawning allowed, or the
		// guest could not run at all.
		const allowed = await evaluate(listen, {
			permissions: { network: "allow" },
			...bare,
		});
		expect(allowed).toMatchObject({ outcome: "succeeded", value: true });
	});

	test("retains state in a context shared by JavaScript and TypeScript", async () => {
		await using context = await createContext(bare);

		await executeTypeScript("globalThis.answer = 40 as number", { context });
		const retained = await evaluate<number>("globalThis.answer + 2", {
			context,
		});
		expect(retained).toMatchObject({ outcome: "succeeded", value: 42 });

		const typed = await evaluateTypeScript<number>(
			"(globalThis as unknown as { answer: number }).answer",
			{ context },
		);
		expect(typed).toMatchObject({ outcome: "succeeded", value: 40 });

		await context.reset();
		const reset = await evaluate<string>("typeof globalThis.answer", {
			context,
		});
		expect(reset).toMatchObject({ outcome: "succeeded", value: "undefined" });
	});

	test("disposing a context disposes its VM", async () => {
		const sidecar = await AgentOs.getSharedSidecar();
		const context = await createContext(bare);
		expect(sidecar.describe().activeVmCount).toBe(1);
		await context.dispose();
		expect(sidecar.describe().activeVmCount).toBe(0);
	});

	test("type-checks TypeScript", async () => {
		const result = await check(`const total: number = "nope";`, bare);
		expect(result).toMatchObject({ outcome: "succeeded", hasErrors: true });
		expect(result.diagnostics[0]).toMatchObject({ code: 2322 });
	});

	test("rejects VM options combined with a context", async () => {
		await using context = await createContext(bare);
		await expect(
			// @ts-expect-error VM options cannot be combined with a context.
			evaluate("1", { context, permissions: { network: "allow" } }),
		).rejects.toThrow(
			"permissions cannot be combined with context; pass VM options to createContext()",
		);
	});

	test("requires a context for npm operations", () => {
		// @ts-expect-error npm operations require a context.
		expect(() => install(["zod"], {})).toThrow(
			"npm operations require a context",
		);
	});
});
