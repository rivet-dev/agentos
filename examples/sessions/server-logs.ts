import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "@agentos-software/pi";

// Swap in your own structured logger.
const logger = console;

const vm = agentOS({
  software: [pi],
  onAgentStderr(event) {
    // event: { sessionId, agentType, processId, pid, chunk: Uint8Array }
    const line = new TextDecoder().decode(event.chunk);
    logger.info(`[agent:${event.agentType} session:${event.sessionId}] ${line}`);
  },
});

export const registry = setup({ use: { vm } });
registry.start();
