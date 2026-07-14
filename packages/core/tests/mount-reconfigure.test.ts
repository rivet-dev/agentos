import { describe, expect, it } from "vitest";
import { createInMemoryFileSystem } from "../src/memory-filesystem.js";
import { NativeSidecarKernelProxy } from "../src/sidecar/rpc-client.js";

// Regression coverage for post-boot mountFs delivery to the native sidecar:
//   1. Package projections are sidecar-owned, so the client sends only the
//      changed mount set and does not cache/replay boot or linked packages.
//   2. mountFs used to be fire-and-forget with a swallowed rejection, so a
//      failed reconfigure left the mount silently host-only and callers had no
//      way to know when (or whether) the guest could see it.
// The proxy is exercised against a stub SidecarProcess so the test stays fast
// and deterministic without booting a real VM.

const session = { connectionId: "conn-1", sessionId: "sess-1" };
const vm = { vmId: "vm-test" };

function createStubClient(options?: { failConfigureVm?: boolean }) {
	const configureCalls: Array<Record<string, unknown>> = [];
	const readCalls: string[] = [];
	const client = {
		async configureVm(
			_session: unknown,
			_vm: unknown,
			payload: Record<string, unknown>,
		) {
			configureCalls.push(payload);
			if (options?.failConfigureVm) {
				throw new Error("configure_vm rejected");
			}
			return {
				appliedMounts: [],
				projectedCommands: [],
				agents: [],
			};
		},
		async disposeVm() {},
		async dispose() {},
		async readFile(_session: unknown, _vm: unknown, path: string) {
			readCalls.push(path);
			return new TextEncoder().encode("from sidecar");
		},
		waitForEvent(
			_filter: unknown,
			_unused: unknown,
			opts: { signal: AbortSignal },
		) {
			return new Promise((_resolve, reject) => {
				opts.signal.addEventListener("abort", () =>
					reject(new Error("aborted")),
				);
			});
		},
	};
	return { client, configureCalls, readCalls };
}

function createProxy(client: unknown) {
	const options = {
		client,
		session,
		vm,
		env: {},
		cwd: "/work",
		localMounts: [],
		sidecarMounts: [],
		ownsClient: true,
	};
	return new NativeSidecarKernelProxy(
		options as ConstructorParameters<typeof NativeSidecarKernelProxy>[0],
	);
}

describe("post-boot mount reconfiguration", () => {
	it("sends only mounts on runtime mountFs", async () => {
		const { client, configureCalls } = createStubClient();
		const proxy = createProxy(client);

		await proxy.mountFs("/mnt/dynamic", createInMemoryFileSystem());

		expect(configureCalls).toHaveLength(1);
		const payload = configureCalls[0];
		expect(payload).not.toHaveProperty("packages");
		expect(payload).not.toHaveProperty("packagesMountAt");
		expect(payload).not.toHaveProperty("permissions");
		expect(payload).not.toHaveProperty("commandPermissions");
		expect(payload).not.toHaveProperty("loopbackExemptPorts");
		expect(payload.mounts).toEqual([
			expect.objectContaining({ guestPath: "/mnt/dynamic" }),
		]);

		await proxy.unmountFs("/mnt/dynamic");
		expect(configureCalls).toHaveLength(2);
		expect(configureCalls[1].mounts).toEqual([]);

		await proxy.dispose();
	});

	it("rejects the mountFs promise when sidecar delivery fails", async () => {
		const { client } = createStubClient({ failConfigureVm: true });
		const proxy = createProxy(client);

		await expect(
			proxy.mountFs("/mnt/dynamic", createInMemoryFileSystem()),
		).rejects.toThrow("configure_vm rejected");

		await proxy.dispose();
	});

	it("routes public filesystem calls through the sidecar and keeps host mounts callback-only", async () => {
		const { client, readCalls } = createStubClient();
		const proxy = createProxy(client);
		const hostFs = createInMemoryFileSystem();
		await hostFs.writeFile("/note.txt", "from host callback");
		await proxy.mountFs("/mnt/dynamic", hostFs);

		expect(
			new TextDecoder().decode(await proxy.readFile("/mnt/dynamic/note.txt")),
		).toBe("from sidecar");
		expect(readCalls).toEqual(["/mnt/dynamic/note.txt"]);
		const callbackFilesystem = proxy.hostFilesystemForMount("/mnt/dynamic");
		expect(callbackFilesystem).toBe(hostFs);
		if (!callbackFilesystem) throw new Error("missing callback filesystem");
		expect(
			new TextDecoder().decode(await callbackFilesystem.readFile("/note.txt")),
		).toBe("from host callback");

		await proxy.dispose();
	});

	it("resolves unmountFs immediately for an unknown mount without reconfiguring", async () => {
		const { client, configureCalls } = createStubClient();
		const proxy = createProxy(client);

		await proxy.unmountFs("/mnt/never-mounted");
		expect(configureCalls).toHaveLength(0);

		await proxy.dispose();
	});
});
