export const DEFAULT_HOST_CAPTURE_LIMIT_BYTES = 16 * 1024 * 1024;

export class CapturedOutputLimitError extends Error {
	readonly code = "ERR_AGENTOS_CAPTURED_OUTPUT_LIMIT";

	constructor(
		readonly limitBytes: number,
		configuration: string,
	) {
		super(`Captured output exceeded ${limitBytes} bytes; ${configuration}`);
		this.name = "CapturedOutputLimitError";
	}
}

/** Budget host-owned capture independently of the sidecar's output policy. */
export class HostOutputCaptureBudget {
	private error: CapturedOutputLimitError | undefined;
	private bytes = 0;
	private warned = false;

	constructor(
		readonly limit: number,
		private readonly configuration: string,
	) {
		if (!Number.isSafeInteger(limit) || limit < 1) {
			throw new RangeError(
				"maxCapturedOutputBytes must be a positive safe integer",
			);
		}
	}

	append(bytes: number): CapturedOutputLimitError | undefined {
		if (this.error) return this.error;
		if (bytes > this.limit - this.bytes) {
			this.error = new CapturedOutputLimitError(this.limit, this.configuration);
			return this.error;
		}
		this.bytes += bytes;
		if (!this.warned && this.bytes >= this.limit * 0.8) {
			this.warned = true;
			console.warn(
				`[agentOS] captured output reached ${this.bytes} of ${this.limit} bytes; ${this.configuration}`,
			);
		}
		return undefined;
	}

	check(): void {
		if (this.error) throw this.error;
	}
}

export class HostTextOutputCapture {
	private readonly budget: HostOutputCaptureBudget;
	private readonly decoders = {
		stdout: new TextDecoder(),
		stderr: new TextDecoder(),
	};
	private readonly text = { stdout: "", stderr: "" };

	constructor(limit = DEFAULT_HOST_CAPTURE_LIMIT_BYTES) {
		this.budget = new HostOutputCaptureBudget(
			limit,
			"raise maxCapturedOutputBytes",
		);
	}

	/** Decode each stream separately, and keep streaming after capture fills. */
	append(stream: "stdout" | "stderr", chunk: Uint8Array): string {
		const error = this.budget.append(chunk.byteLength);
		const text = this.decoders[stream].decode(chunk, { stream: true });
		if (!error) this.text[stream] += text;
		return text;
	}

	finish(): { stdout: string; stderr: string } {
		this.budget.check();
		return {
			stdout: this.text.stdout + this.decoders.stdout.decode(),
			stderr: this.text.stderr + this.decoders.stderr.decode(),
		};
	}
}
