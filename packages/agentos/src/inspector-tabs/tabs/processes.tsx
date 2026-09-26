// Kernel process table with bounded output replay and control actions.
import { useQueryClient, useSuspenseQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { ActionErrorNote, ChevronRight, relativeTime, StatusDot } from "../common";
import { cn } from "../lib/cn";
import { processRowIdentity } from "../lib/process-identity";
import { drainProcessReplay, ProcessOutputDecoder } from "../lib/process-output";
import { useArmedConfirm } from "../lib/hooks";
import { useAgentOsActor } from "../lib/rivet";
import { agentOsSource } from "../lib/source";
import type { KernelProcessInfo, ProcessExitPayload, ProcessTreeNode } from "../lib/types";
import { VmStatusBadges } from "../vm-status-badges";
import React from "react";
import type { Output } from "../../generated/contract";

/** Format an epoch-ms first-observed time for display; `—` when absent. */
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
		// Kernel rows omit `args` for processes with no argv (and older runtimes
		// omit it entirely); normalize once here so every consumer can index it.
		out.push({ ...info, args: info.args ?? [], depth });
		flattenTree(children ?? [], depth + 1, out);
	}
	return out;
}

function requireProcessHandle(row: ProcessRow): Output.ActorProcessId {
	if (!row.process) throw new Error("This guest process has no actor process handle");
	return row.process;
}

// Replay is authoritative after a disconnected tab or VM restart. Events only
// prompt an earlier refresh of the process table; the output cursor is pulled.
const MAX_BUFFER_CHARS = 64_000;

function useProcessReplay(actorId: string, process: Output.ActorProcessId | null) {
	const generation = process?.generation;
	const pid = process?.pid;
	const [output, setOutput] = useState("");
	const [error, setError] = useState<unknown>(null);
	const [historyTruncated, setHistoryTruncated] = useState(false);
	useEffect(() => {
		// ExpandedDetail is keyed by actor, generation, PID, and handle identity,
		// so a new target starts with fresh state before it is rendered.
		if (generation === undefined || pid === undefined) return;
		let disposed = false;
		let pending = false;
		let cursor: number | bigint | undefined;
		let endReported = false;
		const controller = new AbortController();
		const decoder = new ProcessOutputDecoder();
		const pull = async () => {
			if (pending || disposed) return;
			pending = true;
			try {
				const replay = await drainProcessReplay(
					agentOsSource.processOutputReader(generation, pid),
					generation,
					cursor,
					controller.signal,
				);
				if (disposed) return;
				if (replay.truncated) setHistoryTruncated(true);
				cursor = replay.nextCursor ?? cursor;
				let text = replay.events.map((event) => decoder.decode(event)).join("");
				const gap = replay.truncated ? "\n[earlier output was truncated]\n" : "";
				const end = !replay.hasMore && replay.end && !endReported
					? `\n[exited ${replay.end.exitCode}]`
					: "";
				if (end) text += decoder.finish();
				if (end) endReported = true;
				if (text || gap || end) {
					setOutput((previous) => (previous + gap + text + end).slice(-MAX_BUFFER_CHARS));
				}
				setError(null);
			} catch (failure) {
				if (!disposed) setError(failure);
			} finally {
				pending = false;
			}
		};
		void pull();
		const timer = window.setInterval(() => void pull(), 2_000);
		return () => {
			disposed = true;
			controller.abort();
			window.clearInterval(timer);
		};
	}, [actorId, generation, pid]);
	return { output, error, historyTruncated };
}

/** Label-over-value cell for the expanded detail grid — compact, no divider
 * rows, the layout VM dashboards use for inspect summaries. */
function Field({ label, value }: { label: string; value: string }) {
	return (
		<div className="min-w-0">
			<div className="text-muted-foreground/70">{label}</div>
			<div className="truncate font-mono" title={value}>
				{value}
			</div>
		</div>
	);
}

function ExpandedDetail({
	actorId,
	p,
	onStop,
	onKill,
	actionError,
}: {
	actorId: string;
	p: ProcessRow;
	onStop: () => void;
	onKill: () => void;
	actionError: unknown;
}) {
	const replay = useProcessReplay(actorId, p.process);
	// Arming is per-process: expanding another row must not inherit it.
	const { armed, confirm } = useArmedConfirm<"stop" | "kill">({ resetKey: processRowIdentity(p) });
	return (
		<div className="flex flex-col gap-3 px-4 py-3 text-xs">
			<div className="grid grid-cols-2 gap-x-8 gap-y-2 sm:grid-cols-4">
				<Field label="ppid" value={p.ppid ? String(p.ppid) : "—"} />
				<Field label="cwd" value={p.cwd || "—"} />
				<Field label="driver" value={p.driver || "—"} />
				<Field label="exit code" value={p.exitCode == null ? "—" : String(p.exitCode)} />
				<Field label="args" value={p.args.join(" ") || "—"} />
				<Field label="first seen" value={formatStartedAt(p.startTime)} />
				<Field label="exit observed" value={p.exitTime == null ? "—" : relativeTime(p.exitTime)} />
				<Field label="group / session" value={`${p.pgid} / ${p.sid}`} />
			</div>
			{p.status === "running" && p.process ? (
				<div className="flex items-center gap-2">
					<button
						type="button"
						onClick={() => confirm("stop", onStop)}
						className="rounded border px-2 py-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
					>
						{armed === "stop" ? "Confirm stop?" : "Stop"}
					</button>
					<button
						type="button"
						onClick={() => confirm("kill", onKill)}
						className="rounded border border-destructive/40 px-2 py-0.5 text-destructive transition-colors hover:bg-destructive/10"
					>
						{armed === "kill" ? "Confirm kill?" : "Kill"}
					</button>
				</div>
			) : null}
			{!p.process ? (
				<div className="text-muted-foreground/60">
					Control and output replay are available only for processes started through this actor.
				</div>
			) : null}
			{actionError ? <ActionErrorNote error={actionError} className="p-0" /> : null}
			{replay.error ? <ActionErrorNote error={replay.error} className="p-0" /> : null}
			{replay.historyTruncated ? (
				<div className="text-muted-foreground">Some earlier output is no longer available.</div>
			) : null}
			{replay.output ? (
				<div>
					<div className="mb-1 text-muted-foreground/70">Output (replay, latest 64,000 characters)</div>
					<pre className="max-h-48 overflow-y-auto whitespace-pre-wrap break-words rounded bg-muted/50 p-2 font-mono text-[11px] leading-relaxed">
						{replay.output}
					</pre>
				</div>
			) : p.process ? (
				<div className="text-muted-foreground/60">
					No captured output for this process.
				</div>
			) : null}
		</div>
	);
}

/** Running/total counts for the System overview; same query key as the table
 * so React Query serves both from one fetch. */
export function useProcessCounts(actorId: string): { running: number; total: number } {
	const { data: tree } = useSuspenseQuery(agentOsSource.processTreeQueryOptions(actorId));
	const rows = flattenTree(tree);
	return { running: rows.filter((p) => p.status === "running").length, total: rows.length };
}

export function ProcessTable({ actorId }: { actorId: string }) {
	const { data: tree } = useSuspenseQuery(agentOsSource.processTreeQueryOptions(actorId));
	const queryClient = useQueryClient();
	const [expandedIdentity, setExpandedIdentity] = useState<string | null>(null);
	const [actionError, setActionError] = useState<unknown>(null);

	const rows = flattenTree(tree);

	const actor = useAgentOsActor();
	const useAgentEvent = actor.useEvent as (
		name: string,
		handler: (payload: unknown) => void,
	) => void;
	useAgentEvent("processExit", (raw) => {
		const payload = raw as ProcessExitPayload | undefined;
		if (!payload || typeof payload.pid !== "number") return;
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
		return <div className="px-4 py-3 text-sm text-muted-foreground">No processes in the VM.</div>;
	}
	return (
		<div className="max-h-96 overflow-y-auto">
			<table className="w-full text-sm">
				<thead className="sticky top-0 bg-secondary text-[11px] text-muted-foreground">
					<tr className="border-b">
						<th className="w-8 px-3 py-2" aria-label="Expand" />
						<th className="w-16 px-2 py-2 text-left font-medium">PID</th>
						<th className="px-2 py-2 text-left font-medium">Command</th>
						<th className="w-28 px-2 py-2 text-left font-medium">First seen</th>
						<th className="w-28 px-2 py-2 text-left font-medium">Status</th>
					</tr>
				</thead>
				<tbody>
					{rows.map((p) => (
						<React.Fragment key={processRowIdentity(p)}>
							<tr
								onClick={() => setExpandedIdentity((cur) => (cur === processRowIdentity(p) ? null : processRowIdentity(p)))}
								className={cn(
									"cursor-pointer border-b border-foreground/[0.06] hover:bg-muted/50",
									expandedIdentity === processRowIdentity(p) && "bg-muted/40",
									p.status === "exited" && "opacity-50",
								)}
							>
								<td className="px-3 py-1.5">
									<ChevronRight
										className={cn(
											"size-3 text-muted-foreground/60 transition-transform",
											expandedIdentity === processRowIdentity(p) && "rotate-90",
										)}
									/>
								</td>
								<td className="px-2 py-1.5 font-mono">{p.pid}</td>
								<td className="px-2 py-1.5 font-mono text-xs">
									<span style={{ paddingLeft: `${p.depth * 14}px` }}>
										{p.depth > 0 ? <span className="text-muted-foreground/40">└ </span> : null}
										{p.command}
										{p.args.length > 0 ? (
											<span className="text-muted-foreground/60"> {p.args.join(" ")}</span>
										) : null}
									</span>
								</td>
								<td className="px-2 py-1.5 font-mono text-xs text-muted-foreground">
									{formatStartedAt(p.startTime)}
								</td>
								<td className="px-2 py-1.5">
									<span className="inline-flex items-center gap-1.5 text-xs">
										<StatusDot color={p.status === "running" ? "green" : "muted"} />
										{p.status}
									</span>
								</td>
							</tr>
							{expandedIdentity === processRowIdentity(p) ? (
								<tr className="border-b border-foreground/[0.06] bg-muted/20">
									<td colSpan={5}>
										<ExpandedDetail
											key={`${actorId}:${processRowIdentity(p)}`}
											actorId={actorId}
											p={p}
											onStop={() => void runControl(() => {
											const handle = requireProcessHandle(p);
											return agentOsSource.stopProcess(handle.generation, handle.pid);
										})}
											onKill={() => void runControl(() => {
											const handle = requireProcessHandle(p);
											return agentOsSource.killProcess(handle.generation, handle.pid);
										})}
											actionError={actionError}
										/>
									</td>
								</tr>
							) : null}
						</React.Fragment>
					))}
				</tbody>
			</table>
		</div>
	);
}

export function ProcessesTabConnected({ actorId }: { actorId: string }) {
	return (
		<div className="relative h-full min-h-0 overflow-auto p-4">
			<div className="mb-3 flex items-center gap-2">
				<h1 className="text-sm font-semibold">Processes</h1>
				<span className="ml-auto" />
				<VmStatusBadges actorId={actorId} />
			</div>
			<div className="overflow-hidden rounded-lg border bg-secondary">
				<ProcessTable actorId={actorId} />
			</div>
		</div>
	);
}
