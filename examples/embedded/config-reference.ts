import { AgentOs, nodeModulesMount } from "@rivet-dev/agentos";

// Common AgentOs.create() configuration. The embedding process is trusted, so
// it may also configure host resources such as host directories and local
// package paths.
const vm = await AgentOs.create({
	// SQLite storage for VM-owned state. On its own it does not make the root
	// filesystem durable; see /agentos/docs/persistence.
	database: { type: "sqlite_file", path: ".agentos/agentos.sqlite" },
	// Store the root filesystem in `database` so files survive VM recreation.
	// Omit it for the default in-memory root, discarded on dispose().
	rootFilesystem: {
		type: "native",
		plugin: { id: "chunked_sqlite", config: {} },
	},
	// Filesystems to mount at boot. Use nodeModulesMount() to expose a host
	// node_modules tree at /root/node_modules.
	mounts: [nodeModulesMount("/path/to/project/node_modules")],
	// Kernel permission policy (see /agentos/docs/permissions) and runtime caps.
	permissions: { network: "allow" },
	limits: { jsRuntime: { v8HeapLimitMb: 128 } },
	// Install a packed .aospkg from a trusted local path, and a registry package
	// by name, at boot.
	software: [{ packagePath: "/path/to/tool.aospkg" }, { registry: "git" }],
	// Where registry packages resolve. Defaults to releases.rivet.dev at this
	// client's release; point `url` at a mirror or pin `version`.
	softwareRegistry: { url: "https://releases.rivet.dev/agent-os/" },
	// Also install the default software bundle (sh + coreutils). Defaults to true;
	// set false for a bare VM with only the software you list.
	defaultSoftware: true,
	// Ports exempt from SSRF checks (for testing against host-side mock servers)
	loopbackExemptPorts: [3000],
	// Sidecar placement defaults to the shared `default` pool.
	sidecar: { kind: "shared" },
});

await vm.dispose();
