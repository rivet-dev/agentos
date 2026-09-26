import type { KernelProcessInfo } from "./types";

/** Display PIDs can overlap between tracked roots and untracked guest children. */
export function processRowIdentity(
	row: Pick<KernelProcessInfo, "generation" | "pid" | "process">,
): string {
	return row.process
		? `${row.process.generation}:tracked:${row.process.pid}`
		: `${row.generation}:guest:${row.pid}`;
}
