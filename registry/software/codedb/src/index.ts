import type { WasmCommandPackage } from "@rivet-dev/agent-os-registry-types";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

const pkg = {
	name: "codedb",
	aptName: "codedb",
	description: "codedb code intelligence CLI (reduced WASI fork)",
	source: "zig" as const,
	commands: [{ name: "codedb", permissionTier: "full" as const }],
	get commandDir() {
		return resolve(__dirname, "..", "wasm");
	},
} satisfies WasmCommandPackage;

export default pkg;
