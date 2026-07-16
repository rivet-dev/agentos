import { agentOS, setup } from "../../dist/index.js";
import { allowAll } from "@rivet-dev/agentos-core/internal/runtime-compat";
import { event } from "rivetkit";

const vm = agentOS({
	defaultSoftware: false,
	permissions: allowAll,
	createState: (_c, input) => ({
		wakeCount: 0,
		creationInput: input ?? null,
	}),
	events: {
		customLifecycle: event(),
	},
	actions: {
		echo: (_c, value) => value,
		getCreationInput: (c) => c.state.creationInput,
		getWakeCount: (c) => c.state.wakeCount,
		sleepActor: (c) => c.sleep(),
		inspectAgentOsStorage: async (c) => {
			const tables = await c.db.execute(
				"SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'agentos_vfs_%' ORDER BY name",
			);
			const metadata = await c.db.execute(
				"SELECT COUNT(*) AS count FROM agentos_vfs_metadata_heads",
			);
			const metadataChunks = await c.db.execute(
				"SELECT COUNT(*) AS count, COALESCE(SUM(length(content)), 0) AS bytes FROM agentos_vfs_metadata_chunks",
			);
			const blocks = await c.db.execute(
				"SELECT COUNT(*) AS count, COALESCE(SUM(length(content)), 0) AS bytes FROM agentos_vfs_blocks",
			);
			return {
				tables: tables.map((row) => row.name),
				metadataCount: Number(metadata[0]?.count ?? 0),
				metadataChunkCount: Number(metadataChunks[0]?.count ?? 0),
				metadataChunkBytes: Number(metadataChunks[0]?.bytes ?? 0),
				blockCount: Number(blocks[0]?.count ?? 0),
				blockBytes: Number(blocks[0]?.bytes ?? 0),
			};
		},
	},
	onWake: (c) => {
		c.state.wakeCount += 1;
		c.broadcast("customLifecycle", {
			phase: "wake",
			wakeCount: c.state.wakeCount,
		});
	},
});

export const registry = setup({
	use: { vm },
	endpoint: process.env.AGENTOS_E2E_ENDPOINT,
	namespace: process.env.RIVET_NAMESPACE ?? "default",
	token: process.env.RIVET_TOKEN ?? "dev",
	envoy: { poolName: process.env.AGENTOS_E2E_POOL_NAME ?? "agentos-e2e" },
	runtime: "native",
});

registry.start();
