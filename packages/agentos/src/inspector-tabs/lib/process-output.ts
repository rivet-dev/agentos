import type { Output } from "../../generated/contract";
import { decodeActionBytes, PROCESS_REPLAY_PAGE_LIMITS } from "./source";

const MAX_REPLAY_PAGES_PER_POLL = 8;

/** Drain a bounded portion of retained process output before the next UI poll. */
export async function drainProcessReplay(
	read: (after?: number | bigint) => Promise<Output.ActorOutputReplay>,
	generation: number | bigint,
	after?: number | bigint,
	signal?: AbortSignal,
): Promise<{
	events: Output.ActorOutputEvent[];
	nextCursor?: number | bigint;
	hasMore: boolean;
	truncated: boolean;
	end?: Output.ActorExitStatus;
}> {
	const events: Output.ActorOutputEvent[] = [];
	let cursor = after;
	let hasMore = false;
	let truncated = false;
	let end: Output.ActorExitStatus | undefined;
	for (let pageIndex = 0; pageIndex < MAX_REPLAY_PAGES_PER_POLL; pageIndex++) {
		signal?.throwIfAborted();
		const page = await read(cursor);
		signal?.throwIfAborted();
		if (BigInt(page.generation) !== BigInt(generation)) {
			throw new Error("Process replay returned a different VM generation");
		}
		if (page.events.length > PROCESS_REPLAY_PAGE_LIMITS.maxEvents) {
			throw new Error("Process replay exceeded the requested event page limit");
		}
		let lastSequence = cursor;
		let pageBytes = 0;
		const normalizedEvents: Output.ActorOutputEvent[] = [];
		for (const event of page.events) {
			if (
				event.sequence < 0 ||
				(typeof event.sequence === "number" &&
					!Number.isSafeInteger(event.sequence)) ||
				(lastSequence !== undefined && event.sequence <= lastSequence)
			) {
				throw new Error("Process replay returned out-of-order sequences");
			}
			truncated ||= BigInt(event.sequence) > BigInt(lastSequence ?? -1) + 1n;
			lastSequence = event.sequence;
			const data = decodeActionBytes(event.data);
			pageBytes += data.byteLength;
			if (pageBytes > PROCESS_REPLAY_PAGE_LIMITS.maxBytes) {
				throw new Error(
					"Process replay exceeded the requested byte page limit",
				);
			}
			normalizedEvents.push({ ...event, data });
		}
		const nextCursor = page.nextCursor ?? undefined;
		if (
			(page.hasMore && page.events.length === 0) ||
			(page.events.length > 0 && nextCursor === undefined) ||
			(nextCursor !== undefined &&
				(lastSequence === undefined ||
					BigInt(nextCursor) !== BigInt(lastSequence)))
		) {
			throw new Error("Process replay did not advance its cursor");
		}
		events.push(...normalizedEvents);
		cursor = nextCursor ?? cursor;
		truncated ||= page.truncated;
		end = page.end ?? end;
		hasMore = page.hasMore;
		if (!hasMore) break;
	}
	return { events, nextCursor: cursor, hasMore, truncated, end };
}

/** stdout and stderr are separate UTF-8 streams, even when their events are
 * interleaved. Retain incomplete characters across both pages and UI polls. */
export class ProcessOutputDecoder {
	private streams = { stdout: new TextDecoder(), stderr: new TextDecoder() };
	private sequence: number | bigint | undefined;

	decode(event: Output.ActorOutputEvent): string {
		if (
			this.sequence !== undefined &&
			BigInt(event.sequence) !== BigInt(this.sequence) + 1n
		) {
			this.reset();
		}
		this.sequence = event.sequence;
		return this.streams[event.stream].decode(event.data, { stream: true });
	}

	reset(): void {
		this.streams = { stdout: new TextDecoder(), stderr: new TextDecoder() };
		this.sequence = undefined;
	}

	finish(): string {
		return this.streams.stdout.decode() + this.streams.stderr.decode();
	}
}
