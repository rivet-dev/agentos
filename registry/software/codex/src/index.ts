import type { WasmCommandPackage } from "@rivet-dev/agent-os-registry-types";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

const pkg = {
	name: "codex",
	aptName: "codex",
	description: "OpenAI Codex command package (codex, codex-exec)",
	source: "rust" as const,
	commands: [
		{ name: "codex", permissionTier: "full" as const },
		{ name: "codex-exec", permissionTier: "full" as const },
	],
	get commandDir() {
		return resolve(__dirname, "..", "wasm");
	},
} satisfies WasmCommandPackage;

export default pkg;
