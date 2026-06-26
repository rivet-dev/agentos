// React hook over the rivetkit actor connection: live `sessionEvent` stream for
// one session, mapped to display transcript events. Persisted history is loaded
// separately (`transcriptQueryOptions`); this returns only the events that
// arrive AFTER the tab subscribes, which the transcript tab appends after the
// backfill.
import { useEffect, useRef, useState } from "react";
import { subscribeSessionEvents } from "./actor-client";
import { mapNotification } from "./source";
import type { JsonRpcNotification, SessionEventPayload, TranscriptEvent } from "./types";

export function useLiveSessionEvents(sessionId: string | null): TranscriptEvent[] {
	const [events, setEvents] = useState<TranscriptEvent[]>([]);
	// Monotonic synthetic seq for live events (broadcasts carry no `seq`). Only
	// used for ordering/keys; the transcript namespaces live keys so these never
	// collide with the persisted backfill's real seqs.
	const seqRef = useRef(0);

	useEffect(() => {
		setEvents([]);
		seqRef.current = 0;
		if (!sessionId) return;
		return subscribeSessionEvents((payload: SessionEventPayload) => {
			// The broadcast may omit `sessionId`; when present, keep only this
			// session's events. When absent we accept all (the tab watches one
			// session at a time).
			if (payload?.sessionId && payload.sessionId !== sessionId) return;
			const notification: JsonRpcNotification | undefined =
				payload?.event ?? (payload as unknown as JsonRpcNotification);
			if (!notification) return;
			const mapped = mapNotification(notification, seqRef.current++);
			setEvents((prev) => [...prev, mapped]);
		});
	}, [sessionId]);

	return events;
}
