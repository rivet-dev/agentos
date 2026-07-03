import common from "@agentos-software/common";
import { agentOS, setup } from "@rivet-dev/agentos";

// TEMP (local debug): only `common` (sha 42d8146) has valid projected packs;
// `sqlite3` (efc374f) is missing its `dist/package`, so it's dropped for now.
const shellVm = agentOS({
	software: [common],
});

export const registry = setup({ use: { shellVm } });

registry.start();
