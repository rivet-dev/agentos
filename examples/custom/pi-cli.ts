import { defineSoftware } from "@rivet-dev/agentos";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");

// Example: a pi-acp adapter that runs the Pi CLI.
export default defineSoftware({
  name: "pi-cli",
  type: "agent",
  packageDir,
  // Two separate packages: the ACP adapter, and the agent it drives.
  requires: ["pi-acp", "@mariozechner/pi-coding-agent"],
  agent: {
    id: "pi-cli",
    // `pi-acp` is a thin ACP adapter. It does NOT run the agent itself:
    // it speaks ACP on stdio and spawns the `pi` CLI as a separate child
    // process, translating between ACP and the CLI.
    acpAdapter: "pi-acp",
    // The actual agent, launched by pi-acp as its own process.
    agentPackage: "@mariozechner/pi-coding-agent",
    // Tell the adapter where to find the `pi` CLI inside the VM
    // (resolved at boot to a guest path under /root/node_modules).
    env: (ctx) => ({
      PI_ACP_PI_COMMAND: ctx.resolveBin("@mariozechner/pi-coding-agent", "pi"),
    }),
  },
});
