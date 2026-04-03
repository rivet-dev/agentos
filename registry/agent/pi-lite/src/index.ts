import { defineSoftware } from "@rivet-dev/agent-os-core";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const packageDir = resolve(__dirname, "..");

const piLiteCommand = defineSoftware({
	name: "pi-lite-command",
	type: "tool" as const,
	packageDir,
	requires: ["@rivet-dev/agent-os-pi-lite"],
	bins: {
		"pi-lite": "@rivet-dev/agent-os-pi-lite",
	},
});

const piLiteAgent = defineSoftware({
	name: "pi-lite",
	type: "agent" as const,
	packageDir,
	requires: ["@rivet-dev/agent-os-pi-lite"],
	agent: {
		id: "pi-lite",
		acpAdapter: "@rivet-dev/agent-os-pi-lite",
		agentPackage: "@rivet-dev/agent-os-pi-lite",
		env: (ctx) => ({
			PI_LITE_COMMAND: "pi-lite",
			PI_LITE_LIBRARY_PATH_HOST: resolve(packageDir, "bin"),
			PI_LITE_LIBRARY_PATH: `${ctx.resolvePackage("@rivet-dev/agent-os-pi-lite")}/bin`,
		}),
		prepareInstructions: async (
			kernel,
			_cwd,
			additionalInstructions,
			opts,
		) => {
			const parts: string[] = [];
			if (!opts?.skipBase) {
				const data = await kernel.readFile("/etc/agentos/instructions.md");
				parts.push(new TextDecoder().decode(data));
			}
			if (additionalInstructions) parts.push(additionalInstructions);
			if (opts?.toolReference) parts.push(opts.toolReference);
			parts.push("---");
			const instructions = parts.join("\n\n");
			if (!instructions) return {};
			return { args: ["--append-system-prompt", instructions] };
		},
	},
});

const piLite = [piLiteCommand, piLiteAgent] as const;

export default piLite;
