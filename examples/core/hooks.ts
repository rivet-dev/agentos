import { AgentOs } from "@rivet-dev/agentos-core";
import pi from "@agentos-software/pi";

// With the core package, session events and permission requests are observed
// per-session on the AgentOs instance (there is no actor-factory hook).
const vm = await AgentOs.create({ software: [pi] });
const { sessionId } = await vm.createSession("pi");

// Runs for every event on this session.
vm.onSessionEvent(sessionId, (event) => {
  console.log("Session event:", sessionId, event.method);
});

// Fires when the agent requests permission.
vm.onPermissionRequest(sessionId, (request) => {
  console.log("Permission request:", sessionId, request.permissionId);
});
