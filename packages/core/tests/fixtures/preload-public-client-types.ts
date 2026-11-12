import type { AgentOsClient } from "../../src/hosted-client.js";

// Compile-only proof against the actual agentOS public return type. No fake
// Registry, ActorDefinition, generated coordinator namespace, cast, or any.
export function publicCoordinatorMethods(client: AgentOsClient) {
  const handle = client.getOrCreate("agentOS-preload-coordinator", ["type-only"], { createWithInput: {} });
  const actorId: Promise<string> = handle.resolve();
  const status: Promise<unknown> = handle.action({ name: "preload.status", args: [null] });
  const replace: Promise<unknown> = handle.action({ name: "preload.replaceBaseline", args: [{ protocolVersion: 1, artifacts: [] }] });
  return { actorId, status, replace };
}
