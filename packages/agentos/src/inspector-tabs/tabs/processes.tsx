import { useQueryClient, useSuspenseQuery } from "@tanstack/react-query";
import { useRef, useState } from "react";
import { ActionErrorNote, AgentOsEmpty, relativeTime, StatusDot } from "../common";
import { useAgentOsActor } from "../lib/rivet";
import { agentOsSource, decodeActionBytes } from "../lib/source";
import type { KernelProcessInfo, ProcessExitPayload, ProcessOutputPayload, ProcessTreeNode } from "../lib/types";
import { ScrollArea } from "../ui/scroll-area";
import React from "react";

/** Format an epoch-ms spawn time for display; `—` when absent. */
function formatStartedAt(startedAt: number | undefined): string {
	if (!startedAt) return "—";
	return new Date(startedAt).toLocaleTimeString();
}

/** Tree → rows with depth for indentation. Running processes sort before
 * exited at every level; ties break newest-first so fresh activity is on top. */
interface ProcessRow extends KernelProcessInfo {
	depth: number;
}

function flattenTree(nodes: ProcessTreeNode[], depth = 0, out: ProcessRow[] = []): ProcessRow[] {
	const sorted = [...nodes].sort(
		(a, b) =>
			Number(b.status === "running") - Number(a.status === "running") ||
			b.startTime - a.startTime,
	);
	for (const node of sorted) {
		const { children, ...info } = node;
		out.push({ ...info, depth });
		flattenTree(children, depth + 1, out);
	}
	return out;
}

// ── Live output tail ───────────────────────────────────────────────────────
// Bounded per-pid buffers fed by the `processOutput` broadcast. Only pids
// spawned through the SDK have output pumps; everything else shows nothing.
const MAX_TRACKED_PIDS = 32;
const MAX_BUFFER_CHARS = 64_000;

class OutputBuffers {
	private buffers = new Map<number, string>();

	append(pid: number, text: string): void {
		const prev = this.buffers.get(pid);
		if (prev === undefined && this.buffers.size >= MAX_TRACKED_PIDS) {
			// Bounded: drop the oldest-tracked pid's buffer.
			const oldest = this.buffers.keys().next().value;
			if (oldest !== undefined) this.buffers.delete(oldest);
		}
		const next = (prev ?? "") + text;
		this.buffers.set(pid, next.length > MAX_BUFFER_CHARS ? next.slice(-MAX_BUFFER_CHARS) : next);
	}

	get(pid: number): string | undefined {
		return this.buffers.get(pid);
	}
}

function ProcessDetail({
	p,
	outputTail,
	onStop,
	onKill,
	actionError,
}: {
	p: ProcessRow;
	outputTail?: string;
	onStop: () => void;
	onKill: () => void;
	actionError: unknown;
}) {
	const [confirming, setConfirming] = useState<"stop" | "kill" | null>(null);
	const confirmTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
	const arm = (which: "stop" | "kill", run: () => void) => {
		if (confirming === which) {
			clearTimeout(confirmTimer.current);
			setConfirming(null);
			run();
			return;
		}
		setConfirming(which);
		clearTimeout(confirmTimer.current);
		confirmTimer.current = setTimeout(() => setConfirming(null), 3_000);
	};
	const rows: [string, string][] = [
		["pid", String(p.pid)],
		["ppid", p.ppid ? String(p.ppid) : "—"],
		["command", p.command],
		["args", p.args.join(" ") || "—"],
		["cwd", p.cwd || "—"],
		["driver", p.driver || "—"],
		["status", p.status],
		["exit code", p.exitCode == null ? "—" : String(p.exitCode)],
		["started", formatStartedAt(p.startTime)],
		["exited", p.exitTime == null ? "—" : relativeTime(p.exitTime)],
	];
	return (
		<div className="flex h-full flex-col">
			<div className="flex items-center gap-2 border-b px-4 py-3">
				<span className="font-mono text-sm">pid {p.pid}</span>
				<span className="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground">
					{p.command}
				</span>
				{p.status === "running" ? (
					<>
						<button
							type="button"
							onClick={() => arm("stop", onStop)}
							title="Graceful stop (SIGTERM-style)"
							className="shrink-0 rounded-md border px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
						>
							{confirming === "stop" ? "Confirm stop?" : "Stop"}
						</button>
						<button
							type="button"
							onClick={() => arm("kill", onKill)}
							title="Force kill"
							className="shrink-0 rounded-md border border-destructive/50 px-2 py-1 text-xs text-destructive transition-colors hover:bg-destructive/10"
						>
							{confirming === "kill" ? "Confirm kill?" : "Kill"}
						</button>
					</>
				) : null}
			</div>
			{actionError ? <ActionErrorNote error={actionError} className="border-b py-2" /> : null}
			<ScrollArea className="min-h-0 flex-1">
				<dl className="px-4 py-2 text-xs">
					{rows.map(([key, val]) => (
						<div key={key} className="flex gap-3 border-b border-foreground/[0.06] py-2 last:border-0">
							<dt className="w-24 shrink-0 text-muted-foreground">{key}</dt>
							<dd className="min-w-0 flex-1 break-words font-mono">{val}</dd>
						</div>
					))}
				</dl>
				{outputTail ? (
					<div className="px-4 pb-3">
						<div className="mb-1 text-[10px] text-muted-foreground/70">Output (live tail)</div>
						<pre className="max-h-64 overflow-y-auto whitespace-pre-wrap break-words rounded bg-muted/50 p-2 font-mono text-[11px] leading-relaxed">
							{outputTail}
						</pre>
					</div>
				) : (
					<div className="px-4 pb-3 text-[11px] text-muted-foreground/60">
						No live output. Only processes spawned through the SDK stream stdout/stderr here.
					</div>
				)}
			</ScrollArea>
		</div>
	);
}

export function ProcessesLoaded({ actorId }: { actorId: string }) {
	const { data: tree } = useSuspenseQuery(agentOsSource.processTreeQueryOptions(actorId));
	const queryClient = useQueryClient();
	const [selectedPid, setSelectedPid] = useState<number>();
	const [actionError, setActionError] = useState<unknown>(null);

	const rows = flattenTree(tree);
	const selected = rows.find((p) => p.pid === selectedPid) ?? rows[0];

	// Live output tail for SDK-spawned pids; exit refreshes the table.
	const buffersRef = useRef(new OutputBuffers());
	const [, setOutputVersion] = useState(0);
	const actor = useAgentOsActor();
	const useAgentEvent = actor.useEvent as (
		name: string,
		handler: (payload: unknown) => void,
	) => void;
	useAgentEvent("processOutput", (raw) => {
		const payload = raw as ProcessOutputPayload | undefined;
		if (!payload || typeof payload.pid !== "number") return;
		const text = new TextDecoder("utf-8", { fatal: false }).decode(
			decodeActionBytes(payload.data),
		);
		if (!text) return;
		buffersRef.current.append(payload.pid, text);
		setOutputVersion((v) => v + 1);
	});
	useAgentEvent("processExit", (raw) => {
		const payload = raw as ProcessExitPayload | undefined;
		if (!payload || typeof payload.pid !== "number") return;
		buffersRef.current.append(payload.pid, `\n[exited ${payload.exitCode}]`);
		setOutputVersion((v) => v + 1);
		void queryClient.invalidateQueries({
			queryKey: agentOsSource.processTreeQueryOptions(actorId).queryKey,
		});
	});

	const invalidate = () =>
		queryClient.invalidateQueries({
			queryKey: agentOsSource.processTreeQueryOptions(actorId).queryKey,
		});
	const runControl = async (action: () => Promise<unknown>) => {
		setActionError(null);
		try {
			await action();
			await invalidate();
		} catch (error) {
			setActionError(error);
		}
	};

	if (rows.length === 0) {
		return <AgentOsEmpty>No processes in the VM.</AgentOsEmpty>;
	}
	return (
		<div className="flex min-h-0 flex-1">
			<ScrollArea className="min-h-0 w-3/5 border-r">
				<table className="w-full text-sm">
					<thead className="text-[11px] text-muted-foreground">
						<tr className="border-b">
							<th className="px-3 py-2 text-left font-medium">PID</th>
							<th className="px-3 py-2 text-left font-medium">Command</th>
							<th className="px-3 py-2 text-left font-medium">Started</th>
							<th className="px-3 py-2 text-left font-medium">Status</th>
						</tr>
					</thead>
					<tbody>
						{rows.map((p) => (
							<tr
								key={p.pid}
								onClick={() => setSelectedPid(p.pid)}
								className={`cursor-pointer border-b border-foreground/[0.06] hover:bg-muted/50 ${
									selected?.pid === p.pid ? "bg-muted/50" : ""
								} ${p.status === "exited" ? "opacity-50" : ""}`}
							>
								<td className="px-3 py-2 font-mono">{p.pid}</td>
								<td className="px-3 py-2 font-mono text-xs">
									<span style={{ paddingLeft: `${p.depth * 14}px` }}>
										{p.depth > 0 ? <span className="text-muted-foreground/40">└ </span> : null}
										{p.command} {p.args.join(" ")}
									</span>
								</td>
								<td className="px-3 py-2 font-mono text-xs text-muted-foreground">
									{formatStartedAt(p.startTime)}
								</td>
								<td className="px-3 py-2">
									<span className="inline-flex items-center gap-1.5 text-xs">
										<StatusDot color={p.status === "running" ? "green" : "red"} />
										{p.status}
									</span>
								</td>
							</tr>
						))}
					</tbody>
				</table>
			</ScrollArea>
			<div className="min-h-0 w-2/5">
				{selected ? (
					<ProcessDetail
						p={selected}
						outputTail={buffersRef.current.get(selected.pid)}
						onStop={() => void runControl(() => agentOsSource.stopProcess(selected.pid))}
						onKill={() => void runControl(() => agentOsSource.killProcess(selected.pid))}
						actionError={actionError}
					/>
				) : (
					<AgentOsEmpty>Select a process.</AgentOsEmpty>
				)}
			</div>
		</div>
	);
}

