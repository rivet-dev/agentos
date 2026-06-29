import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "@agentos-software/pi";

const vm = agentOS({
  software: [pi],
  onSessionEvent: async (sessionId, event) => {
    // Server-side hook runs once per event, even with multiple clients
    console.log("Session event:", sessionId, event.method);
  },
});

export const registry = setup({ use: { vm } });

registry.start();
