function formatSidecarStderrSuffix(stderr: string): string {
	return stderr ? `\nstderr:\n${stderr}` : "";
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
