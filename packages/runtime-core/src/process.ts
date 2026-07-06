import {
	type ChildProcessWithoutNullStreams,
	spawn,
} from "node:child_process";
export {
	SidecarProcessError,
	SidecarProcessExited,
} from "./sidecar-errors.js";
import {
	SidecarProcessError,
	SidecarProcessExited,
} from "./sidecar-errors.js";

export interface StdioSidecarProcessSpawnOptions {
	command: string;
	args?: string[];
	cwd?: string;
}

export class StdioSidecarProcess {
	readonly child: ChildProcessWithoutNullStreams;
	private readonly stderrChunks: Buffer[] = [];
	private readonly exitListeners = new Set<(error: SidecarProcessExited) => void>();
	private readonly errorListeners = new Set<(error: SidecarProcessError) => void>();

	private constructor(child: ChildProcessWithoutNullStreams) {
		this.child = child;
		this.child.stderr.on("data", (chunk: Buffer | string) => {
			this.stderrChunks.push(
				typeof chunk === "string" ? Buffer.from(chunk) : Buffer.from(chunk),
			);
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
		return new StdioSidecarProcess(
			spawn(options.command, options.args ?? [], {
				cwd: options.cwd,
				stdio: ["pipe", "pipe", "pipe"],
			}),
		);
	}

	static fromChild(
		child: ChildProcessWithoutNullStreams,
	): StdioSidecarProcess {
		return new StdioSidecarProcess(child);
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

	stderrText(): string {
		return Buffer.concat(this.stderrChunks).toString("utf8").trim();
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
