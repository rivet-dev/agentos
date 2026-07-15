import { useQuery, useQueryClient, useSuspenseQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { AgentOsEmpty, ChevronRight, relativeTime, StatusDot } from "../common";
import { isInspectorActionError } from "../lib/actor-client";
import { cn } from "../lib/cn";
import { useAgentOsActor } from "../lib/rivet";
import {
	agentOsSource,
	cancelPrompt,
	liveSessionsQueryOptions,
	mapNotification,
} from "../lib/source";
import type {
	AgentCrashedPayload,
	JsonRpcNotification,
	PermissionRequestPayload,
	SessionEventPayload,
	TranscriptEvent,
} from "../lib/types";
import { ScrollArea } from "../ui/scroll-area";
import { VmStatusBadges } from "../vm-status-badges";
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
		case "tool": {
			const hasBody =
				event.input !== undefined || event.output !== undefined || !!event.locations?.length;
			const summary = (
				<>
					<span className="font-mono">{event.tool}</span>
					{event.status ? (
						<span className="rounded-full border px-1.5 py-px text-[10px]">{event.status}</span>
					) : null}
				</>
			);
			if (!hasBody) {
				return (
					<div className="flex items-center gap-2 text-xs text-muted-foreground">{summary}</div>
				);
			}
			return (
				<details className="group rounded-lg border border-foreground/10 bg-muted/20 text-xs text-muted-foreground">
					<summary className="flex cursor-pointer list-none items-center gap-2 px-2.5 py-1.5 [&::-webkit-details-marker]:hidden">
						<ChevronRight className="size-3 shrink-0 transition-transform group-open:rotate-90" />
						{summary}
					</summary>
					<div className="flex flex-col gap-2 border-t border-foreground/10 px-2.5 py-2">
						{event.input !== undefined ? (
							<div>
								<div className="mb-1 text-[10px] text-muted-foreground/70">Input</div>
								<pre className="max-h-40 overflow-y-auto whitespace-pre-wrap break-words rounded bg-muted/50 p-2 font-mono text-[11px] leading-relaxed">
									{JSON.stringify(event.input, null, 2)}
								</pre>
							</div>
						) : null}
						{event.output !== undefined ? (
							<div>
								<div className="mb-1 text-[10px] text-muted-foreground/70">Output</div>
								<pre className="max-h-48 overflow-y-auto whitespace-pre-wrap break-words rounded bg-muted/50 p-2 font-mono text-[11px] leading-relaxed">
									{event.output}
								</pre>
							</div>
						) : null}
						{event.locations?.length ? (
							<div className="font-mono text-[11px] text-muted-foreground/70">
								{event.locations.join("  ")}
							</div>
						) : null}
					</div>
				</details>
			);
		}
		case "plan":
			return (
				<div className="rounded-lg border border-foreground/10 bg-muted/20 px-2.5 py-2 text-xs text-muted-foreground">
					<div className="mb-1 text-[10px] text-muted-foreground/70">Plan</div>
					<ul className="flex flex-col gap-1">
						{event.entries.map((entry, i) => (
							<li key={`${i}-${entry.content.slice(0, 24)}`} className="flex items-start gap-2">
								<StatusDot
									color={
										entry.status === "completed"
											? "green"
											: entry.status === "in_progress"
												? "amber"
												: "muted"
									}
									className="mt-1"
								/>
								<span
									className={cn(
										"min-w-0 flex-1",
										entry.status === "completed" && "text-muted-foreground/60 line-through",
									)}
								>
									{entry.content}
								</span>
							</li>
						))}
					</ul>
				</div>
			);
		case "notice":
			return (
				<div className="text-center text-[11px] text-muted-foreground/60">{event.text}</div>
			);
		case "permission":
			return (
				<div className="flex items-center gap-2 rounded-lg border border-amber-500/30 bg-amber-500/5 px-2.5 py-1.5 text-xs text-amber-500/90">
					<StatusDot color="amber" className="size-1.5" />
					{event.text}
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
						<ChevronRight className="size-3 shrink-0 transition-transform group-open:rotate-90" />
						<span className="font-mono">{event.label}</span>
					</summary>
					<pre className="mt-1.5 max-h-48 overflow-y-auto whitespace-pre-wrap break-words rounded bg-muted/50 p-2 font-mono text-[11px] leading-relaxed">
						{JSON.stringify(event.json, null, 2)}
					</pre>
				</details>
			);
	}
}

// Pins the transcript to the newest event — but only while the user is already
// at the bottom. Scrolling up to read history detaches the pin (tracked by an
// IntersectionObserver against the ScrollArea viewport); returning to the
// bottom re-attaches it.
function ScrollAnchor({ count }: { count: number }) {
	const ref = useRef<HTMLDivElement>(null);
	const stickRef = useRef(true);
	useEffect(() => {
		const el = ref.current;
		const viewport = el?.closest("[data-radix-scroll-area-viewport]");
		if (!el || !viewport) return;
		const observer = new IntersectionObserver(
			([entry]) => {
				stickRef.current = entry.isIntersecting;
			},
			{ root: viewport, rootMargin: "0px 0px 96px 0px" },
		);
		observer.observe(el);
		return () => observer.disconnect();
	}, []);
	useEffect(() => {
		if (stickRef.current) ref.current?.scrollIntoView({ block: "end" });
	}, [count]);
	return <div ref={ref} className="h-px" />;
}

// ── Render pipeline: raw event list → displayable rows ────────────────────
// The ACP stream is chunked (one event per message fragment) and tool calls
// arrive as a `tool_call` plus N `tool_call_update`s. Coalesce so one message
// renders as one bubble and one tool call as one card that updates in place.
type KeyedEvent = TranscriptEvent & { key: string };

function coalesceTranscript(events: KeyedEvent[]): KeyedEvent[] {
	const out: KeyedEvent[] = [];
	const toolIndex = new Map<string, number>();
	let planIndex: number | null = null;
	for (const e of events) {
		const last = out[out.length - 1];
		if (
			(e.kind === "user" || e.kind === "assistant" || e.kind === "thinking") &&
			last?.kind === e.kind
		) {
			out[out.length - 1] = { ...last, text: last.text + e.text };
			continue;
		}
		if (e.kind === "tool" && e.toolCallId) {
			const idx = toolIndex.get(e.toolCallId);
			if (idx !== undefined) {
				const prev = out[idx] as Extract<KeyedEvent, { kind: "tool" }>;
				out[idx] = {
					...prev,
					// Updates often omit the title (the mapper then falls back to the
					// id) — never overwrite a real title with the id.
					tool: e.tool !== e.toolCallId ? e.tool : prev.tool,
					status: e.status ?? prev.status,
					input: e.input ?? prev.input,
					output: e.output ?? prev.output,
					locations: e.locations ?? prev.locations,
				};
				continue;
			}
			toolIndex.set(e.toolCallId, out.length);
		}
		if (e.kind === "plan") {
			// Plan updates are full snapshots: replace the existing card in place.
			if (planIndex !== null) {
				out[planIndex] = { ...out[planIndex], entries: e.entries } as KeyedEvent;
				continue;
			}
			planIndex = out.length;
		}
		out.push(e);
	}
	return out;
}

// Subtle three-dot pulse shown as the last row while a turn is in flight.
function TurnInFlight() {
	return (
		<div className="flex items-center gap-1 px-1 py-0.5" aria-label="Turn in progress">
			{[0, 1, 2].map((i) => (
				<span
					key={i}
					className="size-1.5 animate-pulse rounded-full bg-muted-foreground/50"
					style={{ animationDelay: `${i * 200}ms` }}
				/>
			))}
		</div>
	);
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
	actorId,
	sessionId,
	sessionStatus,
	onSessionCreated,
	onErrorEvent,
	onBusyChange,
}: {
	actorId: string;
	sessionId: string | null;
	sessionStatus?: string;
	onSessionCreated: (sessionId: string) => void;
	onErrorEvent: (text: string) => void;
	onBusyChange?: (busy: boolean) => void;
}) {
	const [draft, setDraft] = useState("");
	const [busy, setBusyState] = useState(false);
	const setBusy = (b: boolean) => {
		setBusyState(b);
		onBusyChange?.(b);
	};
	const [stopUnsupported, setStopUnsupported] = useState(false);
	const [optionsOpen, setOptionsOpen] = useState(false);
	const [agentType, setAgentType] = useState(
		() => localStorage.getItem(LS_AGENT_TYPE) ?? "opencode",
	);
	const [envJson, setEnvJson] = useState(() => localStorage.getItem(LS_ENV_JSON) ?? DEFAULT_ENV_JSON);
	// The in-flight session survives re-renders; also used by Stop.
	const activeSessionRef = useRef<string | null>(null);
	// Agent-type suggestions from the installed software (fetched only while the
	// options panel is open). Free text stays authoritative: the datalist is a
	// hint, and agent types are derived from package names heuristically.
	const software = useQuery({
		...agentOsSource.softwareQueryOptions(actorId),
		enabled: optionsOpen,
	});
	const agentTypeSuggestions = (software.data ?? [])
		.filter((bundle) => bundle.name.endsWith("· agent"))
		.map((bundle) => bundle.name.split(" · ")[0]?.split("/").pop() ?? "")
		.filter(Boolean);

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
				{optionsOpen && !sessionId ? (
					<div className="mb-2 flex flex-col gap-2 rounded-xl border bg-muted/30 p-3 text-xs">
						<label className="flex items-center gap-2">
							<span className="w-24 shrink-0 text-muted-foreground">Agent type</span>
							<input
								value={agentType}
								onChange={(e) => {
									setAgentType(e.target.value);
									localStorage.setItem(LS_AGENT_TYPE, e.target.value);
								}}
								list="agentos-composer-agent-types"
								className="flex-1 rounded border bg-background px-2 py-1 font-mono"
							/>
							<datalist id="agentos-composer-agent-types">
								{agentTypeSuggestions.map((type) => (
									<option key={type} value={type} />
								))}
							</datalist>
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
						{/* Agent choice and session env only matter when the next send
						    CREATES a session — an existing session already carries both,
						    so the control disappears once one is selected. */}
						{!sessionId ? (
							<button
								type="button"
								onClick={() => setOptionsOpen((v) => !v)}
								title="New-session options (agent, env)"
								className="inline-flex items-center gap-1 rounded px-2.5 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
							>
								<span aria-hidden="true" className="text-sm leading-none">
									+
								</span>
								Options
							</button>
						) : null}
						<span className="ml-auto" />
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
	// undefined = no explicit choice (default to the newest session); null =
	// explicitly composing a new session. Without the distinction, once any
	// session exists the UI can never start another one.
	const [selected, setSelected] = useState<string | null | undefined>(undefined);
	const sessionId = selected === undefined ? (sessions[0]?.sessionId ?? null) : selected;
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
		handler: (payload: unknown) => void,
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
	useAgentEvent("sessionEvent", (raw) => {
		const payload = raw as SessionEventPayload | undefined;
		const cur = sessionIdRef.current;
		if (!cur) return;
		// Broadcast may omit sessionId; when present, keep only this session's.
		if (payload?.sessionId && payload.sessionId !== cur) return;
		const notification: JsonRpcNotification | undefined =
			payload?.event ?? (payload as unknown as JsonRpcNotification);
		if (!notification) return;
		setLive((prev) => [...prev, mapNotification(notification, seqRef.current++)]);
	});
	// Permission requests render inline (non-interactive) so the history shows
	// "agent asked" in conversation position; the global banner above the tab is
	// the interactive surface.
	useAgentEvent("permissionRequest", (raw) => {
		const payload = raw as PermissionRequestPayload | undefined;
		const cur = sessionIdRef.current;
		if (!cur || payload?.sessionId !== cur) return;
		const request = payload?.request;
		setLive((prev) => [
			...prev,
			{
				kind: "permission",
				seq: seqRef.current++,
				text: `Permission requested: ${request?.description ?? request?.permissionId ?? "unknown"} (answer in the banner above)`,
			},
		]);
	});
	// Agent crashes surface in the thread; the runtime may auto-restart.
	useAgentEvent("agentCrashed", (raw) => {
		const payload = raw as AgentCrashedPayload | undefined;
		const cur = sessionIdRef.current;
		if (!cur || payload?.sessionId !== cur) return;
		const crash = payload?.event;
		setLive((prev) => [
			...prev,
			{
				kind: "error",
				seq: seqRef.current++,
				text: `Agent crashed (exit ${crash?.exitCode ?? "?"}) — restart: ${crash?.restart ?? "unknown"}${crash?.restartCount ? ` (attempt ${crash.restartCount})` : ""}`,
			},
		]);
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
	const [turnActive, setTurnActive] = useState(false);
	// Close ends the live agent process; the persisted transcript stays. Errors
	// (e.g. the session is idle, nothing to close) land in the thread.
	const closeSession = async (sid: string) => {
		try {
			await agentOsSource.closeSession(sid);
		} catch (error) {
			pushErrorEvent(error instanceof Error ? error.message : String(error));
		}
		setSelected(undefined);
		void queryClient.invalidateQueries({
			queryKey: agentOsSource.sessionsQueryOptions(actorId).queryKey,
		});
	};
	const events = useMemo(
		() =>
			coalesceTranscript([
				...backfill.map((e) => ({ ...e, key: `b-${e.seq}` })),
				...live.map((e) => ({ ...e, key: `l-${e.seq}` })),
			]),
		[backfill, live],
	);
	return (
		<div className="flex h-full min-h-0">
			<div className="flex h-full w-56 shrink-0 flex-col border-r">
				<div className="flex items-center px-3 pb-1 pt-2.5">
					{/* Live count lives here, next to the rows it describes — the
					    status strip stays hidden while the VM is simply healthy. */}
					<span className="text-[11px] font-medium text-muted-foreground">
						Sessions
						{(() => {
							const liveCount = liveIds
								? liveIds.size
								: sessions.filter((s) => s.status === "running").length;
							return liveCount > 0 ? ` · ${liveCount} live` : "";
						})()}
					</span>
					<VmStatusBadges actorId={actorId} align="left" />
					{sessionId !== null ? (
						<button
							type="button"
							onClick={() => setSelected(null)}
							title="Compose a prompt that starts a fresh session"
							className="ml-auto rounded border px-1.5 py-0.5 text-[10px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
						>
							New session
						</button>
					) : null}
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
						{(() => {
							const record = sessions.find((s) => s.sessionId === sessionId);
							const live = record ? isLive(record) : false;
							return (
								<div className="flex shrink-0 items-center gap-2 border-b px-4 py-2 text-xs">
									<StatusDot color={live ? "green" : "muted"} />
									<span className="font-mono">{record?.agentType ?? "session"}</span>
									<span className="text-muted-foreground">
										{record ? relativeTime(record.createdAt) : null}
									</span>
									<span className="font-mono text-[10px] text-muted-foreground/60">
										…{sessionId.slice(-10)}
									</span>
									<span className="text-muted-foreground/70">{live ? "live" : "idle"}</span>
									<span className="ml-auto" />
									{live ? (
										<button
											type="button"
											onClick={() => void closeSession(sessionId)}
											title="End the live agent process; the transcript stays"
											className="rounded border px-2 py-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
										>
											Close session
										</button>
									) : null}
								</div>
							);
						})()}
						<ScrollArea className="min-h-0 flex-1">
							{events.length === 0 && !turnActive ? (
								<AgentOsEmpty>No activity yet — send a prompt below.</AgentOsEmpty>
							) : (
								<div className="mx-auto flex w-full max-w-3xl flex-col gap-3 px-4 py-4">
									{events.map((e) => (
										<TranscriptEventView key={e.key} event={e} />
									))}
									{turnActive ? <TurnInFlight /> : null}
									<ScrollAnchor count={backfill.length + live.length} />
								</div>
							)}
						</ScrollArea>
					</>
				)}
				<Composer
					actorId={actorId}
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
					onBusyChange={setTurnActive}
				/>
			</div>
		</div>
	);
}
