import { useSuspenseQuery } from "@tanstack/react-query";
import { useState } from "react";
import { AgentOsEmpty, StatusDot } from "../common";
import { agentOsSource } from "../lib/source";
import type { ProcessInfo } from "../lib/types";
import { ScrollArea } from "../ui/scroll-area";
import React from "react";

/** Format an epoch-ms spawn time for display; `—` when absent. */
function formatStartedAt(startedAt: number | undefined): string {
	if (!startedAt) return "—";
	return new Date(startedAt).toLocaleTimeString();
}

function ProcessDetail({ p }: { p: ProcessInfo }) {
	const rows: [string, string][] = [
		["pid", String(p.pid)],
		["command", p.command],
		["args", p.args.join(" ") || "—"],
		["status", p.running ? "running" : "exited"],
		["exit code", p.exitCode == null ? "—" : String(p.exitCode)],
		["started", formatStartedAt(p.startedAt)],
	];
	return (
		<div className="flex h-full flex-col">
			<div className="flex items-center gap-2 border-b px-4 py-3">
				<span className="font-mono text-sm">pid {p.pid}</span>
				<span className="truncate font-mono text-xs text-muted-foreground">{p.command}</span>
			</div>
			<ScrollArea className="min-h-0 flex-1">
				<dl className="px-4 py-2 text-xs">
					{rows.map(([key, val]) => (
						<div key={key} className="flex gap-3 border-b border-foreground/[0.06] py-2 last:border-0">
							<dt className="w-24 shrink-0 text-muted-foreground">{key}</dt>
							<dd className="min-w-0 flex-1 break-words font-mono">{val}</dd>
						</div>
					))}
				</dl>
			</ScrollArea>
		</div>
	);
}

export function ProcessesTabConnected({ actorId }: { actorId: string }) {
	const { data } = useSuspenseQuery(agentOsSource.processesQueryOptions(actorId));
	const [selectedPid, setSelectedPid] = useState<number>();
	const selected = data.find((p) => p.pid === selectedPid) ?? data[0];

	return (
		<div className="flex h-full flex-col">
			{data.length === 0 ? (
				<AgentOsEmpty>No processes running.</AgentOsEmpty>
			) : (
				<div className="flex min-h-0 flex-1">
					<ScrollArea className="min-h-0 w-3/5 border-r">
						<table className="w-full text-sm">
							<thead className="text-[11px] uppercase tracking-wide text-muted-foreground">
								<tr className="border-b">
									<th className="px-3 py-2 text-left font-medium">PID</th>
									<th className="px-3 py-2 text-left font-medium">Command</th>
									<th className="px-3 py-2 text-left font-medium">Started</th>
									{/* <th className="px-3 py-2 text-left font-medium">Mem</th> */}
									{/* <th className="px-3 py-2 text-left font-medium">CPU</th> */}
									<th className="px-3 py-2 text-left font-medium">Status</th>
								</tr>
							</thead>
							<tbody>
								{data.map((p) => (
									<tr
										key={p.pid}
										onClick={() => setSelectedPid(p.pid)}
										className={`cursor-pointer border-b border-foreground/[0.06] hover:bg-muted/50 ${
											selected?.pid === p.pid ? "bg-muted/50" : ""
										}`}
									>
										<td className="px-3 py-2 font-mono">{p.pid}</td>
										<td className="px-3 py-2 font-mono text-xs">{p.command} {p.args.join(" ")}</td>
										<td className="px-3 py-2 font-mono text-xs text-muted-foreground">{formatStartedAt(p.startedAt)}</td>
										{/* mem, cpu */}
										<td className="px-3 py-2">
											<span className="inline-flex items-center gap-1.5 text-xs">
												<StatusDot color={p.running ? "green" : "red"} />
												{p.running ? "running" : "exited"}
											</span>
										</td>
									</tr>
								))}
							</tbody>
						</table>
					</ScrollArea>
					<div className="min-h-0 w-2/5">
						{selected ? <ProcessDetail p={selected} /> : <AgentOsEmpty>Select a process.</AgentOsEmpty>}
					</div>
				</div>
			)}
		</div>
	);
}
