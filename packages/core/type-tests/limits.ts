import type { AgentOsLimits } from "../src/index.js";

const emptyLimits: AgentOsLimits = { tls: {}, execution: {} };
const explicitLimits: AgentOsLimits = {
	tls: { maxBufferedBytes: 2048 },
	execution: {
		completedTtlMs: 60_000,
		maxCompletedExecutions: 128,
		liveExecutionWarningThreshold: 32,
	},
};
void [emptyLimits, explicitLimits];

const invalidTls: AgentOsLimits = {
	// @ts-expect-error limit fields use camelCase wire names
	tls: { max_buffered_bytes: 2048 },
};
const invalidExecution: AgentOsLimits = {
	// @ts-expect-error execution retention fields use explicit units
	execution: { completedTtl: 60_000 },
};
void [invalidTls, invalidExecution];
