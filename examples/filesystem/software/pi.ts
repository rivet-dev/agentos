/**
 * Local stand-in for the `@agentos-software/pi` agent package.
 *
 * In a real app you would `import pi from "@agentos-software/pi"`; this fixture
 * is self-contained, so it builds an equivalently-shaped agent software
 * descriptor with `defineSoftware(...)` to exercise the `software: [...]` config
 * field.
 */
import { defineSoftware } from "@rivet-dev/agentos";

const pi = defineSoftware({
  name: "pi",
  type: "agent",
  packageDir: import.meta.dirname,
  requires: ["@agentos-software/pi", "@mariozechner/pi-coding-agent"],
  agent: {
    id: "pi",
    acpAdapter: "@agentos-software/pi",
    agentPackage: "@mariozechner/pi-coding-agent",
  },
});

export default pi;
