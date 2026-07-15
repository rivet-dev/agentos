// Interactive PTY into the VM, on the contracted shell surface: `openShell` +
// `writeShell`/`resizeShell`/`closeShell` actions and the `shellData`/
// `shellStderr`/`shellExit` broadcasts. xterm.js stays confined to this lazy
// chunk — nothing outside this file may import it, or it lands in the shared
// main bundle.
//
// Starting is an explicit gate because `openShell` boots a sleeping VM; merely
// opening the tab must never wake anything. There is no scrollback-replay
// action, so an iframe remount cannot reattach — a stale shell id (kept in
// sessionStorage) is closed best-effort and a fresh shell started instead.
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { useEffect, useRef, useState } from "react";
import { ActionErrorNote, AgentOsEmpty, StatusDot } from "../common";
import { useAgentOsActor } from "../lib/rivet";
import { agentOsSource, decodeActionBytes } from "../lib/source";
import type { ShellDataPayload, ShellExitPayload } from "../lib/types";
import "@xterm/xterm/css/xterm.css";
import React from "react";

// Batch keystrokes into one `writeShell` per frame: the actor worker executes
// actions serially, so per-keystroke calls would queue behind slow actions.
const WRITE_FLUSH_MS = 16;
const RESIZE_DEBOUNCE_MS = 150;

const staleShellKey = (actorId: string) => `agentos-inspector:shell:${actorId}`;

type ShellState =
	| { kind: "gate" }
	| { kind: "opening" }
	| { kind: "live"; shellId: string }
	| { kind: "exited"; code: number }
	| { kind: "vm-down"; reason?: string };

export function TerminalTabConnected({ actorId }: { actorId: string }) {
	const [state, setState] = useState<ShellState>({ kind: "gate" });
	const [startError, setStartError] = useState<unknown>(null);
	const containerRef = useRef<HTMLDivElement>(null);
	const termRef = useRef<Terminal | null>(null);
	const fitRef = useRef<FitAddon | null>(null);
	const shellIdRef = useRef<string | null>(null);
	const writeBufRef = useRef("");
	const flushTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

	// Close a shell without awaiting the caller's flow; the failure is logged,
	// not surfaced — the runtime reaps shells on VM sleep regardless.
	const closeQuietly = (shellId: string) => {
		void agentOsSource.closeShell(shellId).catch((error) => {
			console.warn(`agentos inspector: closeShell(${shellId}) failed`, error);
		});
	};

	const flushWrites = () => {
		flushTimerRef.current = undefined;
		const shellId = shellIdRef.current;
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

	const start = async () => {
		setStartError(null);
		setState({ kind: "opening" });
		try {
			const term = ensureTerm();
			const { shellId } = await agentOsSource.openShell({
				cols: term?.cols,
				rows: term?.rows,
			});
			shellIdRef.current = shellId;
			sessionStorage.setItem(staleShellKey(actorId), shellId);
			setState({ kind: "live", shellId });
			term?.reset();
			term?.focus();
		} catch (error) {
			setStartError(error);
			setState({ kind: "gate" });
		}
	};

	// A shell id left over from a previous iframe mount can't be reattached
	// (no scrollback replay) — close it so it doesn't linger in the VM.
	useEffect(() => {
		const stale = sessionStorage.getItem(staleShellKey(actorId));
		if (stale) {
			sessionStorage.removeItem(staleShellKey(actorId));
			closeQuietly(stale);
		}
	}, [actorId]);

	// Fit-to-container, propagated to the PTY (debounced).
	useEffect(() => {
		const container = containerRef.current;
		if (!container) return;
		let timer: ReturnType<typeof setTimeout> | undefined;
		const observer = new ResizeObserver(() => {
			clearTimeout(timer);
			timer = setTimeout(() => {
				const term = termRef.current;
				fitRef.current?.fit();
				const shellId = shellIdRef.current;
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

	// Teardown: close the live shell and dispose the terminal.
	useEffect(() => {
		return () => {
			clearTimeout(flushTimerRef.current);
			const shellId = shellIdRef.current;
			shellIdRef.current = null;
			if (shellId) {
				sessionStorage.removeItem(staleShellKey(actorId));
				closeQuietly(shellId);
			}
			termRef.current?.dispose();
			termRef.current = null;
		};
	}, [actorId]);

	// Broadcast streams. `shellData`/`shellStderr` fan out to every connected
	// client, so filter to this iframe's shell id (kept in a ref — the handlers
	// are ref-stable).
	const actor = useAgentOsActor();
	const useAgentEvent = actor.useEvent as (
		name: string,
		handler: (payload: unknown) => void,
	) => void;
	const writeShellOutput = (raw: unknown) => {
		const payload = raw as ShellDataPayload | undefined;
		if (!payload?.shellId || payload.shellId !== shellIdRef.current) return;
		termRef.current?.write(decodeActionBytes(payload.data));
	};
	useAgentEvent("shellData", writeShellOutput);
	useAgentEvent("shellStderr", writeShellOutput);
	useAgentEvent("shellExit", (raw) => {
		const payload = raw as ShellExitPayload | undefined;
		if (!payload?.shellId || payload.shellId !== shellIdRef.current) return;
		shellIdRef.current = null;
		sessionStorage.removeItem(staleShellKey(actorId));
		termRef.current?.write(`\r\n\x1b[2m[shell exited with code ${payload.exitCode}]\x1b[0m\r\n`);
		setState({ kind: "exited", code: payload.exitCode });
	});
	useAgentEvent("vmShutdown", (raw) => {
		if (!shellIdRef.current) return;
		shellIdRef.current = null;
		sessionStorage.removeItem(staleShellKey(actorId));
		const reason = (raw as { reason?: string } | undefined)?.reason;
		termRef.current?.write(`\r\n\x1b[2m[VM shut down${reason ? ` (${reason})` : ""} — shell terminated]\x1b[0m\r\n`);
		setState({ kind: "vm-down", reason });
	});

	const closeShell = () => {
		const shellId = shellIdRef.current;
		if (!shellId) return;
		shellIdRef.current = null;
		sessionStorage.removeItem(staleShellKey(actorId));
		closeQuietly(shellId);
		termRef.current?.write("\r\n\x1b[2m[shell closed]\x1b[0m\r\n");
		setState({ kind: "gate" });
	};

	return (
		<div className="flex h-full min-h-0 flex-col">
			{state.kind === "gate" && !termRef.current ? (
				<AgentOsEmpty>
					<div className="flex max-w-sm flex-col items-center gap-2">
						<span>Interactive shell into the VM.</span>
						<button
							type="button"
							onClick={() => void start()}
							className="rounded-md bg-primary px-3 py-1.5 text-xs text-primary-foreground transition-opacity hover:opacity-90"
						>
							Start shell
						</button>
						<span className="text-xs text-muted-foreground/70">
							Runs sh in the VM, booting it first if it is asleep. The root filesystem is
							in-memory and does not survive VM restarts.
						</span>
						{startError ? <ActionErrorNote error={startError} className="p-0 text-left" /> : null}
					</div>
				</AgentOsEmpty>
			) : (
				<div className="flex items-center gap-2 border-b px-3 py-1.5 text-xs">
					<StatusDot
						color={
							state.kind === "live" ? "green" : state.kind === "opening" ? "amber" : "muted"
						}
					/>
					<span className="text-muted-foreground">
						{state.kind === "live"
							? "Shell running"
							: state.kind === "opening"
								? "Starting shell…"
								: state.kind === "exited"
									? `Shell exited (${state.code})`
									: state.kind === "vm-down"
										? "VM shut down"
										: "Shell closed"}
					</span>
					{state.kind === "live" ? (
						<span className="font-mono text-[10px] text-muted-foreground/60">
							…{state.shellId.slice(-10)}
						</span>
					) : null}
					<span className="ml-auto" />
					{state.kind === "live" ? (
						<button
							type="button"
							onClick={closeShell}
							className="rounded border px-2 py-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
						>
							Close shell
						</button>
					) : state.kind !== "opening" ? (
						<button
							type="button"
							onClick={() => void start()}
							className="rounded-md bg-primary px-2.5 py-0.5 text-primary-foreground transition-opacity hover:opacity-90"
						>
							New shell
						</button>
					) : null}
				</div>
			)}
			{startError && termRef.current ? <ActionErrorNote error={startError} /> : null}
			<div
				ref={containerRef}
				className={state.kind === "gate" && !termRef.current ? "hidden" : "min-h-0 flex-1 bg-[#0a0a0b] p-2"}
			/>
		</div>
	);
}
