import { useQuery, useQueryClient, useSuspenseQuery } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { AgentOsEmpty, relativeTime, StatusDot } from "../common";
import { isInspectorActionError } from "../lib/actor-client";
import { cn } from "../lib/cn";
import { cancelPrompt, liveSessionsQueryOptions } from "../lib/health";
import { useAgentOsActor } from "../lib/rivet";
import { agentOsSource, mapNotification } from "../lib/source";
import type { JsonRpcNotification, SessionEventPayload, TranscriptEvent } from "../lib/types";
import { ScrollArea } from "../ui/scroll-area";
import React from "react";

// Chat-style transcript rendering. Conversation content (user/assistant) gets
// bubbles; plumbing (usage updates, unknown ACP events) collapses to one-line
// chips with the raw JSON behind an expander so it never dominates the pane.
function TranscriptEventView({ event }: { event: TranscriptEvent }) {
	switch (event.kind) {
		case "user":
			return (
				<div className="flex justify-end">
					<div className="max-w-[80%] whitespace-pre-wrap rounded-2xl rounded-br-md bg-muted px-3.5 py-2 text-sm">
						{event.text || "—"}
					</div>
				</div>
			);
		case "assistant":
			return (
				<div className="flex">
					<div className="max-w-[85%] whitespace-pre-wrap text-sm leading-relaxed">
						{event.text || "—"}
					</div>
				</div>
			);
		case "thinking":
			return (
				<div className="flex">
					<div className="max-w-[85%] whitespace-pre-wrap text-xs italic leading-relaxed text-muted-foreground/70">
						{event.text || "—"}
					</div>
				</div>
			);
		case "tool":
			return (
				<div className="flex items-center gap-2 text-xs text-muted-foreground">
					<span aria-hidden="true">⚙</span>
					<span className="font-mono">{event.tool}</span>
					{event.status ? (
						<span className="rounded-full border px-1.5 py-px text-[10px] uppercase tracking-wide">
							{event.status}
						</span>
					) : null}
				</div>
			);
		case "error":
			return (
				<div className="whitespace-pre-wrap text-sm text-destructive">{event.text}</div>
			);
		default:
			return (
				<details className="group text-xs text-muted-foreground/70">
					<summary className="flex cursor-pointer list-none items-center gap-1.5 [&::-webkit-details-marker]:hidden">
						<span className="transition-transform group-open:rotate-90" aria-hidden="true">
							▸
						</span>
						<span className="font-mono">{event.label}</span>
					</summary>
					<pre className="mt-1.5 max-h-48 overflow-y-auto whitespace-pre-wrap break-words rounded bg-muted/50 p-2 font-mono text-[11px] leading-relaxed">
						{JSON.stringify(event.json, null, 2)}
					</pre>
				</details>
			);
	}
}

// Pins the transcript to the newest event whenever one arrives.
function ScrollAnchor({ count }: { count: number }) {
	const ref = useRef<HTMLDivElement>(null);
	useEffect(() => {
		ref.current?.scrollIntoView({ block: "end" });
	}, [count]);
	return <div ref={ref} />;
}

// Composer defaults persist in localStorage: they contain the API key the
// user pastes, which never leaves the browser except inside createSession's
// env (sent only to this actor).
const LS_AGENT_TYPE = "agentos-inspector:composer-agent-type";
const LS_ENV_JSON = "agentos-inspector:composer-env";
const DEFAULT_ENV_JSON = JSON.stringify(
	{
		ANTHROPIC_API_KEY: "",
		OPENCODE_CONFIG_CONTENT: JSON.stringify({ model: "anthropic/claude-haiku-4-5-20251001" }),
	},
	null,
	2,
);

function Composer({
	sessionId,
	sessionStatus,
	onSessionCreated,
	onErrorEvent,
}: {
	sessionId: string | null;
	sessionStatus?: string;
	onSessionCreated: (sessionId: string) => void;
	onErrorEvent: (text: string) => void;
}) {
	const [draft, setDraft] = useState("");
	const [busy, setBusy] = useState(false);
	const [stopUnsupported, setStopUnsupported] = useState(false);
	const [optionsOpen, setOptionsOpen] = useState(false);
	const [agentType, setAgentType] = useState(
		() => localStorage.getItem(LS_AGENT_TYPE) ?? "opencode",
	);
	const [envJson, setEnvJson] = useState(() => localStorage.getItem(LS_ENV_JSON) ?? DEFAULT_ENV_JSON);
	// The in-flight session survives re-renders; also used by Stop.
	const activeSessionRef = useRef<string | null>(null);

	const send = async () => {
		const text = draft.trim();
		if (!text || busy) return;
		setBusy(true);
		setDraft("");
		try {
			let sid = sessionId;
			if (!sid) {
				let env: Record<string, string>;
				try {
					env = JSON.parse(envJson);
				} catch (e) {
					throw new Error(`Session env is not valid JSON: ${(e as Error).message}`);
				}
				const raw = await agentOsSource.createSession(agentType.trim() || "opencode", { env });
				sid = typeof raw === "string" ? raw : raw.sessionId;
				onSessionCreated(sid);
			}
			activeSessionRef.current = sid;
			await agentOsSource.sendPrompt(sid, text);
		} catch (error) {
			let hint = isInspectorActionError(error) ? ` — ${error.hint}` : "";
			// Prompting a persisted-but-idle session (from an earlier VM boot)
			// fails inside the runtime; the generic crash hint is misleading there.
			if (sessionId && sessionStatus !== "running") {
				hint =
					" — this session is idle (not live in the current VM boot). Send without a selection to start a fresh session, or resume it from a client.";
			}
			onErrorEvent(`${error instanceof Error ? error.message : String(error)}${hint}`);
		} finally {
			activeSessionRef.current = null;
			setBusy(false);
		}
	};

	const stop = async () => {
		const sid = activeSessionRef.current ?? sessionId;
		if (!sid) return;
		try {
			const supported = await cancelPrompt(sid);
			if (!supported) {
				setStopUnsupported(true);
				onErrorEvent("Stop is not supported by this runtime (no cancelPrompt action).");
			}
		} catch (error) {
			onErrorEvent(error instanceof Error ? error.message : String(error));
		}
	};

	return (
		<div className="shrink-0">
			<div className="mx-auto w-full max-w-3xl px-3 pb-3">
				{optionsOpen ? (
					<div className="mb-2 flex flex-col gap-2 rounded-xl border bg-muted/30 p-3 text-xs">
						<label className="flex items-center gap-2">
							<span className="w-24 shrink-0 text-muted-foreground">Agent type</span>
							<input
								value={agentType}
								onChange={(e) => {
									setAgentType(e.target.value);
									localStorage.setItem(LS_AGENT_TYPE, e.target.value);
								}}
								className="flex-1 rounded border bg-background px-2 py-1 font-mono"
							/>
						</label>
						<label className="flex items-start gap-2">
							<span className="w-24 shrink-0 pt-1 text-muted-foreground">Session env</span>
							<textarea
								value={envJson}
								onChange={(e) => {
									setEnvJson(e.target.value);
									localStorage.setItem(LS_ENV_JSON, e.target.value);
								}}
								rows={4}
								spellCheck={false}
								className="flex-1 resize-y rounded border bg-background px-2 py-1 font-mono"
							/>
						</label>
						<div className="text-muted-foreground/70">
							Used when sending without a session: creates one with these options. Values stay
							in this browser.
						</div>
					</div>
				) : null}
				{/* Single-card composer (ChatGPT/Codex style): borderless textarea on
				    top, inline controls in the same card, circular send/stop. */}
				<div className="rounded-2xl border bg-muted/40 transition-colors focus-within:border-ring/60">
					<textarea
						value={draft}
						onChange={(e) => setDraft(e.target.value)}
						onKeyDown={(e) => {
							if (e.key === "Enter" && !e.shiftKey) {
								e.preventDefault();
								void send();
							}
						}}
						placeholder={sessionId ? "Prompt this session…" : "Prompt a new session…"}
						rows={2}
						className="max-h-48 min-h-[3.25rem] w-full resize-none bg-transparent px-4 pt-3 text-sm placeholder:text-muted-foreground/50 focus:outline-none"
					/>
					<div className="flex items-center gap-1.5 px-2.5 pb-2.5">
						<button
							type="button"
							onClick={() => setOptionsOpen((v) => !v)}
							title="Session options"
							className="inline-flex items-center gap-1 rounded px-2.5 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
						>
							<span aria-hidden="true" className="text-sm leading-none">
								+
							</span>
							Options
						</button>
						<span className="ml-auto" />
						<span className="px-1 font-mono text-[11px] text-muted-foreground/60">
							{agentType.trim() || "opencode"}
						</span>
						{busy ? (
							<button
								type="button"
								onClick={() => void stop()}
								disabled={stopUnsupported}
								title={
									stopUnsupported
										? "This runtime has no cancelPrompt action"
										: "Stop the running turn"
								}
								className="inline-flex size-8 items-center justify-center rounded-md bg-foreground/90 text-background transition-opacity hover:opacity-80 disabled:opacity-40"
							>
								<span aria-hidden="true" className="block size-2.5 rounded-[2px] bg-current" />
							</button>
						) : (
							<button
								type="button"
								onClick={() => void send()}
								disabled={!draft.trim()}
								title="Send (⏎)"
								className="inline-flex size-8 items-center justify-center rounded-md bg-primary text-primary-foreground transition-opacity hover:opacity-90 disabled:opacity-40"
							>
								<svg
									viewBox="0 0 16 16"
									className="size-4"
									fill="none"
									stroke="currentColor"
									strokeWidth="2"
									strokeLinecap="round"
									strokeLinejoin="round"
									aria-hidden="true"
								>
									<path d="M8 13V3M4 7l4-4 4 4" />
								</svg>
							</button>
						)}
					</div>
				</div>
			</div>
		</div>
	);
}

export function TranscriptTabConnected({ actorId }: { actorId: string }) {
	const { data: sessions } = useSuspenseQuery(agentOsSource.sessionsQueryOptions(actorId));
	const queryClient = useQueryClient();
	const [selected, setSelected] = useState<string | null>(null);
	const sessionId = selected ?? sessions[0]?.sessionId ?? null;
	// Liveness: cross-reference listSessions (records carry no status on the
	// current runtime); fall back to the record's own status where it exists.
	const { data: liveIds } = useQuery(liveSessionsQueryOptions(actorId));
	const isLive = (s: (typeof sessions)[number]) =>
		liveIds ? liveIds.has(s.sessionId) : s.status === "running";
	// Persisted history (one-off) + live tail (websocket `sessionEvent` stream).
	const { data: backfill = [] } = useQuery(agentOsSource.transcriptQueryOptions(actorId, sessionId));

	// Live tail via the typed useActor connection. `sessionEvent` isn't in the
	// actor's TS event schema (Rust owns broadcasts), so `useEvent` is typed with
	// `never` event names — cast to subscribe to the real runtime event.
	const actor = useAgentOsActor();
	const useAgentEvent = actor.useEvent as (
		name: string,
		handler: (payload: SessionEventPayload) => void,
	) => void;
	const [live, setLive] = useState<TranscriptEvent[]>([]);
	// Synthetic monotonic seq (broadcasts carry none); latest sessionId via ref so
	// the (ref-stable) event handler never filters against a stale value.
	const seqRef = useRef(0);
	const sessionIdRef = useRef(sessionId);
	sessionIdRef.current = sessionId;
	useEffect(() => {
		setLive([]);
		seqRef.current = 0;
	}, [sessionId]);
	useAgentEvent("sessionEvent", (payload) => {
		const cur = sessionIdRef.current;
		if (!cur) return;
		// Broadcast may omit sessionId; when present, keep only this session's.
		if (payload?.sessionId && payload.sessionId !== cur) return;
		const notification: JsonRpcNotification | undefined =
			payload?.event ?? (payload as unknown as JsonRpcNotification);
		if (!notification) return;
		setLive((prev) => [...prev, mapNotification(notification, seqRef.current++)]);
	});
	// Composer callbacks: failed turns render as error rows in the live stream
	// (fixes invisible failures); new sessions get selected + the list refreshed.
	const pushErrorEvent = (text: string) =>
		setLive((prev) => [...prev, { kind: "error", seq: seqRef.current++, text }]);
	const handleSessionCreated = (sid: string) => {
		setSelected(sid);
		void queryClient.invalidateQueries({
			queryKey: agentOsSource.sessionsQueryOptions(actorId).queryKey,
		});
	};
	return (
		<div className="flex h-full min-h-0">
			<div className="flex h-full w-56 shrink-0 flex-col border-r">
				<div className="px-3 pb-1 pt-2.5 text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
					Sessions
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
										"flex w-full items-center gap-2 rounded px-2 py-1.5 text-left",
										s.sessionId === sessionId ? "bg-muted" : "hover:bg-muted/50",
									)}
								>
									<StatusDot color={isLive(s) ? "green" : "muted"} />
									<div className="min-w-0 flex-1" title={isLive(s) ? "Live in the current VM" : "Idle — persisted, not loaded in the current VM"}>
										<div className="truncate text-xs">
											{s.agentType} · {relativeTime(s.createdAt)}
										</div>
										<div className="truncate font-mono text-[10px] text-muted-foreground/60">
											…{s.sessionId.slice(-10)}
										</div>
									</div>
								</button>
							))}
						</div>
					)}
				</ScrollArea>
			</div>
			<div className="flex min-h-0 flex-1 flex-col">
				{!sessionId ? (
					<AgentOsEmpty>
						<span>
							No session selected — send a prompt below to start one.
							<br />
							<span className="text-xs text-muted-foreground/70">
								Sessions and their transcripts persist, but the VM's root filesystem is
								in-memory: files do not survive VM restarts.
							</span>
						</span>
					</AgentOsEmpty>
				) : (
					<>
						<ScrollArea className="min-h-0 flex-1">
							{backfill.length + live.length === 0 ? (
								<AgentOsEmpty>No activity yet — send a prompt below.</AgentOsEmpty>
							) : (
								<div className="mx-auto flex w-full max-w-3xl flex-col gap-3 px-4 py-4">
									{backfill.map((e) => (
										<TranscriptEventView key={`b-${e.seq}`} event={e} />
									))}
									{live.map((e) => (
										<TranscriptEventView key={`l-${e.seq}`} event={e} />
									))}
									<ScrollAnchor count={backfill.length + live.length} />
								</div>
							)}
						</ScrollArea>
					</>
				)}
				<Composer
					sessionId={sessionId}
					sessionStatus={
						sessionId
							? (() => {
									const record = sessions.find((s) => s.sessionId === sessionId);
									return record && isLive(record) ? "running" : "idle";
								})()
							: undefined
					}
					onSessionCreated={handleSessionCreated}
					onErrorEvent={pushErrorEvent}
				/>
			</div>
		</div>
	);
}
