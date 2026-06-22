import { defineSoftware } from "@rivet-dev/agentos-core";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const packageDir = resolve(__dirname, "..");

const piCli = defineSoftware({
	name: "pi-cli",
	type: "agent" as const,
	packageDir,
	requires: ["pi-acp", "@mariozechner/pi-coding-agent"],
	agent: {
		id: "pi-cli",
		acpAdapter: "pi-acp",
		agentPackage: "@mariozechner/pi-coding-agent",
		env: (ctx) => ({
			PI_ACP_PI_COMMAND: ctx.resolveBin(
				"@mariozechner/pi-coding-agent",
				"pi",
			),
		}),
	},
});

export default piCli;
