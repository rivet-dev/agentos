import { useSuspenseQuery } from "@tanstack/react-query";
import type { SessionStreamEntry } from "@rivet-dev/agentos-core";
import { useEffect, useRef, useState } from "react";
import { AgentOsEmpty, StatusDot } from "../common";
import { cn } from "../lib/cn";
import { useAgentOsActor } from "../lib/rivet";
import { agentOsSource, mapSessionEvent } from "../lib/source";
import type { TranscriptEvent } from "../lib/types";
import { ScrollArea } from "../ui/scroll-area";
import React from "react";

function EventFrame({ label, meta, children }: { label: string; meta?: string; children: React.ReactNode }) {
	return (
		<div className="border-b px-4 py-3">
			<div className="mb-1 flex items-center gap-2">
				<span className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">{label}</span>
				{meta ? <span className="ml-auto font-mono text-xs text-muted-foreground">{meta}</span> : null}
			</div>
			{children}
		</div>
	);
}

function TranscriptEventView({ event }: { event: TranscriptEvent }) {
	switch (event.kind) {
		case "user":
		case "assistant":
			return (
				<EventFrame label={event.kind}>
					<p className="whitespace-pre-wrap text-sm">{event.text || "—"}</p>
				</EventFrame>
			);
		case "thinking":
			return (
				<EventFrame label="thinking">
					<p className="whitespace-pre-wrap text-sm italic text-muted-foreground">{event.text || "—"}</p>
				</EventFrame>
			);
		case "tool":
			return (
				<EventFrame label={`tool · ${event.tool}`} meta={event.status}>
					<span className="font-mono text-xs text-muted-foreground">{event.status ?? ""}</span>
				</EventFrame>
			);
		default:
			return (
				<EventFrame label={event.label}>
					<pre className="whitespace-pre-wrap break-words rounded bg-muted/50 p-2 font-mono text-[11px] leading-relaxed text-muted-foreground">
						{JSON.stringify(event.json, null, 2)}
					</pre>
				</EventFrame>
			);
	}
}

export function TranscriptTabConnected({ actorId }: { actorId: string }) {
	const { data: sessions } = useSuspenseQuery(agentOsSource.sessionsQueryOptions(actorId));
	const [selected, setSelected] = useState<string | null>(null);
	const sessionId = selected ?? sessions[0]?.sessionId ?? null;
	// Live-only transcript via the actor event stream.
	const actor = useAgentOsActor();
	const useAgentEvent = actor.useEvent as (
		name: string,
		handler: (event: SessionStreamEntry) => void,
	) => void;
	const [live, setLive] = useState<TranscriptEvent[]>([]);
	// Keep the latest session id in a ref so the event handler never filters
	// against a stale selection.
	const sessionIdRef = useRef(sessionId);
	sessionIdRef.current = sessionId;
	useEffect(() => {
		setLive([]);
	}, [sessionId]);
	useAgentEvent("sessionEvent", (event) => {
		const cur = sessionIdRef.current;
		if (!cur) return;
		if (event.sessionId !== cur) return;
		setLive((prev) => [...prev, mapSessionEvent(event)]);
	});
	return (
		<div className="flex h-full min-h-0">
			<div className="flex h-full w-64 shrink-0 flex-col border-r">
				<div className="border-b px-3 py-2.5">
					<div className="text-sm font-semibold">Sessions</div>
					<div className="text-[11px] uppercase tracking-wider text-muted-foreground">
						{sessions.length} session{sessions.length === 1 ? "" : "s"}
					</div>
				</div>
				<ScrollArea className="min-h-0 flex-1">
					{sessions.length === 0 ? (
						<AgentOsEmpty>No sessions yet.</AgentOsEmpty>
					) : (
						<div className="p-1.5">
							{sessions.map((s) => (
								<button
									key={s.sessionId}
									type="button"
									onClick={() => setSelected(s.sessionId)}
									className={cn(
										"flex w-full items-center gap-2 rounded px-2 py-2 text-left",
										s.sessionId === sessionId ? "bg-muted" : "hover:bg-muted/50",
									)}
								>
									<StatusDot color="green" />
									<div className="min-w-0 flex-1">
										<div className="truncate font-mono text-xs">{s.sessionId}</div>
										<div className="text-[11px] text-muted-foreground">{s.agentType}</div>
									</div>
								</button>
							))}
						</div>
					)}
				</ScrollArea>
			</div>
			<div className="flex min-h-0 flex-1 flex-col">
				{!sessionId ? (
					<AgentOsEmpty>Select a session to view its transcript.</AgentOsEmpty>
				) : (
					<>
						<div className="border-b px-4 py-3">
							<div className="font-mono text-sm">{sessionId}</div>
							<div className="text-xs text-muted-foreground">
								{live.length} event{live.length === 1 ? "" : "s"}
							</div>
						</div>
						<ScrollArea className="min-h-0 flex-1">
							{live.length === 0 ? (
								<AgentOsEmpty>Events appear here while this tab is connected.</AgentOsEmpty>
							) : (
								live.map((event, index) => (
									<TranscriptEventView key={index} event={event} />
								))
							)}
						</ScrollArea>
					</>
				)}
			</div>
		</div>
	);
}
