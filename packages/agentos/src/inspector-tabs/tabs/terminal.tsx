// Interactive PTY into the VM, on the contracted shell surface: `openShell` +
// `writeShell`/`resizeShell`/`closeShell` actions and the `shellData`/
// `shellStderr`/`shellExit` broadcasts. xterm.js stays confined to this lazy
// chunk — nothing outside this file may import it, or it lands in the shared
// main bundle.
//
// Starting is an explicit gate because `openShell` boots a sleeping VM; merely
// opening the tab must never wake anything.
//
// Shells survive tab switches: the dashboard swaps this iframe out when
// another tab is opened, so open shell ids plus locally captured scrollback
// persist in sessionStorage (saved on pagehide) and reattach on return. The
// runtime has no scrollback-replay action, so output produced while the tab
// was hidden is not captured — reattach says so. Shells die with the VM
// (sleep/shutdown) regardless.
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { useEffect, useRef, useState } from "react";
import {
	ActionErrorNote,
	AgentOsEmpty,
	AgentOsWordmark,
	IconButton,
	PlusIcon,
	relativeTime,
	StatusDot,
} from "../common";
import { cn } from "../lib/cn";
import { useAgentOsActor } from "../lib/rivet";
import { agentOsSource, decodeActionBytes } from "../lib/source";
import type { ShellDataPayload, ShellExitPayload } from "../lib/types";
import { ScrollArea } from "../ui/scroll-area";
import { VmStatusBadges } from "../vm-status-badges";
import "@xterm/xterm/css/xterm.css";
import React from "react";

// Batch keystrokes into one `writeShell` per frame: the actor worker executes
// actions serially, so per-keystroke calls would queue behind slow actions.
const WRITE_FLUSH_MS = 16;
const RESIZE_DEBOUNCE_MS = 150;
// Bounded: shells fan their output out to every connected dashboard client,
// and scrollback lives in sessionStorage (~5 MB budget shared per origin).
const MAX_SHELLS = 4;
const MAX_SCROLLBACK_CHARS = 128 * 1024;

const shellsKey = (actorId: string) => `agentos-inspector:shells:${actorId}`;

type ShellStatus = "live" | "exited" | "dead";

interface ShellEntry {
	shellId: string;
	/** Display name ("sh 1", "sh 2", …) — shell ids are opaque and unreadable. */
	name: string;
	openedAt: number;
	status: ShellStatus;
	exitCode?: number;
}

interface PersistedShells {
	shells: { shellId: string; name?: string; openedAt?: number; scrollback: string }[];
	active: string | null;
	counter?: number;
}

function loadPersisted(actorId: string): PersistedShells | null {
	try {
		const raw = sessionStorage.getItem(shellsKey(actorId));
		if (!raw) return null;
		const parsed = JSON.parse(raw) as PersistedShells;
		return Array.isArray(parsed?.shells) && parsed.shells.length > 0 ? parsed : null;
	} catch {
		return null;
	}
}

export function TerminalTabConnected({ actorId }: { actorId: string }) {
	const [shells, setShells] = useState<ShellEntry[]>([]);
	const [activeId, setActiveId] = useState<string | null>(null);
	const [opening, setOpening] = useState(false);
	const [startError, setStartError] = useState<unknown>(null);
	const containerRef = useRef<HTMLDivElement>(null);
	const termRef = useRef<Terminal | null>(null);
	const fitRef = useRef<FitAddon | null>(null);
	const writeBufRef = useRef("");
	const flushTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
	// Ref mirrors for the ref-stable broadcast handlers.
	const shellsRef = useRef(shells);
	shellsRef.current = shells;
	const activeIdRef = useRef(activeId);
	activeIdRef.current = activeId;
	// Per-shell scrollback + a streaming decoder (chunk boundaries can split
	// multibyte UTF-8; a fresh decode per chunk would corrupt them).
	const scrollbackRef = useRef(new Map<string, { text: string; decoder: TextDecoder }>());
	// Monotonic display-name counter ("sh 1", "sh 2", …), persisted so names
	// stay unique across tab switches.
	const nameCounterRef = useRef(1);

	// Close a shell without awaiting the caller's flow; the failure is logged,
	// not surfaced — the runtime reaps shells on VM sleep regardless.
	const closeQuietly = (shellId: string) => {
		void agentOsSource.closeShell(shellId).catch((error) => {
			console.warn(`agentos inspector: closeShell(${shellId}) failed`, error);
		});
	};

	const appendScrollback = (shellId: string, text: string) => {
		const map = scrollbackRef.current;
		const entry = map.get(shellId) ?? { text: "", decoder: new TextDecoder() };
		entry.text = (entry.text + text).slice(-MAX_SCROLLBACK_CHARS);
		map.set(shellId, entry);
	};

	const flushWrites = () => {
		flushTimerRef.current = undefined;
		const shellId = activeIdRef.current;
		const data = writeBufRef.current;
		if (!shellId || !data) return;
		writeBufRef.current = "";
		void agentOsSource.writeShell(shellId, data).catch((error) => {
			termRef.current?.write(
				`\r\n\x1b[31m[input failed: ${error instanceof Error ? error.message : String(error)}]\x1b[0m\r\n`,
			);
		});
	};

	const ensureTerm = (): Terminal | null => {
		if (termRef.current) return termRef.current;
		const container = containerRef.current;
		if (!container) return null;
		const term = new Terminal({
			cursorBlink: true,
			fontSize: 12,
			fontFamily: '"IBM Plex Mono", ui-monospace, monospace',
			// Match the dashboard theme tokens (--background: 240 6% 4%).
			theme: { background: "#0a0a0b" },
		});
		const fit = new FitAddon();
		term.loadAddon(fit);
		term.open(container);
		fit.fit();
		term.onData((data) => {
			writeBufRef.current += data;
			if (flushTimerRef.current === undefined) {
				flushTimerRef.current = setTimeout(flushWrites, WRITE_FLUSH_MS);
			}
		});
		termRef.current = term;
		fitRef.current = fit;
		return term;
	};

	// Show `shellId`'s scrollback in the (single) xterm instance.
	const renderShell = (shellId: string) => {
		const term = ensureTerm();
		if (!term) return;
		term.reset();
		const text = scrollbackRef.current.get(shellId)?.text;
		if (text) term.write(text);
		term.focus();
	};

	const switchTo = (shellId: string) => {
		if (shellId === activeIdRef.current) return;
		// Drop keystrokes buffered for the previous shell.
		writeBufRef.current = "";
		setActiveId(shellId);
		renderShell(shellId);
	};

	const start = async () => {
		if (shellsRef.current.filter((s) => s.status === "live").length >= MAX_SHELLS) {
			setStartError(new Error(`Shell limit reached (${MAX_SHELLS}) — close one first.`));
			return;
		}
		setStartError(null);
		setOpening(true);
		try {
			const term = ensureTerm();
			const { shellId } = await agentOsSource.openShell({
				cols: term?.cols,
				rows: term?.rows,
			});
			const name = `sh ${nameCounterRef.current++}`;
			setShells((prev) => [...prev, { shellId, name, openedAt: Date.now(), status: "live" }]);
			setActiveId(shellId);
			term?.reset();
			term?.focus();
		} catch (error) {
			setStartError(error);
		} finally {
			setOpening(false);
		}
	};

	const closeOne = (shellId: string) => {
		const entry = shellsRef.current.find((s) => s.shellId === shellId);
		if (entry?.status === "live") closeQuietly(shellId);
		scrollbackRef.current.delete(shellId);
		setShells((prev) => {
			const next = prev.filter((s) => s.shellId !== shellId);
			if (activeIdRef.current === shellId) {
				const fallback = next[next.length - 1]?.shellId ?? null;
				setActiveId(fallback);
				if (fallback) renderShell(fallback);
				else termRef.current?.reset();
			}
			return next;
		});
	};

	// Persist open shells + scrollback so a tab switch (iframe swap) can
	// reattach. React unmount cleanup does not run when the dashboard swaps the
	// iframe's document away, so save on pagehide.
	useEffect(() => {
		const persist = () => {
			const live = shellsRef.current.filter((s) => s.status === "live");
			if (live.length === 0) {
				sessionStorage.removeItem(shellsKey(actorId));
				return;
			}
			const payload: PersistedShells = {
				shells: live.map((s) => ({
					shellId: s.shellId,
					name: s.name,
					openedAt: s.openedAt,
					scrollback: scrollbackRef.current.get(s.shellId)?.text ?? "",
				})),
				active: activeIdRef.current,
				counter: nameCounterRef.current,
			};
			try {
				sessionStorage.setItem(shellsKey(actorId), JSON.stringify(payload));
			} catch (error) {
				console.warn("agentos inspector: failed to persist shells", error);
			}
		};
		window.addEventListener("pagehide", persist);
		return () => {
			window.removeEventListener("pagehide", persist);
			persist();
		};
	}, [actorId]);

	// Reattach shells from a previous mount of this tab. Liveness is probed
	// with a resize (cheap, idempotent): a reaped shell answers with a runtime
	// error and gets marked dead instead of silently eating keystrokes.
	useEffect(() => {
		const persisted = loadPersisted(actorId);
		if (!persisted) return;
		sessionStorage.removeItem(shellsKey(actorId));
		for (const s of persisted.shells) {
			appendScrollback(s.shellId, s.scrollback);
			appendScrollback(
				s.shellId,
				"\r\n\x1b[2m[reattached — output while this tab was hidden was not captured]\x1b[0m\r\n",
			);
		}
		nameCounterRef.current = persisted.counter ?? persisted.shells.length + 1;
		setShells(
			persisted.shells.map((s, i) => ({
				shellId: s.shellId,
				name: s.name ?? `sh ${i + 1}`,
				openedAt: s.openedAt ?? Date.now(),
				status: "live" as const,
			})),
		);
		const active = persisted.active ?? persisted.shells[0]?.shellId ?? null;
		setActiveId(active);
		// Render after the container is visible (state update above unhides it).
		queueMicrotask(() => {
			if (active) renderShell(active);
			const term = termRef.current;
			for (const s of persisted.shells) {
				void agentOsSource
					.resizeShell(s.shellId, term?.cols ?? 80, term?.rows ?? 24)
					.catch(() => {
						appendScrollback(
							s.shellId,
							"\r\n\x1b[2m[shell no longer exists — the VM may have slept]\x1b[0m\r\n",
						);
						setShells((prev) =>
							prev.map((e) => (e.shellId === s.shellId ? { ...e, status: "dead" } : e)),
						);
						if (activeIdRef.current === s.shellId) renderShell(s.shellId);
					});
			}
		});
	}, [actorId]);

	// Fit-to-container, propagated to the active PTY (debounced).
	useEffect(() => {
		const container = containerRef.current;
		if (!container) return;
		let timer: ReturnType<typeof setTimeout> | undefined;
		const observer = new ResizeObserver(() => {
			clearTimeout(timer);
			timer = setTimeout(() => {
				const term = termRef.current;
				fitRef.current?.fit();
				const shellId = activeIdRef.current;
				if (term && shellId) {
					void agentOsSource.resizeShell(shellId, term.cols, term.rows).catch((error) => {
						console.warn("agentos inspector: resizeShell failed", error);
					});
				}
			}, RESIZE_DEBOUNCE_MS);
		});
		observer.observe(container);
		return () => {
			clearTimeout(timer);
			observer.disconnect();
		};
	}, []);

	// Dispose the xterm instance on unmount. Shells are deliberately NOT
	// closed: they persist across tab switches and are reaped on VM sleep.
	useEffect(() => {
		return () => {
			clearTimeout(flushTimerRef.current);
			termRef.current?.dispose();
			termRef.current = null;
		};
	}, []);

	// Broadcast streams. `shellData`/`shellStderr` fan out to every connected
	// client, so keep only output for shells this iframe owns; the active one
	// also renders live.
	const actor = useAgentOsActor();
	const useAgentEvent = actor.useEvent as (
		name: string,
		handler: (payload: unknown) => void,
	) => void;
	const onShellOutput = (raw: unknown) => {
		const payload = raw as ShellDataPayload | undefined;
		if (!payload?.shellId) return;
		const entry = shellsRef.current.find((s) => s.shellId === payload.shellId);
		if (!entry) return;
		const sb = scrollbackRef.current.get(payload.shellId) ?? {
			text: "",
			decoder: new TextDecoder(),
		};
		const text = sb.decoder.decode(decodeActionBytes(payload.data), { stream: true });
		sb.text = (sb.text + text).slice(-MAX_SCROLLBACK_CHARS);
		scrollbackRef.current.set(payload.shellId, sb);
		if (payload.shellId === activeIdRef.current) termRef.current?.write(text);
	};
	useAgentEvent("shellData", onShellOutput);
	useAgentEvent("shellStderr", onShellOutput);
	useAgentEvent("shellExit", (raw) => {
		const payload = raw as ShellExitPayload | undefined;
		if (!payload?.shellId) return;
		if (!shellsRef.current.some((s) => s.shellId === payload.shellId)) return;
		const note = `\r\n\x1b[2m[shell exited with code ${payload.exitCode}]\x1b[0m\r\n`;
		appendScrollback(payload.shellId, note);
		setShells((prev) =>
			prev.map((s) =>
				s.shellId === payload.shellId
					? { ...s, status: "exited", exitCode: payload.exitCode }
					: s,
			),
		);
		if (payload.shellId === activeIdRef.current) termRef.current?.write(note);
	});
	useAgentEvent("vmShutdown", (raw) => {
		if (!shellsRef.current.some((s) => s.status === "live")) return;
		const reason = (raw as { reason?: string } | undefined)?.reason;
		const note = `\r\n\x1b[2m[VM shut down${reason ? ` (${reason})` : ""} — shell terminated]\x1b[0m\r\n`;
		for (const s of shellsRef.current) {
			if (s.status === "live") appendScrollback(s.shellId, note);
		}
		setShells((prev) => prev.map((s) => (s.status === "live" ? { ...s, status: "dead" } : s)));
		if (activeIdRef.current) termRef.current?.write(note);
	});

	const hasShells = shells.length > 0;
	const liveCount = shells.filter((s) => s.status === "live").length;
	// Sidebar + content, matching the transcript and filesystem tabs. Exited
	// shells stay listed with their scrollback (readable history) until closed.
	return (
		<div className="flex h-full min-h-0">
			<div className="flex h-full w-64 shrink-0 flex-col border-r">
				<div className="flex items-center px-3 pb-1 pt-2.5">
					<span className="text-[11px] font-medium text-muted-foreground">
						Shells{liveCount > 0 ? ` · ${liveCount} live` : ""}
					</span>
					<span className="ml-auto" />
					{opening ? (
						<span className="text-[10px] text-muted-foreground">Starting…</span>
					) : hasShells ? (
						<IconButton title="New shell" onClick={() => void start()}>
							<PlusIcon className="size-3.5" />
						</IconButton>
					) : null}
				</div>
				{!hasShells ? (
					<AgentOsEmpty>No shells yet.</AgentOsEmpty>
				) : (
					<ScrollArea className="min-h-0 flex-1">
						<div className="p-1.5">
							{shells.map((s) => (
								<div
									key={s.shellId}
									role="button"
									tabIndex={0}
									onClick={() => switchTo(s.shellId)}
									onKeyDown={(e) => {
										if (e.key === "Enter" || e.key === " ") switchTo(s.shellId);
									}}
									className={cn(
										"group flex w-full cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-left",
										s.shellId === activeId ? "bg-muted" : "hover:bg-muted/50",
									)}
								>
									<StatusDot color={s.status === "live" ? "green" : "muted"} />
									<div
										className="min-w-0 flex-1"
										title={
											s.status === "live"
												? "Live"
												: s.status === "exited"
													? `Exited with code ${s.exitCode}`
													: "Shell no longer exists (VM slept or shut down)"
										}
									>
										<div className="truncate text-xs">
											{s.name} · {relativeTime(s.openedAt)}
											{s.status === "exited" ? (
												<span className="text-muted-foreground/70"> · exited</span>
											) : null}
											{s.status === "dead" ? (
												<span className="text-muted-foreground/70"> · gone</span>
											) : null}
										</div>
										<div className="truncate font-mono text-[10px] text-muted-foreground/60">
											…{s.shellId.slice(-10)}
										</div>
									</div>
									<button
										type="button"
										onClick={(e) => {
											e.stopPropagation();
											closeOne(s.shellId);
										}}
										title={s.status === "live" ? "Close shell" : "Remove from list"}
										aria-label="Close shell"
										className="text-muted-foreground/40 opacity-0 transition-opacity hover:text-foreground focus:opacity-100 group-hover:opacity-100"
									>
										<svg
											viewBox="0 0 12 12"
											className="size-2.5"
											fill="none"
											stroke="currentColor"
											strokeWidth="1.5"
											strokeLinecap="round"
											aria-hidden="true"
										>
											<path d="M3 3l6 6M9 3l-6 6" />
										</svg>
									</button>
								</div>
							))}
						</div>
					</ScrollArea>
				)}
			</div>
			<div className="relative flex min-h-0 flex-1 flex-col">
				{/* VM trouble chips float over the top-right corner; nothing renders
				    while the VM is healthy. */}
				<div className="absolute right-3 top-2 z-10">
					<VmStatusBadges actorId={actorId} />
				</div>
				{!hasShells && !opening ? (
					<AgentOsEmpty>
						<div className="flex max-w-sm flex-col items-center gap-2">
							<AgentOsWordmark className="mb-3 w-44" />
							<span>Interactive shell into the VM.</span>
							<button
								type="button"
								onClick={() => void start()}
								className="rounded-md bg-primary px-3 py-1.5 text-xs text-primary-foreground transition-opacity hover:opacity-90"
							>
								Start shell
							</button>
							<span className="text-xs text-muted-foreground/70">
								Runs sh in the VM, booting it first if it is asleep. Shells stay open across
								inspector tabs, but die when the VM sleeps. Files under mounts persist across
								restarts; the rest of the filesystem resets.
							</span>
							{startError ? (
								<ActionErrorNote error={startError} className="p-0 text-left" />
							) : null}
						</div>
					</AgentOsEmpty>
				) : startError ? (
					<div className="shrink-0 border-b px-3 py-1.5 text-xs text-destructive">
						{startError instanceof Error ? startError.message : String(startError)}
					</div>
				) : null}
				<div
					ref={containerRef}
					className={!hasShells && !opening ? "hidden" : "min-h-0 flex-1 bg-[#0a0a0b] p-2"}
				/>
			</div>
		</div>
	);
}
