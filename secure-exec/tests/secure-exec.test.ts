import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { AgentOs } from "@rivet-dev/agentos-core";
import { afterAll, describe, expect, test } from "vitest";
import { z } from "zod";
import {
	binding,
	bindings,
	createVm,
	evaluate,
	execute,
	executeFile,
	hostDirMount,
	init,
	SidecarRejectedError,
	shutdown,
} from "../src/index.js";
import { check, evaluate as evaluateTypeScript } from "../src/typescript.js";

// Like the core PR suite, skip the default software bundle: these tests run
// JavaScript only, and CI does not assemble the bundle's WASM packages.
const bare = { defaultSoftware: false } as const;

// The shared sidecar keeps piped stdio open, which blocks the vitest worker
// from exiting unless it is shut down.
afterAll(shutdown);

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

describe("one-shot calls", () => {
	test("init starts the shared sidecar", async () => {
		await init();
		const sidecar = await AgentOs.getSharedSidecar();
		expect(sidecar.describe()).toMatchObject({
			state: "ready",
			activeVmCount: 0,
		});
	});

	test("run each call in its own VM and dispose it", async () => {
		await execute("globalThis.leaked = true", bare);
		const result = await evaluate<string>("typeof globalThis.leaked", bare);
		expect(result).toMatchObject({ outcome: "succeeded", value: "undefined" });

		const sidecar = await AgentOs.getSharedSidecar();
		expect(sidecar.describe().activeVmCount).toBe(0);
	});

	test("forward execution options and return guest failures", async () => {
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

	test("deny the network by default and merge a partial policy", async () => {
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

	test("run and type-check TypeScript", async () => {
		const value = await evaluateTypeScript<number>("(40 as number) + 2", bare);
		expect(value).toMatchObject({ outcome: "succeeded", value: 42 });

		const checked = await check(`const total: number = "nope";`, bare);
		expect(checked).toMatchObject({ outcome: "succeeded", hasErrors: true });
		expect(checked.diagnostics[0]).toMatchObject({ code: 2322 });
	});

	test("run a mounted file", async () => {
		const hostDir = mkdtempSync(join(tmpdir(), "secure-exec-files-"));
		try {
			writeFileSync(join(hostDir, "main.mjs"), 'console.log("from a mount");');
			const result = await executeFile("/mnt/app/main.mjs", {
				mounts: [hostDirMount("/mnt/app", hostDir)],
				output: { capture: "all" },
				...bare,
			});
			expect(result).toMatchObject({
				outcome: "succeeded",
				stdout: "from a mount\n",
			});
		} finally {
			rmSync(hostDir, { recursive: true, force: true });
		}
	});

	test("call host functions as guest globals", async () => {
		const math = bindings({
			name: "math",
			description: "Arithmetic on the host.",
			bindings: {
				add: binding({
					description: "Add two numbers.",
					inputSchema: z.object({ a: z.number(), b: z.number() }),
					execute: ({ a, b }) => a + b,
				}),
			},
		});
		// Each collection is a guest global, and each binding an async function.
		const result = await evaluate<number>("math.add({ a: 40, b: 2 })", {
			bindings: [math],
			...bare,
		});
		expect(result).toMatchObject({ outcome: "succeeded", value: 42 });
	});
});

describe("createVm", () => {
	test("returns an agentOS VM with the secure permission defaults", async () => {
		const vm = await createVm(bare);
		try {
			const denied = await vm.javascript.evaluate(listen);
			expect(denied.outcome).toBe("failed");
		} finally {
			await vm.dispose();
		}
	});

	test("keeps files across calls and exchanges them with the host", async () => {
		const vm = await createVm(bare);
		try {
			await vm.filesystem.writeFile("/workspace/in.txt", "from the host");
			await vm.javascript.execute(`
				import { readFileSync, writeFileSync } from "node:fs";
				writeFileSync("/workspace/out.txt", readFileSync("/workspace/in.txt", "utf8").toUpperCase());
			`);
			const out = await vm.filesystem.readFile("/workspace/out.txt");
			expect(new TextDecoder().decode(out)).toBe("FROM THE HOST");
		} finally {
			await vm.dispose();
		}
	});

	test("runs a spawned server that the host can call", async () => {
		const vm = await createVm({ permissions: { network: "allow" }, ...bare });
		try {
			const ready = Promise.withResolvers<void>();
			const decoder = new TextDecoder();
			const server = await vm.javascript.spawn(
				`
				import { createServer } from "node:http";
				createServer((request, response) => response.end("hello"))
					.listen(3000, () => console.log("listening"));
				`,
				{
					onStdout: (chunk) => {
						if (decoder.decode(chunk).includes("listening")) ready.resolve();
					},
				},
			);
			await ready.promise;
			const response = await vm.network.httpRequest({ port: 3000, path: "/" });
			const body = new TextDecoder().decode(response.body);
			expect(body).toBe("hello");
			await vm.process.kill(server.pid);
		} finally {
			await vm.dispose();
		}
	});
});

describe("vm.createContext", () => {
	test("retains state, resets it, and deletes only the context", async () => {
		const vm = await createVm(bare);
		try {
			const context = await vm.createContext();
			await context.execute("globalThis.answer = 40");
			expect(await context.evaluate<number>("answer + 2")).toMatchObject({
				outcome: "succeeded",
				value: 42,
			});

			// TypeScript shares the context's state through its id.
			const typed = await vm.typescript.evaluate<number>(
				"(globalThis as unknown as { answer: number }).answer",
				{ contextId: context.contextId },
			);
			expect(typed).toMatchObject({ outcome: "succeeded", value: 40 });

			await context.reset();
			expect(
				await context.evaluate<string>("typeof globalThis.answer"),
			).toMatchObject({ outcome: "succeeded", value: "undefined" });

			// Disposing the context deletes it and leaves the VM running.
			await context.dispose();
			await expect(context.evaluate("1")).rejects.toMatchObject({
				detail: { code: "context_not_found" },
			});
			expect(await vm.javascript.evaluate<number>("1 + 1")).toMatchObject({
				outcome: "succeeded",
				value: 2,
			});
		} finally {
			await vm.dispose();
		}
	});

	test("runs contexts in one VM in parallel with separate state", async () => {
		const vm = await createVm(bare);
		try {
			const contexts = await Promise.all([
				vm.createContext(),
				vm.createContext(),
				vm.createContext(),
			]);
			const results = await Promise.all(
				contexts.map((context, index) =>
					context.evaluate<number>(
						"new Promise((resolve) => setTimeout(() => resolve((globalThis.id = inputs.id)), 300))",
						{ inputs: { id: index } },
					),
				),
			);
			expect(
				results.map((result) => "value" in result && result.value),
			).toEqual([0, 1, 2]);
		} finally {
			await vm.dispose();
		}
	});

	test("rejects a second call on a busy context with a typed error", async () => {
		const vm = await createVm(bare);
		try {
			const context = await vm.createContext();
			const slow = context.evaluate(
				"new Promise((resolve) => setTimeout(() => resolve(1), 500))",
			);
			const rejection = await context.evaluate("2").catch((error) => error);
			expect(rejection).toBeInstanceOf(SidecarRejectedError);
			expect(rejection.detail.code).toBe("execution_busy");
			expect(await slow).toMatchObject({ outcome: "succeeded", value: 1 });
		} finally {
			await vm.dispose();
		}
	});
});

describe("shutdown", () => {
	test("stops the sidecar, and the next call starts a new one", async () => {
		await shutdown();
		const result = await evaluate<number>("1 + 2", bare);
		expect(result).toMatchObject({ outcome: "succeeded", value: 3 });
	});
});
