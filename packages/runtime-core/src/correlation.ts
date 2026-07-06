interface PendingResponse<TResponse> {
	resolve: (frame: TResponse) => void;
	reject: (error: Error) => void;
	timer: ReturnType<typeof setTimeout>;
}

export class PendingResponseRegistry<TResponse> {
	private readonly pending = new Map<number, PendingResponse<TResponse>>();

	waitForResponse(
		requestId: number,
		options: {
			timeoutMs: number;
			timeoutMessage: () => string;
		},
	): Promise<TResponse> {
		if (this.pending.has(requestId)) {
			throw new Error(
				`response waiter already registered for request ${requestId}`,
			);
		}
		return new Promise<TResponse>((resolve, reject) => {
			const entry = {
				resolve: (frame: TResponse) => {
					clearTimeout(entry.timer);
					this.pending.delete(requestId);
					resolve(frame);
				},
				reject: (error: Error) => {
					clearTimeout(entry.timer);
					this.pending.delete(requestId);
					reject(error);
				},
				timer: setTimeout(() => {
					this.pending.delete(requestId);
					reject(new Error(options.timeoutMessage()));
				}, options.timeoutMs),
			};
			this.pending.set(requestId, entry);
		});
	}

	resolve(requestId: number, frame: TResponse): boolean {
		const pending = this.pending.get(requestId);
		if (!pending) {
			return false;
		}
		pending.resolve(frame);
		return true;
	}

	reject(requestId: number, error: Error): boolean {
		const pending = this.pending.get(requestId);
		if (!pending) {
			return false;
		}
		pending.reject(error);
		return true;
	}

	rejectAll(error: Error): void {
		for (const pending of this.pending.values()) {
			pending.reject(error);
		}
		this.pending.clear();
	}
}
