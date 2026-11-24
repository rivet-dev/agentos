import { afterEach, describe, expect, test } from "vitest";
import { z } from "zod";
import { AgentOs, type VirtualFileSystem } from "../src/index.js";
import { createInMemoryFileSystem } from "../src/test/runtime.js";

// Every VM in this group is a tenant of one real sidecar process. A guest process must
// reach only its own VM's host functions and filesystem driver.
describe("VM callbacks on a shared sidecar", () => {
	const cleanups: Array<() => Promise<void>> = [];
	afterEach(async () => {
		for (const cleanup of cleanups.splice(0).reverse()) await cleanup();
	});

	async function createTenants() {
		const sidecar = await AgentOs.createSidecar();
		cleanups.push(() => sidecar.dispose());
		return (options: Parameters<typeof AgentOs.create>[0] = {}) =>
			createVm(sidecar, options);
	}

	async function createVm(
		sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>>,
		options: Parameters<typeof AgentOs.create>[0],
	) {
		const vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			...options,
		});
		let disposed = false;
		const dispose = async () => {
			if (disposed) return;
			disposed = true;
			await vm.dispose();
		};
		cleanups.push(dispose);
		return { vm, dispose };
	}

	function whoFunctions(tenant: string, calls: string[]) {
		return {
			t: {
				who: {
					inputSchema: z.object({ marker: z.string().optional() }),
					execute: ({ marker }: { marker?: string }) => {
						calls.push(tenant);
						return { tenant, marker };
					},
				},
			},
		};
	}

	// Spawn and wait: `execArgv` closes stdin, which can race a fast guest exit.
	async function runGuest(vm: AgentOs, command: string, args: string[]) {
		let stdout = "";
		let stderr = "";
		const { pid } = await vm.process.spawn(command, args, {
			onStdout: (chunk) => {
				stdout += new TextDecoder().decode(chunk);
			},
			onStderr: (chunk) => {
				stderr += new TextDecoder().decode(chunk);
			},
		});
		const { exitCode } = await vm.process.wait(pid);
		return { exitCode, stdout, stderr };
	}

	async function callWho(vm: AgentOs, ...args: string[]) {
		const result = await runGuest(vm, "agentos-t", ["who", ...args]);
		expect(result.exitCode, result.stderr).toBe(0);
		return JSON.parse(result.stdout).result;
	}

	test("a guest reaches only its own host functions while siblings come and go", async () => {
		const create = await createTenants();
		const calls: string[] = [];
		const a = await create({ hostFunctions: whoFunctions("a", calls) });
		const b = await create({ hostFunctions: whoFunctions("b", calls) });

		expect((await callWho(a.vm)).tenant).toBe("a");
		expect((await callWho(b.vm)).tenant).toBe("b");
		expect(calls).toEqual(["a", "b"]);

		// A disposed sibling's closure must never answer for a live VM.
		await b.dispose();
		expect((await callWho(a.vm)).tenant).toBe("a");
		expect(calls).toEqual(["a", "b", "a"]);

		// A later sibling without host functions must not break the VM.
		await create();
		expect((await callWho(a.vm)).tenant).toBe("a");
		expect(calls).toEqual(["a", "b", "a", "a"]);
	}, 120_000);

	test("registry commands read the guest's own host function list and input files", async () => {
		const create = await createTenants();
		const calls: string[] = [];
		const a = await create({
			hostFunctions: {
				...whoFunctions("a", calls),
				onlya: { f: { inputSchema: z.object({}), execute: () => "a" } },
			},
		});
		const b = await create({
			hostFunctions: {
				...whoFunctions("b", calls),
				onlyb: { f: { inputSchema: z.object({}), execute: () => "b" } },
			},
		});
		await a.vm.writeFile("/tmp/input.json", '{"marker":"file of a"}');
		await b.vm.writeFile("/tmp/input.json", '{"marker":"file of b"}');

		const listed = await runGuest(a.vm, "agentos", ["list-host-functions"]);
		expect(listed.exitCode, listed.stderr).toBe(0);
		expect(listed.stdout).toContain("onlya");
		expect(listed.stdout).not.toContain("onlyb");

		expect((await callWho(a.vm, "--json-file", "/tmp/input.json")).marker).toBe(
			"file of a",
		);
	}, 120_000);

	async function createOwnedFilesystem(secret: string) {
		const filesystem: VirtualFileSystem = createInMemoryFileSystem();
		// Mount flags do not override POSIX DAC: this fixture starts root-owned.
		await filesystem.chmod("/", 0o777);
		await filesystem.writeFile("/secret.txt", secret);
		return filesystem;
	}

	async function guestNode(vm: AgentOs, code: string) {
		return vm.execArgv("node", ["-e", code]);
	}

	test("a guest reads and writes only its own JS filesystem mount", async () => {
		const create = await createTenants();
		const filesystemA = await createOwnedFilesystem("secret of a");
		const filesystemB = await createOwnedFilesystem("secret of b");
		const a = await create({
			mounts: [{ path: "/mnt/x", driver: filesystemA }],
		});
		const b = await create({
			mounts: [{ path: "/mnt/x", driver: filesystemB }],
		});

		const read = await guestNode(
			a.vm,
			'process.stdout.write(require("node:fs").readFileSync("/mnt/x/secret.txt", "utf8"))',
		);
		expect(read.exitCode, read.stderr).toBe(0);
		expect(read.stdout).toBe("secret of a");

		const write = await guestNode(
			a.vm,
			'require("node:fs").writeFileSync("/mnt/x/written.txt", "from a")',
		);
		expect(write.exitCode, write.stderr).toBe(0);
		expect(await filesystemB.exists("/written.txt")).toBe(false);
		expect(
			new TextDecoder().decode(await filesystemA.readFile("/written.txt")),
		).toBe("from a");

		await b.dispose();
		const afterDispose = await guestNode(
			a.vm,
			'process.stdout.write(require("node:fs").readFileSync("/mnt/x/secret.txt", "utf8"))',
		);
		expect(afterDispose.exitCode, afterDispose.stderr).toBe(0);
		expect(afterDispose.stdout).toBe("secret of a");
	}, 120_000);

	test("a VM that is still being created never reaches a sibling's driver", async () => {
		const create = await createTenants();
		const siblingCalls: string[] = [];
		const siblingFilesystem = createInMemoryFileSystem();
		await create({
			mounts: [
				{
					path: "/mnt/x",
					driver: new Proxy(siblingFilesystem, {
						get(target, property, receiver) {
							const value = Reflect.get(target, property, receiver);
							if (typeof value !== "function") return value;
							return (...args: unknown[]) => {
								siblingCalls.push(String(property));
								return value.apply(target, args);
							};
						},
					}),
				},
			],
		});

		// Mounting under a JS mount makes the sidecar call back during configure,
		// before this VM's own handler exists. Whether create succeeds is not the
		// point here: the callback must not reach the sibling.
		await create({
			mounts: [
				{ path: "/mnt/x", driver: createInMemoryFileSystem() },
				{ path: "/mnt/x/nested", plugin: { id: "memory", config: {} } },
			],
		}).catch(() => {});

		expect(siblingCalls).toEqual([]);
	}, 120_000);

	test("disposing a VM with a callback in flight leaves its sibling working", async () => {
		const create = await createTenants();
		const calls: string[] = [];
		let releaseSlow = () => {};
		let startedSlow = () => {};
		const started = new Promise<void>((resolve) => {
			startedSlow = resolve;
		});
		const a = await create({ hostFunctions: whoFunctions("a", calls) });
		const b = await create({
			hostFunctions: {
				t: {
					who: {
						inputSchema: z.object({}),
						execute: async () => {
							calls.push("b");
							startedSlow();
							await new Promise<void>((resolve) => {
								releaseSlow = resolve;
							});
							return "late result of b";
						},
					},
				},
			},
		});

		const stuck = runGuest(b.vm, "agentos-t", ["who"]);
		stuck.catch(() => {});
		await started;
		const disposing = b.dispose();
		releaseSlow();
		await disposing;

		expect((await callWho(a.vm)).tenant).toBe("a");
		expect(calls).toEqual(["b", "a"]);
	}, 120_000);
});

describe("independent sidecar lifecycle", () => {
	test("disposing one owner preserves the other VM's files and execution", async () => {
		const sidecars: Awaited<ReturnType<typeof AgentOs.createSidecar>>[] = [];
		const errors: unknown[] = [];
		try {
			const vms: AgentOs[] = [];
			for (let index = 0; index < 2; index++) {
				// Equal display IDs must not give independent native owners shared storage.
				const sidecar = await AgentOs.createSidecar({
					sidecarId: "independent-lifecycle-owner",
				});
				sidecars.push(sidecar);
				const vm = await AgentOs.create({
					defaultSoftware: false,
					sidecar: { kind: "explicit", handle: sidecar },
				});
				vms.push(vm);
				await vm.writeFile("/tmp/owner.txt", String(index));
			}
			await vms[0].dispose();
			await sidecars[0].dispose();
			expect(
				new TextDecoder().decode(await vms[1].readFile("/tmp/owner.txt")),
			).toBe("1");
			const result = await vms[1].javascript.execute(
				"const fs = require('node:fs'); fs.writeFileSync('/tmp/survived.txt', fs.readFileSync('/tmp/owner.txt'));",
				{ format: "commonjs", timeoutMs: 3_000 },
			);
			expect(result.outcome, JSON.stringify(result)).toBe("succeeded");
			expect(
				new TextDecoder().decode(await vms[1].readFile("/tmp/survived.txt")),
			).toBe("1");
		} catch (error) {
			errors.push(error);
		} finally {
			const results = await Promise.allSettled(
				sidecars.map((owner) => owner.dispose()),
			);
			for (const result of results) {
				if (result.status === "rejected") errors.push(result.reason);
			}
		}
		if (errors.length)
			throw new AggregateError(errors, "independent sidecar lifecycle failed");
	}, 15_000);
});
