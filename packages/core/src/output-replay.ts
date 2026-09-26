import type { OutputReplay, ProcessOutputEvent } from "./language-execution.js";

export const REPLAY_PAGE_EVENT_LIMIT = 256;
export const REPLAY_PAGE_BYTE_LIMIT = 768 * 1024;
const MAX_WIRE_PAGE_VALUE = 0xffff_ffff;

export interface ReplayReadOptions {
	after?: number;
	maxEvents?: number;
	maxBytes?: number;
}

/** Zero is a wire-only omission sentinel, never an accepted public bound. */
export function replayWirePageLimits(options: ReplayReadOptions): {
	maxEvents: number;
	maxBytes: number;
} {
	replayPageLimits(options);
	return { maxEvents: options.maxEvents ?? 0, maxBytes: options.maxBytes ?? 0 };
}

function limitError(
	field: string,
	requested: number,
	limit: number,
	message?: string,
): Error {
	return Object.assign(
		new Error(
			message ??
				`process.readOutput ${field} requested ${requested}; supported range is 1..=${limit}`,
		),
		{
			code: "ERR_AGENTOS_RESOURCE_LIMIT",
			operation: "process.output.read",
			limitName:
				field === "maxBytes"
					? "output_replay_page_bytes"
					: "output_replay_page_events",
			configuredLimit: limit,
			requested,
			configurationPath: field,
			unit: field === "maxBytes" ? "bytes" : "events",
			scope: "vm",
			retryable: message !== undefined,
		},
	);
}

export function replayPageLimits(options: ReplayReadOptions): {
	maxEvents: number;
	maxBytes: number;
} {
	if (
		options.after !== undefined &&
		(!Number.isSafeInteger(options.after) || options.after < 0)
	) {
		throw Object.assign(
			new Error(
				"invalid_input: output cursor must be a non-negative safe integer",
			),
			{ code: "invalid_input" },
		);
	}
	const maxEvents = options.maxEvents ?? REPLAY_PAGE_EVENT_LIMIT;
	const maxBytes = options.maxBytes ?? REPLAY_PAGE_BYTE_LIMIT;
	for (const [field, value] of [
		["maxEvents", maxEvents],
		["maxBytes", maxBytes],
	] as const) {
		if (
			!Number.isSafeInteger(value) ||
			value < 1 ||
			value > MAX_WIRE_PAGE_VALUE
		)
			throw limitError(field, value, MAX_WIRE_PAGE_VALUE);
	}
	return { maxEvents, maxBytes };
}

export function replayPage(
	pid: number,
	source: readonly ProcessOutputEvent[],
	options: ReplayReadOptions,
	truncatedBefore?: number,
	sourceHasMore = false,
): OutputReplay {
	const { maxEvents, maxBytes } = replayPageLimits(options);
	const events: ProcessOutputEvent[] = [];
	let bytes = 0;
	let hasMore = sourceHasMore;
	let nextCursor = options.after ?? null;
	let gap = truncatedBefore;
	for (const event of source) {
		if (options.after !== undefined && event.sequence <= options.after)
			continue;
		if (
			events.length === maxEvents ||
			bytes + event.chunk.byteLength > maxBytes
		) {
			if (events.length === 0)
				throw limitError(
					"maxBytes",
					event.chunk.byteLength,
					maxBytes,
					`process.output.read next retained event requires ${event.chunk.byteLength} bytes, exceeding maxBytes=${maxBytes}; raise maxBytes to at least ${event.chunk.byteLength}`,
				);
			hasMore = true;
			break;
		}
		bytes += event.chunk.byteLength;
		events.push({ ...event, chunk: event.chunk.slice() });
		nextCursor = event.sequence;
	}
	if (!hasMore && gap !== undefined)
		nextCursor = Math.max(nextCursor ?? -1, gap);
	return {
		pid,
		exitCode: null,
		events,
		nextCursor,
		hasMore,
		truncated:
			gap !== undefined &&
			gap >= (options.after === undefined ? 0 : options.after + 1),
	};
}
