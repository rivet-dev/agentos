import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import type { Duplex } from "node:stream";

export {
	SidecarProcessError,
	SidecarProcessExited,
} from "./sidecar-errors.js";

import { SidecarProcessError, SidecarProcessExited } from "./sidecar-errors.js";

/**
 * Bounds on the sidecar stderr excerpt retained for {@link SidecarProcessExited}.
 * Live stderr is always forwarded to the host, so these bound only the
 * postmortem copy — they must never be unbounded, because the volume is
 * ultimately driven by guest-triggered sidecar diagnostics.
 */
const STDERR_HEAD_MAX_BYTES = 16 * 1024;
const STDERR_TAIL_MAX_BYTES = 48 * 1024;

export interface StdioSidecarProcessSpawnOptions {
	command: string;
	args?: string[];
	cwd?: string;
}

export class StdioSidecarProcess {
	readonly child: ChildProcessWithoutNullStreams;
	readonly control: Duplex;
	/** First bytes of sidecar stderr — the ROOT cause usually appears here. */
	private readonly stderrHead: Buffer[] = [];
	private stderrHeadBytes = 0;
	/** Most recent bytes — the FATAL error appears here. */
	private readonly stderrTail: Buffer[] = [];
	private stderrTailBytes = 0;
	private stderrDroppedBytes = 0;
	private readonly exitListeners = new Set<
		(error: SidecarProcessExited) => void
	>();
	private readonly errorListeners = new Set<
		(error: SidecarProcessError) => void
	>();

	private constructor(child: ChildProcessWithoutNullStreams, control: Duplex) {
		this.child = child;
		this.control = control;
		// Sidecar stderr is handled two ways, and both matter.
		//
		// FORWARD, always: buffering it and only replaying on exit means a LIVE
		// sidecar's warnings/errors are invisible to the host, so a guest-triggered
		// failure the sidecar SURVIVES leaves no host-visible trace at all. The
		// Rust client has always forwarded (it spawns with `Stdio::inherit()`), so
		// gating this behind a flag would also leave the two clients behaviorally
		// different, which the client-parity rule forbids.
		//
		// RETAIN, bounded: the retained copy exists only to populate
		// SidecarProcessExited.stderr for postmortem, so it needs a bounded
		// excerpt, not the whole history. An append-only buffer is a
		// guest-reachable host memory leak: a guest that can drive a high rate of
		// sidecar warnings would grow this without limit and OOM the host process.
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
		const child = spawn(options.command, options.args ?? [], {
			cwd: options.cwd,
			stdio: ["pipe", "pipe", "pipe", "pipe"],
		}) as unknown as ChildProcessWithoutNullStreams;
		try {
			return new StdioSidecarProcess(child, requireControlStream(child));
		} catch (error) {
			child.kill("SIGKILL");
			throw error;
		}
	}

	static fromChild(
		child: ChildProcessWithoutNullStreams,
		control?: Duplex,
	): StdioSidecarProcess {
		return new StdioSidecarProcess(
			child,
			control ?? requireControlStream(child),
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
	 * Retain a bounded excerpt: the first {@link STDERR_HEAD_MAX_BYTES} and the
	 * most recent {@link STDERR_TAIL_MAX_BYTES}. Both ends are kept because the
	 * root cause is typically the FIRST error while the fatal one is the LAST,
	 * and a tail-only buffer discards exactly the line that explains the crash.
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
				// Trim within the chunk rather than dropping it whole, so the
				// bound holds regardless of how the stream happens to chunk.
				this.stderrTail[0] = oldest.subarray(excess);
				this.stderrTailBytes -= excess;
				this.stderrDroppedBytes += excess;
			}
		}
	}

	stderrText(): string {
		const head = Buffer.concat(this.stderrHead).toString("utf8");
		const tail = Buffer.concat(this.stderrTail).toString("utf8");
		// Truncation is stated explicitly: a silently shortened diagnostic is
		// worse than a short one, because it reads as the complete output.
		const gap =
			this.stderrDroppedBytes > 0
				? `\n... [${this.stderrDroppedBytes} bytes of sidecar stderr dropped; ` +
					`raise STDERR_HEAD_MAX_BYTES/STDERR_TAIL_MAX_BYTES to retain more] ...\n`
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
