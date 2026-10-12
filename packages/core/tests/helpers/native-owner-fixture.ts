import type { ChildProcess } from "node:child_process";
import { readFile } from "node:fs/promises";

// Fixture-only bounds. Production transport/VM disposal deadlines are unchanged.
export async function bounded<T>(
	operation: Promise<T>,
	timeoutMs: number,
	label: string,
): Promise<T> {
	let timer: ReturnType<typeof setTimeout> | undefined;
	try {
		return await Promise.race([
			operation,
			new Promise<never>((_, reject) => {
				timer = setTimeout(
					() => reject(new Error(`${label} exceeded ${timeoutMs}ms`)),
					timeoutMs,
				);
			}),
		]);
	} finally {
		clearTimeout(timer);
	}
}

export async function ownedProcessState(
	child: ChildProcess,
): Promise<string | null> {
	if (!child.pid) throw new Error("owned child has no pid");
	try {
		const stat = await readFile(`/proc/${child.pid}/stat`, "utf8");
		return stat.slice(stat.lastIndexOf(")") + 2).split(" ")[0];
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
		throw error;
	}
}

export async function waitUntilStopped(child: ChildProcess): Promise<void> {
	const deadline = performance.now() + 1_000;
	while (performance.now() < deadline) {
		const state = await ownedProcessState(child);
		if (state === "T") return;
		if (
			state === null ||
			child.exitCode !== null ||
			child.signalCode !== null
		) {
			throw new Error("owned sidecar exited before SIGSTOP was observed");
		}
		await new Promise((resolve) => setTimeout(resolve, 5));
	}
	throw new Error("owned sidecar did not enter stopped state within1000ms");
}

// Register before any possible exit. Resolving close observes Node's actual
// waitpid result; no guessed status, process-wide scan, or foreign PID signal.
export function observeOwnedClose(child: ChildProcess) {
	return new Promise<{ code: number | null; signal: NodeJS.Signals | null }>(
		(resolve) => {
			child.once("close", (code, signal) => resolve({ code, signal }));
		},
	);
}

export async function bodyWithCleanup(
	body: () => Promise<void>,
	cleanup: () => Promise<void>,
): Promise<void> {
	const errors: unknown[] = [];
	try {
		await body();
	} catch (error) {
		errors.push(error);
	}
	try {
		await cleanup();
	} catch (error) {
		errors.push(error);
	}
	if (errors.length === 1) throw errors[0];
	if (errors.length > 1)
		throw new AggregateError(
			errors,
			"native ownership test and cleanup failed",
		);
}
