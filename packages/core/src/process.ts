import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import type { Duplex } from "node:stream";

export {
	SidecarProcessError,
	SidecarProcessExited,
} from "./sidecar-errors.js";

import { SidecarProcessError, SidecarProcessExited } from "./sidecar-errors.js";

/**
 * Bounds on the sidecar stderr excerpt retained for SidecarProcessExited and
 * SidecarProcessError. Live stderr is always forwarded to host stderr, so these
 * bound only the postmortem copy. Sidecar diagnostics can be guest-triggered,
 * so the retained copy must never grow without limit.
 */
const STDERR_HEAD_MAX_BYTES = 16 * 1024;
const STDERR_TAIL_MAX_BYTES = 48 * 1024;

export interface StdioSidecarProcessSpawnOptions {
	command: string;
	args?: string[];
	cwd?: string;
	combinedStdio?: boolean;
}

export class StdioSidecarProcess {
	readonly child: ChildProcessWithoutNullStreams;
	readonly control: Duplex | null;
	readonly combinedStdio: boolean;
	/** First bytes of sidecar stderr; the root cause usually appears here. */
	private readonly stderrHead: Buffer[] = [];
	private stderrHeadBytes = 0;
	/** Most recent bytes of sidecar stderr; the fatal error appears here. */
	private readonly stderrTail: Buffer[] = [];
	private stderrTailBytes = 0;
	private stderrDroppedBytes = 0;
	private readonly exitListeners = new Set<
		(error: SidecarProcessExited) => void
	>();
	private readonly errorListeners = new Set<
		(error: SidecarProcessError) => void
	>();

	private constructor(
		child: ChildProcessWithoutNullStreams,
		control: Duplex | null,
	) {
		this.child = child;
		this.control = control;
		this.combinedStdio = control === null;
		// Forward live sidecar stderr so warnings from a sidecar that survives a
		// guest-triggered failure stay host-visible. This matches the Rust
		// client, which spawns the sidecar with inherited stderr. Only a bounded
		// excerpt is retained for exit/error reports.
		this.child.stderr.on("data", (chunk: Buffer | string) => {
			const buffer =
				typeof chunk === "string" ? Buffer.from(chunk) : Buffer.from(chunk);
			process.stderr.write(buffer);
			this.retainStderr(buffer);
		});
		this.child.on("exit", (code, signal) => {
			const error = new SidecarProcessExited({
				exitCode: code,
				signal,
				stderr: this.stderrText(),
			});
			for (const listener of this.exitListeners) {
				listener(error);
			}
		});
		this.child.on("error", (error) => {
			const normalized =
				error instanceof Error ? error : new Error(String(error));
			const sidecarError = new SidecarProcessError(
				normalized,
				this.stderrText(),
			);
			for (const listener of this.errorListeners) {
				listener(sidecarError);
			}
		});
	}

	static spawn(options: StdioSidecarProcessSpawnOptions): StdioSidecarProcess {
		const combinedStdio = options.combinedStdio === true;
		const child = spawn(options.command, options.args ?? [], {
			cwd: options.cwd,
			env: combinedStdio
				? { ...process.env, AGENTOS_SIDECAR_COMBINED_STDIO: "1" }
				: process.env,
			stdio: combinedStdio
				? ["pipe", "pipe", "pipe"]
				: ["pipe", "pipe", "pipe", "pipe"],
		}) as unknown as ChildProcessWithoutNullStreams;
		try {
			return new StdioSidecarProcess(
				child,
				combinedStdio ? null : requireControlStream(child),
			);
		} catch (error) {
			child.kill("SIGKILL");
			throw error;
		}
	}

	static fromChild(
		child: ChildProcessWithoutNullStreams,
		control?: Duplex | null,
	): StdioSidecarProcess {
		return new StdioSidecarProcess(
			child,
			control === null ? null : (control ?? requireControlStream(child)),
		);
	}

	onExit(handler: (error: SidecarProcessExited) => void): () => void {
		this.exitListeners.add(handler);
		return () => {
			this.exitListeners.delete(handler);
		};
	}

	onError(handler: (error: SidecarProcessError) => void): () => void {
		this.errorListeners.add(handler);
		return () => {
			this.errorListeners.delete(handler);
		};
	}

	/**
	 * Retain the first STDERR_HEAD_MAX_BYTES and the most recent
	 * STDERR_TAIL_MAX_BYTES of sidecar stderr, counting everything in between
	 * as dropped.
	 */
	private retainStderr(buffer: Buffer): void {
		let rest = buffer;
		const headRoom = STDERR_HEAD_MAX_BYTES - this.stderrHeadBytes;
		if (headRoom > 0) {
			const take = rest.subarray(0, headRoom);
			this.stderrHead.push(take);
			this.stderrHeadBytes += take.length;
			rest = rest.subarray(take.length);
		}
		if (rest.length === 0) return;
		this.stderrTail.push(rest);
		this.stderrTailBytes += rest.length;
		while (this.stderrTailBytes > STDERR_TAIL_MAX_BYTES) {
			const oldest = this.stderrTail[0];
			const excess = this.stderrTailBytes - STDERR_TAIL_MAX_BYTES;
			if (oldest.length <= excess) {
				this.stderrTail.shift();
				this.stderrTailBytes -= oldest.length;
				this.stderrDroppedBytes += oldest.length;
			} else {
				// Trim within the chunk so the bound holds for any chunking.
				this.stderrTail[0] = oldest.subarray(excess);
				this.stderrTailBytes -= excess;
				this.stderrDroppedBytes += excess;
			}
		}
	}

	stderrText(): string {
		const head = Buffer.concat(this.stderrHead).toString("utf8");
		const tail = Buffer.concat(this.stderrTail).toString("utf8");
		const gap =
			this.stderrDroppedBytes > 0
				? `\n... [${this.stderrDroppedBytes} bytes of sidecar stderr dropped; ` +
					`retained the first ${STDERR_HEAD_MAX_BYTES} and last ` +
					`${STDERR_TAIL_MAX_BYTES} bytes; the full output was forwarded ` +
					`to host stderr] ...\n`
				: "";
		return `${head}${gap}${tail}`.trim();
	}

	currentExitError(): SidecarProcessExited | null {
		if (this.child.exitCode === null && this.child.signalCode === null) {
			return null;
		}
		return new SidecarProcessExited({
			exitCode: this.child.exitCode,
			signal: this.child.signalCode,
			stderr: this.stderrText(),
		});
	}

	waitForExit(timeoutMs: number): Promise<number | null> {
		return new Promise<number | null>((resolve) => {
			let timer: ReturnType<typeof setTimeout> | null = null;
			const cleanup = () => {
				this.child.off("exit", onExit);
				this.child.off("close", onClose);
				if (timer !== null) {
					clearTimeout(timer);
					timer = null;
				}
			};
			const onExit = (code: number | null) => {
				cleanup();
				resolve(code);
			};
			const onClose = (code: number | null) => {
				cleanup();
				resolve(code);
			};
			if (this.child.exitCode !== null || this.child.signalCode !== null) {
				resolve(this.child.exitCode);
				return;
			}
			this.child.on("exit", onExit);
			this.child.on("close", onClose);
			timer = setTimeout(() => {
				cleanup();
				resolve(null);
			}, timeoutMs);
		});
	}
}

function requireControlStream(child: ChildProcessWithoutNullStreams): Duplex {
	const stream = child.stdio[3];
	if (
		!stream ||
		typeof (stream as Duplex).write !== "function" ||
		typeof (stream as Duplex).on !== "function" ||
		typeof (stream as Duplex).read !== "function"
	) {
		throw new Error("sidecar process did not expose a full-duplex fd 3");
	}
	return stream as Duplex;
}
