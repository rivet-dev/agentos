import { AgentOs } from "@rivet-dev/agentos-core";
import {
  createSandboxBindings,
  createSandboxFs,
} from "@rivet-dev/agentos-sandbox";
import { SandboxAgent } from "sandbox-agent";
import { docker } from "sandbox-agent/docker";

const sandbox = await SandboxAgent.start({ sandbox: docker() });

const vm = await AgentOs.create({
  // Bindings let the agent control the sandbox.
  bindings: [createSandboxBindings({ client: sandbox })],
  // Mounts let the agent read the sandbox filesystem.
  mounts: [
    {
      path: "/home/agentos/sandbox",
      plugin: createSandboxFs({ client: sandbox }),
    },
  ],
});

await vm.dispose();
await sandbox.dispose();
