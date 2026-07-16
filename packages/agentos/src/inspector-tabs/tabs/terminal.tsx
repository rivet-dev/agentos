// Interactive PTY into the VM, on the contracted shell surface: `openShell` +
// `writeShell`/`resizeShell`/`closeShell` actions and the `shellData`/
// `shellStderr`/`shellExit` broadcasts. xterm.js stays confined to this lazy
// chunk — nothing outside this file may import it, or it lands in the shared
// main bundle.
//
// Starting is an explicit gate because `openShell` boots a sleeping VM; merely
// opening the tab must never wake anything — including the reattach liveness
// probe, which consults the observe-only `getRuntimeHealth` before touching
// any shell action.
//
// Shells survive tab switches: the dashboard swaps this iframe out when
// another tab is opened, so open shell ids plus captured scrollback persist
// in sessionStorage (lib/shell-store) and reattach on return. While another
// inspector tab is on screen, ITS document keeps recording shell output into
// the same store (lib/shell-capture), so an ordinary tab switch loses
// nothing; only a window with no inspector document open leaves a gap, and
// reattach says so then. Shells die with the VM (sleep/shutdown) regardless.
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useReducer, useRef, useState } from "react";
import {
	ActionErrorNote,
	AgentOsEmpty,
	AgentOsWordmark,
	IconButton,
	PlusIcon,
	relativeTime,
	StatusDot,
} from "../common";
import { isInspectorActionError } from "../lib/actor-client";
import { cn } from "../lib/cn";
import { useAgentOsActor } from "../lib/rivet";
import { ShellMirror } from "../lib/shell-mirror";
import {
	clearPersistedShells,
	loadPersistedShells,
	type PersistedShells,
	savePersistedShells,
	trimScrollback,
} from "../lib/shell-store";
import { agentOsSource, decodeActionBytes, healthQueryOptions } from "../lib/source";
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
// The persisted payload is older than this ⇒ no inspector document was alive
// to capture output in between (an ordinary tab switch re-saves within
// milliseconds), so reattach flags the gap.
const CAPTURE_GAP_MS = 10_000;
const CAPTURE_GAP_NOTE =
	"\r\n\x1b[2m[reattached — output while no inspector tab was open was not captured]\x1b[0m\r\n";

type ShellStatus = "live" | "exited" | "dead";

interface ShellEntry {
	shellId: string;
	/** Display name ("sh 1", "sh 2", …) — shell ids are opaque and unreadable. */
	name: string;
	openedAt: number;
	status: ShellStatus;
	exitCode?: number;
}

export function TerminalTabConnected({ actorId }: { actorId: string }) {
	const [shells, setShellsState] = useState<ShellEntry[]>([]);
	const [activeId, setActiveIdState] = useState<string | null>(null);
	const [opening, setOpening] = useState(false);
	const [startError, setStartError] = useState<unknown>(null);
	// 30s ticker so the sidebar's relativeTime labels don't freeze.
	const [, bumpClock] = useReducer((n: number) => n + 1, 0);
	const queryClient = useQueryClient();
	const containerRef = useRef<HTMLDivElement>(null);
	const termRef = useRef<Terminal | null>(null);
	const fitRef = useRef<FitAddon | null>(null);
	const writeBufRef = useRef("");
	const flushTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
	// Flushes are separate HTTP requests; two in flight can arrive at the actor
	// out of order and reorder the typed bytes. Chain them so each flush is
	// sent only after the previous one is acked. The chain never rejects.
	const writeChainRef = useRef<Promise<void>>(Promise.resolve());
	// Imperative mirrors, updated synchronously WITH every state set (never
	// during render): the broadcast handlers and the write path must see a new
	// shell the instant `openShell` resolves — a render-lagged mirror drops
	// output that arrives before React commits (the shell's first prompt).
	const shellsRef = useRef<ShellEntry[]>([]);
	const activeIdRef = useRef<string | null>(null);
	// One headless mirror per LIVE shell (lib/shell-mirror): it answers the
	// shell's terminal queries whenever the visible xterm is not showing that
	// shell (background here, or every shell while another inspector tab has
	// the iframe), and its serialized buffer is the clean snapshot used for
	// rendering on switch and for persistence.
	const mirrorsRef = useRef(new Map<string, ShellMirror>());
	const setShells = (next: ShellEntry[]) => {
		shellsRef.current = next;
		setShellsState(next);
	};
	const setActiveId = (next: string | null) => {
		// The visible xterm answers queries for the shell it renders; the
		// mirror answers for everyone else. Exactly one answerer per shell.
		if (activeIdRef.current !== next) {
			const prev = activeIdRef.current;
			if (prev) mirrorsRef.current.get(prev)?.setAnswering(true);
			if (next) mirrorsRef.current.get(next)?.setAnswering(false);
		}
		activeIdRef.current = next;
		setActiveIdState(next);
	};
	// Per-shell scrollback with one streaming decoder PER stream: stdout and
	// stderr chunks interleave, and a shared decoder corrupts multibyte UTF-8
	// split across chunks of different streams.
	const scrollbackRef = useRef(
		new Map<string, { text: string; out: TextDecoder; err: TextDecoder }>(),
	);
	// True while renderShell() replays scrollback into xterm. Replayed bytes
	// include terminal queries the shell once sent (DSR/CPR "ESC[6n", …), and
	// xterm answers them by EMITTING input — forwarding those stale answers to
	// the live shell corrupts its line editor (reedline repaints the prompt
	// over command output). Drop everything onData produces during a replay.
	const replayingRef = useRef(false);
	// Monotonic display-name counter ("sh 1", "sh 2", …), persisted so names
	// stay unique across tab switches.
	const nameCounterRef = useRef(1);

	const scrollbackEntry = (shellId: string) => {
		const map = scrollbackRef.current;
		let entry = map.get(shellId);
		if (!entry) {
			entry = { text: "", out: new TextDecoder(), err: new TextDecoder() };
			map.set(shellId, entry);
		}
		return entry;
	};

	// Close a shell without awaiting the caller's flow; the failure is logged,
	// not surfaced — the runtime reaps shells on VM sleep regardless.
	const closeQuietly = (shellId: string) => {
		void agentOsSource.closeShell(shellId).catch((error) => {
			console.warn(`agentos inspector: closeShell(${shellId}) failed`, error);
		});
	};

	// Mirror query replies ride the same serialized write chain as keystrokes
	// (reply errors are log-only: the shell may just have exited).
	const mirrorReply = (shellId: string, data: string) => {
		writeChainRef.current = writeChainRef.current.then(() =>
			agentOsSource.writeShell(shellId, data).then(
				() => undefined,
				(error) => {
					console.warn(`agentos inspector: query reply to ${shellId} failed`, error);
				},
			),
		);
	};

	const ensureMirror = (shellId: string, seed: string, answering: boolean): ShellMirror => {
		let mirror = mirrorsRef.current.get(shellId);
		if (!mirror) {
			const term = termRef.current;
			mirror = new ShellMirror({
				cols: term?.cols ?? 80,
				rows: term?.rows ?? 24,
				onReply: (data) => mirrorReply(shellId, data),
			});
			mirror.seed(seed, answering);
			mirrorsRef.current.set(shellId, mirror);
		}
		return mirror;
	};

	// Freeze a mirror's final state into the raw scrollback (plus a closing
	// note) and drop it — used when a shell exits or dies.
	const freezeMirror = (shellId: string, note: string) => {
		const mirror = mirrorsRef.current.get(shellId);
		const entry = scrollbackEntry(shellId);
		entry.text = trimScrollback(
			(mirror?.isReady() ? mirror.serialize() : entry.text) + note,
		);
		if (mirror) {
			mirror.dispose();
			mirrorsRef.current.delete(shellId);
		}
	};

	const flushWrites = () => {
		flushTimerRef.current = undefined;
		const data = writeBufRef.current;
		// Always drain: a buffer left behind when no shell is active would leak
		// into whichever shell becomes active next.
		writeBufRef.current = "";
		const shellId = activeIdRef.current;
		if (!shellId || !data) return;
		writeChainRef.current = writeChainRef.current.then(() =>
			agentOsSource.writeShell(shellId, data).then(
				() => undefined,
				(error) => {
					termRef.current?.write(
						`\r\n\x1b[31m[input failed: ${error instanceof Error ? error.message : String(error)}]\x1b[0m\r\n`,
					);
				},
			),
		);
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
			// Answers xterm generates while replaying scrollback are stale
			// responses to long-answered queries — never forward them.
			if (replayingRef.current) return;
			// Keystrokes only go to a LIVE shell; an exited/dead one has no PTY,
			// and every write would just spam failing actions.
			const entry = shellsRef.current.find((s) => s.shellId === activeIdRef.current);
			if (entry?.status !== "live") return;
			writeBufRef.current += data;
			if (flushTimerRef.current === undefined) {
				flushTimerRef.current = setTimeout(flushWrites, WRITE_FLUSH_MS);
			}
		});
		termRef.current = term;
		fitRef.current = fit;
		return term;
	};

	// Show `shellId`'s scrollback in the (single) xterm instance. Live shells
	// render from their mirror's serialized snapshot (clean state, no queries
	// or destructive repaints); exited/dead ones from the frozen raw text.
	const renderShell = (shellId: string) => {
		const term = ensureTerm();
		if (!term) return;
		term.reset();
		const mirror = mirrorsRef.current.get(shellId);
		const text = mirror?.isReady()
			? mirror.serialize()
			: scrollbackRef.current.get(shellId)?.text;
		if (text) {
			replayingRef.current = true;
			// The write callback runs after xterm has processed the replayed
			// bytes (and synchronously emitted any query answers into onData).
			term.write(text, () => {
				replayingRef.current = false;
			});
		}
		term.focus();
	};

	// Mark shells dead (VM slept / shell reaped): note in scrollback, status in
	// the sidebar, and a repaint if one of them is on screen.
	const markDead = (shellIds: string[], note: string) => {
		if (shellIds.length === 0) return;
		for (const id of shellIds) freezeMirror(id, note);
		setShells(
			shellsRef.current.map((s) =>
				shellIds.includes(s.shellId) && s.status === "live" ? { ...s, status: "dead" } : s,
			),
		);
		const active = activeIdRef.current;
		if (active && shellIds.includes(active)) renderShell(active);
	};

	const switchTo = (shellId: string) => {
		if (shellId === activeIdRef.current) {
			// Already showing — just hand focus back to the terminal (the click
			// landed on the sidebar row, which otherwise keeps it).
			termRef.current?.focus();
			return;
		}
		// Drop keystrokes buffered for the previous shell.
		writeBufRef.current = "";
		setActiveId(shellId);
		renderShell(shellId);
		// The xterm viewport is shared across shells but the runtime resize only
		// ever targeted the previously-active PTY — sync this one to it.
		const term = termRef.current;
		const entry = shellsRef.current.find((s) => s.shellId === shellId);
		if (term && entry?.status === "live") {
			void agentOsSource.resizeShell(shellId, term.cols, term.rows).catch((error) => {
				console.warn("agentos inspector: resizeShell failed", error);
			});
		}
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
			// Answering starts off — this shell becomes active below, and the
			// visible xterm answers for the active shell.
			ensureMirror(shellId, "", false);
			setShells([
				...shellsRef.current,
				{ shellId, name, openedAt: Date.now(), status: "live" },
			]);
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
		mirrorsRef.current.get(shellId)?.dispose();
		mirrorsRef.current.delete(shellId);
		scrollbackRef.current.delete(shellId);
		const next = shellsRef.current.filter((s) => s.shellId !== shellId);
		if (activeIdRef.current === shellId) {
			// Drop keystrokes buffered for the closing shell before the fallback
			// becomes the flush target.
			writeBufRef.current = "";
			clearTimeout(flushTimerRef.current);
			flushTimerRef.current = undefined;
			const fallback = next[next.length - 1]?.shellId ?? null;
			setActiveId(fallback);
			if (fallback) renderShell(fallback);
			else termRef.current?.reset();
		}
		setShells(next);
	};

	// Persist shells + scrollback so a tab switch (iframe swap) can reattach.
	// React unmount cleanup does not run when the dashboard swaps the iframe's
	// document away, so save on pagehide. Live AND exited shells persist (the
	// sidebar keeps exited history until explicitly closed); dead ones are gone.
	useEffect(() => {
		const persist = () => {
			const keep = shellsRef.current.filter((s) => s.status !== "dead");
			if (keep.length === 0) {
				clearPersistedShells(actorId);
				return;
			}
			const active = activeIdRef.current;
			const payload: PersistedShells = {
				shells: keep.map((s) => {
					const mirror = mirrorsRef.current.get(s.shellId);
					return {
						shellId: s.shellId,
						name: s.name,
						openedAt: s.openedAt,
						scrollback: trimScrollback(
							mirror?.isReady()
								? mirror.serialize()
								: (scrollbackRef.current.get(s.shellId)?.text ?? ""),
						),
						status: s.status === "exited" ? "exited" : "live",
						exitCode: s.exitCode,
					};
				}),
				// Clamp: reattach must never select a shell that is not in the list.
				active: active && keep.some((s) => s.shellId === active) ? active : null,
				counter: nameCounterRef.current,
				cols: termRef.current?.cols,
				rows: termRef.current?.rows,
			};
			savePersistedShells(actorId, payload);
		};
		window.addEventListener("pagehide", persist);
		return () => {
			window.removeEventListener("pagehide", persist);
			persist();
		};
	}, [actorId]);

	// Reattach shells from a previous mount of this tab. The container div is
	// always laid out, so xterm can be created with real dimensions right here
	// (effects run after the DOM commit). Liveness of persisted-live shells is
	// probed without ever waking the VM: the observe-only `getRuntimeHealth`
	// answers "asleep" (shells died with the VM) before any shell action runs.
	useEffect(() => {
		const persisted = loadPersistedShells(actorId);
		if (!persisted) return;
		clearPersistedShells(actorId);
		const entries: ShellEntry[] = persisted.shells.map((s, i) => ({
			shellId: s.shellId,
			name: s.name ?? `sh ${i + 1}`,
			openedAt: s.openedAt ?? Date.now(),
			status: s.status ?? "live",
			exitCode: s.exitCode,
		}));
		// An ordinary tab switch re-saves the payload continuously (the other
		// tab's document records output too — lib/shell-capture), so a stale
		// payload means a real capture gap worth flagging.
		const captureGap = Date.now() - (persisted.savedAt ?? 0) > CAPTURE_GAP_MS;
		const activeId =
			persisted.active && entries.some((s) => s.shellId === persisted.active)
				? persisted.active
				: (entries[entries.length - 1]?.shellId ?? null);
		for (const s of persisted.shells) {
			// SET (not append) so a StrictMode double-mount cannot duplicate the
			// scrollback; the endsWith guard keeps the marker single for the same
			// reason.
			const entry = scrollbackEntry(s.shellId);
			const note =
				captureGap &&
				(s.status ?? "live") === "live" &&
				!s.scrollback.endsWith(CAPTURE_GAP_NOTE)
					? CAPTURE_GAP_NOTE
					: "";
			entry.text = trimScrollback(s.scrollback + note);
			// Live shells get a mirror seeded with the snapshot; it answers
			// queries for every shell except the one the xterm will render.
			if ((s.status ?? "live") === "live") {
				ensureMirror(s.shellId, entry.text, s.shellId !== activeId);
			}
		}
		nameCounterRef.current = persisted.counter ?? persisted.shells.length + 1;
		setShells(entries);
		setActiveId(activeId);
		if (activeId) renderShell(activeId);

		const liveIds = entries.filter((s) => s.status === "live").map((s) => s.shellId);
		let cancelled = false;
		if (liveIds.length > 0) {
			void (async () => {
				let booted: boolean;
				try {
					booted = (await queryClient.fetchQuery(healthQueryOptions(actorId))).booted;
				} catch (error) {
					// Older runtime without the action, or a transient failure: we
					// cannot tell without waking the VM — leave the shells live and
					// let the first interaction surface reality.
					console.warn("agentos inspector: reattach health check failed", error);
					return;
				}
				if (cancelled) return;
				if (!booted) {
					markDead(liveIds, "\r\n\x1b[2m[VM is asleep — shell terminated]\x1b[0m\r\n");
					return;
				}
				// VM already up: cheap idempotent per-shell probe, carrying the REAL
				// terminal size so reattached PTYs match the viewport. A reaped shell
				// answers with a runtime error and gets marked dead instead of
				// silently eating keystrokes.
				const term = termRef.current;
				for (const id of liveIds) {
					void agentOsSource
						.resizeShell(id, term?.cols ?? 80, term?.rows ?? 24)
						.catch((error) => {
							if (cancelled) return;
							// Only a runtime-layer rejection means the shell is gone; a
							// gateway/auth/timeout blip must not kill a live shell's row.
							if (isInspectorActionError(error) && error.layer === "runtime") {
								markDead(
									[id],
									"\r\n\x1b[2m[shell no longer exists — the VM may have slept]\x1b[0m\r\n",
								);
							} else {
								console.warn(
									`agentos inspector: reattach probe for ${id} failed (transient)`,
									error,
								);
							}
						});
				}
			})();
		}
		return () => {
			cancelled = true;
		};
	}, [actorId, queryClient]);

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
				if (term) {
					// Mirrors track the shared viewport size too, so their
					// snapshots reflow like the visible terminal.
					for (const mirror of mirrorsRef.current.values()) {
						mirror.resize(term.cols, term.rows);
					}
				}
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

	// Dispose the xterm instance and the mirrors on unmount. Shells are
	// deliberately NOT closed: they persist across tab switches (the persist
	// effect's cleanup snapshots the mirrors first) and are reaped on VM sleep.
	useEffect(() => {
		return () => {
			clearTimeout(flushTimerRef.current);
			termRef.current?.dispose();
			termRef.current = null;
			for (const mirror of mirrorsRef.current.values()) mirror.dispose();
			mirrorsRef.current.clear();
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
	const onShellOutput = (raw: unknown, stream: "out" | "err") => {
		const payload = raw as ShellDataPayload | undefined;
		if (!payload?.shellId) return;
		if (!shellsRef.current.some((s) => s.shellId === payload.shellId)) return;
		const sb = scrollbackEntry(payload.shellId);
		const text = sb[stream].decode(decodeActionBytes(payload.data), { stream: true });
		sb.text = trimScrollback(sb.text + text);
		// The mirror tracks every shell's true screen state (and answers
		// queries for non-active ones); the visible xterm renders the active.
		mirrorsRef.current.get(payload.shellId)?.write(text);
		if (payload.shellId === activeIdRef.current) termRef.current?.write(text);
	};
	useAgentEvent("shellData", (raw) => onShellOutput(raw, "out"));
	useAgentEvent("shellStderr", (raw) => onShellOutput(raw, "err"));
	useAgentEvent("shellExit", (raw) => {
		const payload = raw as ShellExitPayload | undefined;
		if (!payload?.shellId) return;
		if (!shellsRef.current.some((s) => s.shellId === payload.shellId)) return;
		const note = `\r\n\x1b[2m[shell exited with code ${payload.exitCode}]\x1b[0m\r\n`;
		freezeMirror(payload.shellId, note);
		setShells(
			shellsRef.current.map((s) =>
				s.shellId === payload.shellId
					? { ...s, status: "exited", exitCode: payload.exitCode }
					: s,
			),
		);
		if (payload.shellId === activeIdRef.current) termRef.current?.write(note);
	});
	useAgentEvent("vmShutdown", (raw) => {
		const live = shellsRef.current.filter((s) => s.status === "live").map((s) => s.shellId);
		if (live.length === 0) return;
		const reason = (raw as { reason?: string } | undefined)?.reason;
		markDead(
			live,
			`\r\n\x1b[2m[VM shut down${reason ? ` (${reason})` : ""} — shell terminated]\x1b[0m\r\n`,
		);
	});

	const hasShells = shells.length > 0;
	// Keep "Ns ago" labels moving while any rows are visible.
	useEffect(() => {
		if (!hasShells) return;
		const timer = setInterval(bumpClock, 30_000);
		return () => clearInterval(timer);
	}, [hasShells]);

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
					{/* Always visible, like the transcript's + — it starts a shell
					    directly, whether or not any exist yet. */}
					{opening ? (
						<span className="text-[10px] text-muted-foreground">Starting…</span>
					) : (
						<IconButton title="New shell" onClick={() => void start()}>
							<PlusIcon className="size-3.5" />
						</IconButton>
					)}
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
										if (e.key === "Enter" || e.key === " ") {
											e.preventDefault();
											switchTo(s.shellId);
										}
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
				    while the VM is healthy. Above the empty-state overlay. */}
				<div className="absolute right-3 top-2 z-20">
					<VmStatusBadges actorId={actorId} />
				</div>
				{hasShells && startError ? (
					<div className="shrink-0 border-b px-3 py-1.5 text-xs text-destructive">
						{startError instanceof Error ? startError.message : String(startError)}
					</div>
				) : null}
				{/* Always laid out: xterm must open and fit against a visible,
				    sized container (open/fit on display:none silently keeps the
				    80x24 defaults, which then get pushed to real PTYs). The empty
				    state overlays it instead of replacing it. */}
				<div ref={containerRef} className="min-h-0 flex-1 bg-[#0a0a0b] p-2" />
				{!hasShells && !opening ? (
					<div className="absolute inset-0 z-10 bg-background">
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
								{startError ? (
									<ActionErrorNote error={startError} className="p-0 text-left" />
								) : null}
							</div>
						</AgentOsEmpty>
					</div>
				) : null}
			</div>
		</div>
	);
}
