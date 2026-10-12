import { describe, expect, test } from "vitest";
import {
	actorPath,
	destroyActor,
	engineRequest,
	eventually,
	localClient,
	newVm,
	softwareSources,
} from "./hosted-local-fixture.js";

describe.runIf(process.env.AGENTOS_HOSTED_TEST_REQUIRE_LOCAL === "1")(
	"owned native hosted lifecycle",
	() => {
		test("retained language state survives calls and resets explicitly", async () => {
			const client = localClient();
			const vm = newVm(client);
			let id: string | undefined;
			try {
				id = await vm.resolve();
				const context = await vm.contexts.create({
					contextId: "retained-control",
				});
				const options = {
					inline: {
						process: {
							context: context.context,
							timeoutMs: 10_000,
							output: { capture: "all" as const },
						},
					},
				};
				expect(
					(
						await vm.javascript.execute({
							source: "globalThis.__hostedValue = 41",
							options,
						})
					).error,
				).toBeNull();
				expect(
					(
						await vm.javascript.evaluate({
							expression: "globalThis.__hostedValue + 1",
							options,
						})
					).value,
				).toBe(42);
				await vm.contexts.reset({ context: context.context });
				expect(
					(
						await vm.javascript.evaluate({
							expression: "typeof globalThis.__hostedValue",
							options,
						})
					).value,
				).toBe("undefined");
				await vm.contexts.delete({ context: context.context });
			} finally {
				try {
					if (id) await destroyActor(id);
				} finally {
					await client.dispose();
				}
			}
		}, 90_000);

		test("streams binary output and explicitly cancels a running guest process", async () => {
			const client = localClient();
			const vm = newVm(client);
			let id: string | undefined;
			try {
				id = await vm.resolve();
				const process = await vm.process.spawn({
					command: "node",
					args: [
						"-e",
						"process.stdout.write(Buffer.from([0,255,128]));process.stderr.write(Buffer.from([128,255,0]));setInterval(()=>{},1000)",
					],
				});
				const output = await eventually(
					() =>
						vm.process.output.read({ process, maxEvents: 16, maxBytes: 1024 }),
					(page) =>
						page.events.some((event) => event.stream === "stdout") &&
						page.events.some((event) => event.stream === "stderr"),
				);
				expect(
					Buffer.concat(
						output.events
							.filter((event) => event.stream === "stdout")
							.map((event) => event.data),
					),
				).toEqual(Buffer.from([0, 255, 128]));
				expect(
					Buffer.concat(
						output.events
							.filter((event) => event.stream === "stderr")
							.map((event) => event.data),
					),
				).toEqual(Buffer.from([128, 255, 0]));
				await vm.process.signal({ process, signal: "SIGTERM" });
				expect((await vm.process.wait({ process })).status.signal).toBe(
					"SIGTERM",
				);
				expect((await vm.process.get({ process })).running).toBe(false);
			} finally {
				try {
					if (id) await destroyActor(id);
				} finally {
					await client.dispose();
				}
			}
		}, 90_000);

		test("observed process status agrees across hosted events, waits and replay", async () => {
			const client = localClient();
			const vm = newVm(client);
			const connection = vm.connect();
			let id: string | undefined;
			const events: Array<{
				process: { pid: number };
				status: { exitCode: number; signal?: string | null };
			}> = [];
			try {
				id = await vm.resolve();
				await connection.vm.status({});
				connection.on("process.exit", (event) => events.push(event));
				for (const [kind, code, signal] of [
					["signal", 143, "SIGTERM"],
					["normal143", 143, undefined],
					["caught", 0, undefined],
				] as const) {
					const source =
						kind === "normal143"
							? "process.exit(143)"
							: `${kind === "caught" ? "process.on('SIGTERM', () => process.exit(0));" : ""}console.log('ready');setInterval(()=>{},1000);`;
					const process = await connection.process.spawn({
						command: "node",
						args: ["-e", source],
					});
					if (kind !== "normal143") {
						await eventually(
							() =>
								vm.process.output.read({
									process,
									maxEvents: 16,
									maxBytes: 1024,
								}),
							(page) =>
								Buffer.concat(page.events.map((event) => event.data))
									.toString("utf8")
									.includes("ready"),
						);
						await vm.process.signal({ process, signal: "SIGTERM" });
					}
					const exit = await vm.process.wait({ process });
					expect(exit.status.exitCode).toBe(code);
					expect(exit.status.signal ?? undefined).toBe(signal);
					const live = await eventually(
						async () =>
							events.find((event) => event.process.pid === process.pid),
						(event) => event !== undefined,
					);
					expect(live?.status).toEqual(exit.status);
					expect((await vm.process.wait({ process })).status).toEqual(
						exit.status,
					);
					expect((await vm.process.get({ process })).exit).toEqual(exit.status);
					expect((await vm.process.output.read({ process })).end).toEqual(
						exit.status,
					);
				}
			} finally {
				try {
					await connection.dispose();
				} finally {
					try {
						if (id) await destroyActor(id);
					} finally {
						await client.dispose();
					}
				}
			}
		}, 90_000);

		test("restart and sleep preserve an explicitly durable mount; destroy ends the actor", async () => {
			const client = localClient();
			const vm = newVm(client);
			let id: string | undefined;
			try {
				id = await vm.resolve();
				const bytes = new Uint8Array([0, 255, 128, 42]);
				await vm.filesystem.writeFile({
					path: "/workspace/durable.bin",
					content: bytes,
				});
				const before = await vm.vm.status({});
				const restarted = await vm.vm.restart({});
				expect(BigInt(restarted.generation)).toBeGreaterThan(
					BigInt(before.generation),
				);
				expect(
					await vm.filesystem.readFile({ path: "/workspace/durable.bin" }),
				).toEqual(bytes);
				const sleep = await engineRequest(actorPath(id, "/sleep"), "POST", {});
				expect(sleep.ok).toBe(true);
				const rebound = client.getForId("agentOS", id);
				await eventually(
					() => rebound.vm.status({}),
					(value) => BigInt(value.generation) > BigInt(restarted.generation),
					20_000,
				);
				expect(
					await rebound.filesystem.readFile({ path: "/workspace/durable.bin" }),
				).toEqual(bytes);
				expect(
					BigInt((await rebound.vm.status({})).generation),
				).toBeGreaterThan(BigInt(restarted.generation));
				await destroyActor(id);
				const deleted = id;
				id = undefined;
				await eventually(
					() =>
						engineRequest(
							`/actors?namespace=${encodeURIComponent(process.env.AGENTOS_HOSTED_TEST_NAMESPACE ?? "")}&actor_id=${encodeURIComponent(deleted)}&include_destroyed=true`,
						),
					(value) => value.ok && value.body.actors[0]?.destroy_ts != null,
				);
			} finally {
				try {
					if (id) await destroyActor(id);
				} finally {
					await client.dispose();
				}
			}
		}, 120_000);

		test("creates, reads, cancels previews and retains then cancels a durable cron command", async () => {
			const client = localClient();
			const vm = newVm(client);
			let id: string | undefined;
			try {
				id = await vm.resolve();
				const process = await vm.process.spawn({
					command: "node",
					args: [
						"-e",
						"require('node:http').createServer((_,r)=>r.end('hosted-preview-control')).listen(3000,()=>require('node:fs').writeFileSync('/workspace/preview-ready','1'))",
					],
				});
				await eventually(
					() => vm.filesystem.exists({ path: "/workspace/preview-ready" }),
					(ready) => ready,
				);
				const streamed = await vm.network.fetchStream.start({
					request: { port: 3000, path: "/" },
				});
				const part = await vm.network.fetchStream.read({
					stream: streamed.stream,
					maxBytes: 4,
				});
				expect(Buffer.from(part.body)).toEqual(Buffer.from("host"));
				await vm.network.fetchStream.cancel({ stream: streamed.stream });
				const lease = await vm.network.preview.create({
					port: 3000,
					ttlMs: 60_000,
				});
				const gateway = await vm.getGatewayUrl();
				const preview = new URL(gateway);
				// Raw actor requests use RivetKit's /request endpoint; preserve the Gateway query token.
				preview.pathname = `${preview.pathname.replace(/\/$/, "")}/request${lease.path}`;
				const response = await fetch(preview, {
					redirect: "error",
					signal: AbortSignal.timeout(5_000),
				});
				expect(response.status).toBe(200);
				expect(await response.text()).toBe("hosted-preview-control");
				await vm.network.preview.expire({ token: lease.token });
				expect(
					(
						await fetch(preview, {
							redirect: "error",
							signal: AbortSignal.timeout(5_000),
						})
					).status,
				).toBe(404);
				await vm.process.signal({ process, signal: "SIGTERM" });
				await vm.process.wait({ process });
				const job = await vm.cron.schedule({
					name: "durable-control",
					expression: "0 0 1 1 *",
					command: "node",
					args: ["-e", "process.exit(0)"],
				});
				expect(job.name).toBe("durable-control");
				await vm.vm.restart({});
				expect((await vm.cron.list({})).map((value) => value.name)).toContain(
					"durable-control",
				);
				expect(await vm.cron.cancel({ name: "durable-control" })).toBe(true);
				const source = softwareSources()[0];
				const installed = (await vm.software.list({})).find(
					(value) => value.digest === source.digest,
				);
				expect(installed).toBeDefined();
				if (!installed) throw Error("fixture package was not projected");
				await vm.software.uninstall({ packageId: installed.packageId });
				expect(
					(await vm.software.list({})).some(
						(value) => value.digest === source.digest,
					),
				).toBe(false);
				await vm.software.install({ source });
				expect(
					(await vm.software.list({})).some(
						(value) => value.digest === source.digest,
					),
				).toBe(true);
			} finally {
				try {
					if (id) await destroyActor(id);
				} finally {
					await client.dispose();
				}
			}
		}, 120_000);
	},
);
