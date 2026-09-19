import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer as createHostServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { AgentOs } from "@rivet-dev/agentos-core";
import { afterAll, describe, expect, test } from "vitest";
import { z } from "zod";
import {
	hostFunction,
	hostFunctions,
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
	const { createConnection, createServer } = await import("node:net");
	const server = createServer((socket) => socket.end("local"));
	await new Promise((resolve, reject) => {
		server.once("error", reject);
		server.listen(0, "127.0.0.1", resolve);
	});
	const address = server.address();
	const body = await new Promise((resolve, reject) => {
		const chunks = [];
		const socket = createConnection({ host: "127.0.0.1", port: address.port });
		socket.on("data", (chunk) => chunks.push(chunk));
		socket.on("end", () => resolve(Buffer.concat(chunks).toString()));
		socket.on("error", reject);
	});
	await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
	return body;
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

	test("allow VM-local listeners while denying external network by default", async () => {
		const local = await evaluate(listen, bare);
		expect(local).toMatchObject({ outcome: "succeeded", value: "local" });

		const external = await evaluate(
			`fetch("https://api.anthropic.com").then(() => true)`,
			bare,
		);
		expect(external.outcome).toBe("failed");

		// Granting only the network must leave process spawning allowed, or the
		// guest could not run at all.
		const allowed = await evaluate(listen, {
			permissions: { network: "allow" },
			...bare,
		});
		expect(allowed).toMatchObject({ outcome: "succeeded", value: "local" });
	});

	test("start the requested program while denying guest subprocesses", async () => {
		const result = await evaluate<{
			code: string | null;
			status: number | null;
		}>(
			`(async () => {
				const { spawnSync } = await import("node:child_process");
				const child = spawnSync("node", ["-e", "process.exit(0)"]);
				return {
					code: child.error?.code ?? null,
					status: child.status,
				};
			})()`,
			{
				permissions: { childProcess: "deny" },
				...bare,
			},
		);

		expect(result).toMatchObject({
			outcome: "succeeded",
			value: { code: "EACCES", status: 1 },
		});
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
		const math = hostFunctions({
			name: "math",
			description: "Arithmetic on the host.",
			functions: {
				add: hostFunction({
					description: "Add two numbers.",
					inputSchema: z.object({ a: z.number(), b: z.number() }),
					execute: ({ a, b }) => a + b,
				}),
			},
		});
		// Each collection is a guest global, and each function an async function.
		const result = await evaluate<number>("math.add({ a: 40, b: 2 })", {
			hostFunctions: [math],
			...bare,
		});
		expect(result).toMatchObject({ outcome: "succeeded", value: 42 });
	});
});

describe("createVm", () => {
	test("returns an agentOS VM with the secure permission defaults", async () => {
		const vm = await createVm(bare);
		try {
			const local = await vm.javascript.evaluate(listen);
			expect(local).toMatchObject({ outcome: "succeeded", value: "local" });
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

	test("keeps guest listeners off the host network", async () => {
		const hostServer = createHostServer((_request, response) =>
			response.end("host"),
		);
		await new Promise<void>((resolve, reject) => {
			hostServer.once("error", reject);
			hostServer.listen(0, "127.0.0.1", resolve);
		});
		const address = hostServer.address();
		if (!address || typeof address === "string") {
			throw new Error("expected a TCP host listener");
		}

		let vm: Awaited<ReturnType<typeof createVm>> | undefined;
		try {
			vm = await createVm(bare);
			const ready = Promise.withResolvers<void>();
			const decoder = new TextDecoder();
			const server = await vm.javascript.spawn(
				`
				import { createServer } from "node:http";
				createServer((_request, response) => response.end("guest"))
					.listen(${address.port}, "127.0.0.1", () => console.log("listening"));
				`,
				{
					onStdout: (chunk) => {
						if (decoder.decode(chunk).includes("listening")) ready.resolve();
					},
				},
			);
			await ready.promise;

			const [hostBody, guestResponse] = await Promise.all([
				fetch(`http://127.0.0.1:${address.port}`).then((response) =>
					response.text(),
				),
				vm.network.httpRequest({ port: address.port, path: "/" }),
			]);
			expect(hostBody).toBe("host");
			expect(new TextDecoder().decode(guestResponse.body)).toBe("guest");
			await vm.process.kill(server.pid);
		} finally {
			await vm?.dispose();
			await new Promise<void>((resolve, reject) => {
				hostServer.close((error) => (error ? reject(error) : resolve()));
			});
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
