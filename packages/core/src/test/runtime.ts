/**
 * Internal test-only runtime exports for cross-package integration suites.
 *
 * This keeps repo-owned tests pointed at an Agent OS package surface even
 * while the public SDK removes the raw vm.kernel escape hatch.
 */

export {
	type AgentOsRuntimeAdmin,
	getAgentOsKernel,
	getAgentOsRuntimeAdmin,
} from "../agent-os.js";
export type { VirtualFileSystem } from "../runtime.js";
export { createInMemoryFileSystem } from "../memory-filesystem.js";
export { TerminalHarness } from "./terminal-harness.js";
