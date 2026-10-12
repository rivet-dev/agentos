import { randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import { describe, expect, test } from "vitest";
import { destroyActor, localClient, softwareSources } from "./hosted-local-fixture.js";

async function rejectionOf(operation: Promise<unknown>): Promise<unknown> {
	try { await operation; } catch (error) { return error; }
	throw new Error("budget control unexpectedly succeeded");
}

function publicBudget(error: unknown, setting: string, observed: number, limit: number, remedy: string) {
	expect(error).toMatchObject({ group: "user", code: "limit_exceeded", metadata: { budget: {
		setting, observed, limit, units: "bytes", remediation: expect.stringContaining(remedy),
	} } });
	const value = error as { message: string; metadata: { budget: Record<string, unknown> } };
	expect(value.message).toContain(setting);
	expect(Object.keys(value.metadata)).toEqual(["budget"]);
	expect(Object.keys(value.metadata.budget).sort()).toEqual(["limit", "observed", "remediation", "setting", "units"]);
}

describe.runIf(process.env.AGENTOS_HOSTED_TEST_REQUIRE_LOCAL === "1")(
	"owned hosted actor budget transfer",
	() => {
		test("uses startup budgets and retains the independent VM budget", async () => {
			const mode = process.env.AGENTOS_HOSTED_TEST_BUDGET_CASE;
			if (mode !== "default" && mode !== "raised" && mode !== "vm-limited") {
				throw new Error("owned fixture requires an explicit default/raised/vm-limited budget case");
			}
			const client = localClient();
			const ids: string[] = [];
			const errors: unknown[] = [];
			try {
				const vm = client.agentOS.getOrCreate(["budget-ts", randomUUID()], {
					createWithInput: {
						config: {
							software: softwareSources(),
							...(mode !== "vm-limited" ? { permissions: { network: "allow" as const } } : {}),
							...(mode === "raised" ? { limits: { jsRuntime: { maxReadlineHistoryEntries: 2048 }, http: { maxChunkSizeLineBytes: 32768 } } } : {}),
							filesystem: { mounts: [{ path: "/workspace", backend: { type: "actor-sqlite" }, readOnly: false }] },
							...(mode === "vm-limited" ? { limits: { resources: { maxFilesystemBytes: 1024 * 1024 } } } : {}),
						},
					},
				});
				ids.push(await vm.resolve());
				const status = await vm.vm.status({});
				expect(status.lifecycle).toBe("ready");
				const transferBytes = mode === "vm-limited" ? 1536 * 1024 : 800 * 1024;
				const bytes = Uint8Array.from({ length: transferBytes }, (_, i) => (i * 31 + 17) & 255);
				const path = mode === "vm-limited" ? "/tmp/budget-vm.bin" : "/workspace/budget.bin";
				if (mode === "default") {
					// Tiny request reaches actor policy even if the large byte tag is
					// rejected earlier by the default message/codec envelope.
					publicBudget(await rejectionOf(vm.filesystem.readFile({ path, maxBytes: 768 * 1024 + 1 })),
						"actor.filesystem.maxTransferBytes", 768 * 1024 + 1, 768 * 1024, "--actor-budgets-json");
					let rejected: unknown;
					try { await vm.filesystem.writeFile({ path, content: bytes }); } catch (error) { rejected = error; }
					// RivetKit may reject the larger encoded tag before actor dispatch.
					expect(rejected).toMatchObject({ code: expect.stringMatching(/^(limit_exceeded|incoming_too_long)$/) });
					if (rejected && typeof rejected === "object" && "code" in rejected) {
						console.info("owned default budget rejection", { code: rejected.code });
					}
				} else if (mode === "vm-limited") {
					const failure = await rejectionOf(vm.filesystem.writeFile({ path, content: bytes }));
					const projection = failure as { group?: unknown; code?: unknown; message?: unknown; metadata?: { limit?: unknown; budget?: unknown } | null };
					console.info("owned VM quota diagnostic projection", {
						internalMasked: projection.group === "core" && projection.code === "internal_error",
						namedLimit: typeof projection.message === "string" && projection.message.includes("limits.resources.maxFilesystemBytes"),
						hasTypedLimit: projection.metadata?.limit !== undefined,
						hasActorBudget: projection.metadata?.budget !== undefined,
					});
					// Preserve the required actionable owning diagnostic. A masked
					// rejection is a diagnostic RED, not sufficient VM acceptance.
					expect(failure).toMatchObject({ group: "user", code: "limit_exceeded", message: expect.stringContaining("limits.resources.maxFilesystemBytes"), metadata: { limit: {
						setting: "limits.resources.maxFilesystemBytes", limit: "1048576", observed: expect.stringMatching(/^[0-9]+$/), units: "bytes",
						remediation: expect.stringContaining("limits.resources.maxFilesystemBytes"),
					} } });
					const limit = (failure as { metadata: { limit: { observed: string } } }).metadata.limit;
					expect(BigInt(limit.observed)).toBeGreaterThan(1048576n);
					expect(BigInt(limit.observed)).toBeGreaterThanOrEqual(BigInt(bytes.length));
					expect(Object.keys((failure as { metadata: object }).metadata)).toEqual(["limit"]);
					expect(Object.keys(limit).sort()).toEqual(["limit", "observed", "remediation", "setting", "units"]);
					expect(projection.metadata?.budget).toBeUndefined();
				} else {
					await vm.filesystem.writeFile({ path, content: bytes });
					expect(await vm.filesystem.readFile({ path, maxBytes: bytes.length })).toEqual(bytes);
					publicBudget(await rejectionOf(vm.filesystem.readFile({ path, maxBytes: bytes.length - 1 })),
						"maxBytes", bytes.length, bytes.length - 1, "per-call maxBytes");
					expect(await vm.filesystem.readFile({ path, maxBytes: bytes.length })).toEqual(bytes);
				}

				if (mode !== "vm-limited") {
					async function bridgePolicy(actor: typeof vm, raised: boolean) {
						const source = (name: string) => readFileSync(new URL(`./fixtures/${name}`, import.meta.url), "utf8").replaceAll("__RAISED__", String(raised));
						const http = await actor.javascript.execute({ source: source("hosted-http-policy.mjs"), options: { inline: { process: { timeoutMs: 15_000, output: { capture: "all" } } } } });
						expect(http.error).toBeNull();
						expect(http.exitCode).toBe(0);
						expect(Buffer.from(http.stdout ?? []).toString().trim()).toBe("hosted-http-policy-complete");
						const context = await actor.contexts.create({ contextId: "hosted-bridge-policy" });
						const failures: unknown[] = [];
						try {
							for (let turn = 1; turn <= 3; turn++) {
								const result = await actor.javascript.execute({ source: source("hosted-readline-policy.cjs").replaceAll("__TURN__", String(turn)), options: { format: "commonJs", inline: { process: { context: context.context, timeoutMs: 5_000, output: { capture: "all" } } } } });
								expect(result.error).toBeNull();
								expect(result.exitCode).toBe(0);
								expect(Buffer.from(result.stdout ?? []).length).toBeLessThanOrEqual(16 * 1024);
								expect(Buffer.from(result.stderr ?? []).length).toBeLessThanOrEqual(16 * 1024);
								expect(Buffer.from(result.stdout ?? []).toString().trim()).toBe(`hosted-bridge-turn-${turn}`);
							}
						} catch (error) { failures.push(error); }
						finally {
							try { await actor.contexts.delete({ context: context.context }); } catch (error) { failures.push(error); }
						}
						if (failures.length) throw new AggregateError(failures, "hosted retained bridge policy failed");
					}
					await bridgePolicy(vm, mode === "raised");
					if (mode === "raised") {
						// Omitted limits must stay defaulted in another VM on the same worker.
						const ordinary = client.agentOS.getOrCreate(["budget-ts-default", randomUUID()], { createWithInput: { config: { software: softwareSources(), permissions: { network: "allow" } } } });
						ids.push(await ordinary.resolve());
						await bridgePolicy(ordinary, false);
					}
				}
				expect((await vm.vm.status({})).lifecycle).toBe("ready");
			} catch (error) {
				errors.push(error);
			} finally {
				for (const id of ids) {
					try { await destroyActor(id); } catch (error) { errors.push(error); }
				}
				try { await client.dispose(); } catch (error) { errors.push(error); }
			}
			if (errors.length) throw new AggregateError(errors, "owned hosted budget control failed");
		}, 90_000);
	},
);
