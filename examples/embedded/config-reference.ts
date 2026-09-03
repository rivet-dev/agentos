import { AgentOs, nodeModulesMount } from "@rivet-dev/agentos-core";

// Common embedded AgentOs.create() configuration. Embedded Core additionally
// permits trusted host resources that the hosted actor intentionally forbids.
const vm = await AgentOs.create({
	// Durable SQLite storage for the root filesystem and Core state.
	// Omit it for an in-memory VM that keeps nothing after dispose().
	database: { type: "sqlite_file", path: ".agentos/agentos.sqlite" },
	// Filesystems to mount at boot. Use nodeModulesMount() to expose a host
	// node_modules tree at /root/node_modules.
	mounts: [nodeModulesMount("/path/to/project/node_modules")],
	// Kernel permission policy (see /agentos/docs/permissions) and runtime caps.
	// Embedded Core may also configure host bindings.
	permissions: { network: "allow" },
	limits: { jsRuntime: { v8HeapLimitMb: 128 } },
	// Trusted embedded callers may install a local .aospkg path. The hosted actor
	// accepts only remote HTTPS sources.
	software: [{ packagePath: "/path/to/tool.aospkg" }],
	// Also install the default software bundle (sh + coreutils). Defaults to true;
	// set false for a bare VM with only the software you list.
	defaultSoftware: true,
	// Ports exempt from SSRF checks (for testing against host-side mock servers)
	loopbackExemptPorts: [3000],
	// Sidecar placement defaults to the shared `default` pool.
	sidecar: { kind: "shared" },
});

await vm.dispose();
