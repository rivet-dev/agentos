import type { LiveOwnershipScope } from "./ownership.js";
import type { LiveResponseFrame } from "./protocol-frames.js";

function formatSidecarStderrSuffix(stderr: string): string {
	return stderr ? `\nstderr:\n${stderr}` : "";
}

/** A sidecar-owned request failure, preserving its stable code and wire context. */
export class SidecarRequestRejected extends Error {
	readonly code: string;
	readonly requestId: number;
	readonly ownership: LiveOwnershipScope;
	readonly response: LiveResponseFrame;

	constructor(options: {
		code: string;
		message: string;
		response: LiveResponseFrame;
	}) {
		super(options.message);
		this.name = "SidecarRequestRejected";
		this.code = options.code;
		this.requestId = options.response.request_id;
		this.ownership = options.response.ownership;
		this.response = options.response;
	}
}

export class SidecarProcessExited extends Error {
	readonly exitCode: number | null;
	readonly signal: string | null;
	readonly stderr: string;

	constructor(options: {
		exitCode: number | null;
		signal: string | null;
		stderr: string;
	}) {
		const reason =
			options.signal !== null
				? `signal ${options.signal}`
				: options.exitCode !== null
					? `code ${options.exitCode}`
					: "disconnect";
		super(
			`sidecar process exited with ${reason}${formatSidecarStderrSuffix(options.stderr)}`,
		);
		this.name = "SidecarProcessExited";
		this.exitCode = options.exitCode;
		this.signal = options.signal;
		this.stderr = options.stderr;
	}
}

/**
 * The silence watchdog fired: the sidecar produced no protocol frames at all —
 * not even its 10s liveness heartbeats — for the full silence window, so the
 * process is dead or wedged (not merely busy: a busy sidecar still heartbeats
 * from a dedicated thread). The host kills the sidecar and rejects every
 * in-flight request with this error.
 */
export class SidecarSilenceTimeout extends Error {
	readonly silenceMs: number;
	readonly stderr: string;

	constructor(options: { silenceMs: number; stderr: string }) {
		super(
			`sidecar unresponsive: no protocol frames or heartbeats for ${Math.round(options.silenceMs)}ms; killing sidecar${formatSidecarStderrSuffix(options.stderr)}`,
		);
		this.name = "SidecarSilenceTimeout";
		this.silenceMs = options.silenceMs;
		this.stderr = options.stderr;
	}
}

export class SidecarProcessError extends Error {
	readonly childError: Error;
	readonly stderr: string;

	constructor(error: Error, stderr: string) {
		super(
			`sidecar process error: ${error.message}${formatSidecarStderrSuffix(stderr)}`,
		);
		this.name = "SidecarProcessError";
		this.childError = error;
		this.stderr = stderr;
	}
}
