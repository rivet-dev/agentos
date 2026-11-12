function formatSidecarStderrSuffix(stderr: string): string {
	return stderr ? `\nstderr:\n${stderr}` : "";
}

export interface SidecarRejectionDetail {
	code: string;
	message: string;
	limit_name: string | null;
	configured_limit: number | null;
	current_usage: number | null;
	requested: number | null;
	unit: string | null;
	scope: string | null;
	vm_id: string | null;
	session_generation: number | null;
	capability_id: number | null;
	operation: string | null;
	configuration_path: string | null;
	retryable: boolean | null;
	errno: string | null;
}

export class SidecarRejectedError extends Error {
	readonly requestId: number;
	readonly detail: SidecarRejectionDetail;

	constructor(requestId: number, detail: SidecarRejectionDetail) {
		super(
			`sidecar rejected request ${requestId}: ${detail.code}: ${detail.message}`,
		);
		this.name = "SidecarRejectedError";
		this.requestId = requestId;
		this.detail = detail;
	}
}

/**
 * A host frame exceeded the sidecar's negotiated `max_frame_bytes`. The client
 * rejects it before writing, so the sidecar and its other VMs keep running.
 */
export class SidecarFrameTooLarge extends Error {
	readonly frameBytes: number;
	readonly maxFrameBytes: number;

	constructor(options: { frameBytes: number; maxFrameBytes: number }) {
		super(
			`sidecar protocol frame is ${options.frameBytes} bytes, limit is ${options.maxFrameBytes} (sidecar max_frame_bytes); the request was not sent. Send less data per request, for example by writing large content in several smaller files or by mounting a host directory`,
		);
		this.name = "SidecarFrameTooLarge";
		this.frameBytes = options.frameBytes;
		this.maxFrameBytes = options.maxFrameBytes;
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
		cause?: Error;
	}) {
		const reason =
			options.signal !== null
				? `signal ${options.signal}`
				: options.exitCode !== null
					? `code ${options.exitCode}`
					: "disconnect";
		super(
			`sidecar process exited with ${reason}${formatSidecarStderrSuffix(options.stderr)}`,
			options.cause ? { cause: options.cause } : undefined,
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
