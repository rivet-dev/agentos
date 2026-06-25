import { agentOS } from "@rivet-dev/agentos";

export const vm = agentOS({
  // Runs once per session event, server-side, for every session.
  onSessionEvent: async (sessionId, event) => {
    console.log("Session event:", sessionId, event.method);
  },
});
