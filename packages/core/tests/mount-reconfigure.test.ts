import { describe, expect, it } from "vitest";
import { createInMemoryFileSystem } from "../src/test/runtime.js";
import { NativeSidecarKernelProxy } from "../src/sidecar/rpc-client.js";

// Regression coverage for post-boot mountFs delivery to the native sidecar:
//   1. `configure_vm` replaces boot configuration, so mount changes must resend
//      boot packages, their projection root, and binding shims. Runtime-linked
//      packages are retained by the sidecar, not mirrored by this client.
//   2. mountFs used to be fire-and-forget with a swallowed rejection, so a
//      failed reconfigure left the mount silently host-only and callers had no
//      way to know when (or whether) the guest could see it.
// The proxy is exercised against a stub SidecarProcess so the test stays fast
// and deterministic without booting a real VM.

const session = { connectionId: "conn-1", sessionId: "sess-1" };
const vm = { vmId: "vm-test" };

const bootPackages = [
	{
		name: "common",
		version: "1.0.0",
		path: "/tmp/common.aospkg",
	},
];
const bootBindingShims = ["agentos", "agentos-demo"];

function createStubClient(options?: { failConfigureVm?: boolean }) {
	const configureCalls: Array<Record<string, unknown>> = [];
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
				appliedSoftware: [],
				projectedCommands: [],
			};
		},
		async disposeVm() {},
		async dispose() {},
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
	return { client, configureCalls };
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
		packages: bootPackages,
		packagesMountAt: "/opt/agentos",
		bindingShimCommands: bootBindingShims,
		commandGuestPaths: new Map<string, string>(),
		ownsClient: true,
	};
	return new NativeSidecarKernelProxy(
		options as ConstructorParameters<typeof NativeSidecarKernelProxy>[0],
	);
}

describe("post-boot mount reconfiguration", () => {
	it("resends the boot packages and binding shims on runtime mountFs", async () => {
		const { client, configureCalls } = createStubClient();
		const proxy = createProxy(client);

		await proxy.mountFs("/mnt/dynamic", createInMemoryFileSystem());

		expect(configureCalls).toHaveLength(1);
		const payload = configureCalls[0];
		expect(payload.packages).toEqual(bootPackages);
		expect(payload.packagesMountAt).toBe("/opt/agentos");
		expect(payload.bindingShimCommands).toEqual(bootBindingShims);
		expect(payload.mounts).toEqual([
			expect.objectContaining({ guestPath: "/mnt/dynamic" }),
		]);

		await proxy.unmountFs("/mnt/dynamic");
		expect(configureCalls).toHaveLength(2);
		expect(configureCalls[1].mounts).toEqual([]);
		expect(configureCalls[1].packages).toEqual(bootPackages);
		expect(configureCalls[1].bindingShimCommands).toEqual(bootBindingShims);

		await proxy.dispose();
	});

	it("keeps runtime command registrations out of the boot package payload", async () => {
		const { client, configureCalls } = createStubClient();
		const proxy = createProxy(client);

		// This is the only local update made after linkSoftware receives the
		// sidecar's projected command paths. Rust tests verify that the sidecar
		// retains the package across these boot-only configuration payloads.
		proxy.registerCommandGuestPaths(
			new Map([["linked-command", "/opt/agentos/bin/linked-command"]]),
		);
		await proxy.mountFs("/mnt/dynamic", createInMemoryFileSystem());
		expect(configureCalls[0].packages).toEqual(bootPackages);

		await proxy.unmountFs("/mnt/dynamic");
		expect(configureCalls[1].packages).toEqual(bootPackages);

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

	it("resolves unmountFs immediately for an unknown mount without reconfiguring", async () => {
		const { client, configureCalls } = createStubClient();
		const proxy = createProxy(client);

		await proxy.unmountFs("/mnt/never-mounted");
		expect(configureCalls).toHaveLength(0);

		await proxy.dispose();
	});
});
