const MAX_PENDING_BYTES = 256 * 1024;
const MAX_PENDING_EVENTS = 2048;

interface PendingChunk {
	seq: number | bigint;
	bytes: Uint8Array;
}

interface PendingOutput {
	chunks: PendingChunk[];
	bytes: number;
	droppedThrough?: number | bigint;
}

interface AttachedOutput {
	write: (bytes: Uint8Array) => void;
	sequence: number | bigint;
	warned: boolean;
}

/** Buffers output during replay and keeps its watermark after attaching, since
 * broadcasts covered by a snapshot can arrive after the snapshot response. */
export class OutputRouter {
	private writers = new Map<string, AttachedOutput>();
	private pending = new Map<string, PendingOutput>();

	constructor(private readonly onError: (error: Error) => void) {}

	push(shellId: string, seq: number | bigint, bytes: Uint8Array): void {
		const attached = this.writers.get(shellId);
		if (attached) {
			this.deliver(attached, { seq, bytes });
			return;
		}
		const buffered: PendingOutput = this.pending.get(shellId) ?? {
			chunks: [],
			bytes: 0,
		};
		buffered.chunks.push({ seq, bytes });
		buffered.bytes += bytes.length;
		while (
			buffered.bytes > MAX_PENDING_BYTES ||
			buffered.chunks.length > MAX_PENDING_EVENTS
		) {
			const dropped = buffered.chunks.shift()!;
			buffered.bytes -= dropped.bytes.length;
			if (
				buffered.droppedThrough === undefined ||
				dropped.seq > buffered.droppedThrough
			) {
				buffered.droppedThrough = dropped.seq;
			}
		}
		this.pending.set(shellId, buffered);
	}

	attach(
		shellId: string,
		write: (bytes: Uint8Array) => void,
		sinceSeq: number | bigint = -1,
	): () => void {
		const attached = { write, sequence: sinceSeq, warned: false };
		this.writers.set(shellId, attached);
		const buffered = this.pending.get(shellId);
		if (buffered) {
			this.pending.delete(shellId);
			if (
				buffered.droppedThrough !== undefined &&
				buffered.droppedThrough > sinceSeq
			) {
				this.warn(
					attached,
					"Terminal output was truncated while replay was loading; the inspector buffer is limited to 256 KiB and 2048 events",
				);
			}
			for (const chunk of buffered.chunks) this.deliver(attached, chunk);
		}
		return () => {
			if (this.writers.get(shellId) === attached) this.writers.delete(shellId);
		};
	}

	forget(shellId: string): void {
		this.writers.delete(shellId);
		this.pending.delete(shellId);
	}

	private deliver(attached: AttachedOutput, chunk: PendingChunk): void {
		if (chunk.seq <= attached.sequence) return;
		if (BigInt(chunk.seq) > BigInt(attached.sequence) + 1n) {
			this.warn(
				attached,
				"Terminal live output has a sequence gap; reopen the terminal tab to replay retained output",
			);
		}
		attached.write(chunk.bytes);
		attached.sequence = chunk.seq;
	}

	private warn(attached: AttachedOutput, message: string): void {
		if (attached.warned) return;
		attached.warned = true;
		this.onError(new Error(message));
	}
}
