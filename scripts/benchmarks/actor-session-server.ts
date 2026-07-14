import { agentOS, setup } from "@rivet-dev/agentos";

const agentIds = (process.env.BENCH_AGENTS ?? "claude,pi,codex,opencode")
	.split(",")
	.map((value) => value.trim())
	.filter(Boolean);
const loaders: Record<string, () => Promise<{ default: unknown }>> = {
	claude: () => import("@agentos-software/claude-code"),
	pi: () => import("@agentos-software/pi"),
	codex: () => import("@agentos-software/codex"),
	opencode: () => import("@agentos-software/opencode"),
};
const software = await Promise.all(
	agentIds.map(async (id) => {
		const load = loaders[id];
		if (!load) throw new Error(`Unknown BENCH_AGENTS entry: ${id}`);
		return (await load()).default;
	}),
);

const enginePort = Number.parseInt(
	process.env.RIVET_RUN_ENGINE_PORT ?? "16420",
	10,
);
const mockPort = Number.parseInt(process.env.BENCH_MOCK_PORT ?? "16480", 10);

const vm = agentOS({
	software,
	loopbackExemptPorts: [mockPort],
	permissions: { network: "allow" },
	// OpenCode's first prompt exceeds the runtime's 128 MiB default heap.
	limits: { jsRuntime: { v8HeapLimitMb: 256 } },
});

export const registry = setup({
	use: { vm },
	enginePort,
	endpoint: `http://127.0.0.1:${enginePort}`,
});

registry.start();
