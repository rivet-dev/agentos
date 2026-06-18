import { defineSoftware } from "@rivet-dev/agent-os-core";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const packageDir = resolve(__dirname, "..");

const piRustCommand = defineSoftware({
	name: "pi-rust-command",
	type: "tool" as const,
	packageDir,
	requires: ["@rivet-dev/agent-os-pi-rust"],
	bins: {
		"pi-rust": "@rivet-dev/agent-os-pi-rust",
	},
});

const piRustAgent = defineSoftware({
	name: "pi-rust",
	type: "agent" as const,
	packageDir,
	requires: ["@rivet-dev/agent-os-pi-rust"],
	agent: {
		id: "pi-rust",
		acpAdapter: "@rivet-dev/agent-os-pi-rust",
		agentPackage: "@rivet-dev/agent-os-pi-rust",
		env: (ctx) => ({
			MALLOC_ARENA_MAX: "1",
			PI_RUST_COMMAND: "pi-rust",
			PI_RUST_LIBRARY_PATH_HOST: resolve(packageDir, "bin"),
			PI_RUST_LIBRARY_PATH: `${ctx.resolvePackage("@rivet-dev/agent-os-pi-rust")}/bin`,
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

const piRust = [piRustCommand, piRustAgent] as const;

export default piRust;
