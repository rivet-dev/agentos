import { useSuspenseQuery } from "@tanstack/react-query";
import { AgentOsEmpty, CopyButton } from "../common";
import { agentOsSource } from "../lib/source";
import type { MountInfo } from "../lib/types";
import { Badge } from "../ui/badge";
import { ScrollArea } from "../ui/scroll-area";
import React from "react";

const isEmptyConfig = (config: undefined | null | any): config is null | undefined => {
	return typeof config === "undefined" || config === null || Object.keys(config).length === 0;
}

export function MountsTab({ mounts }: { mounts: MountInfo[] }) {
	return (
		<div className="flex h-full flex-col">
			{mounts.length === 0 ? (
				<AgentOsEmpty>No mounts configured on this actor.</AgentOsEmpty>
			) : (
				<ScrollArea className="min-h-0 flex-1">
					<table className="w-full text-sm">
						<thead className="text-[11px] uppercase tracking-wide text-muted-foreground">
							<tr className="border-b">
								<th className="px-3 py-2 text-left font-medium">Path</th>
								<th className="px-3 py-2 text-left font-medium">Kind</th>
								<th className="px-3 py-2 text-left font-medium">Access</th>
								<th className="px-3 py-2 text-left font-medium">Config</th>
							</tr>
						</thead>
						<tbody>
							{mounts.map((m) => (
								<tr
									key={m.path}
									className="border-b border-foreground/[0.06] align-top hover:bg-muted/50"
								>
									<td className="px-3 py-2 font-mono text-xs">{m.path}</td>
									<td className="px-3 py-2">
										<Badge variant="secondary" className="font-mono">
											{m.kind}
										</Badge>
									</td>
									<td className="px-3 py-2 text-xs text-muted-foreground">
										{m.readOnly ? "read-only" : "read-write"}
									</td>
									<td className="px-3 py-2">
										{isEmptyConfig(m.config) ? (
											<span className="text-muted-foreground/60">—</span>
										) : (
											<div className="group relative max-w-[20rem]">
												<code className="block overflow-x-auto whitespace-nowrap rounded bg-muted/30 py-1.5 pl-2 pr-7 font-mono text-xs text-muted-foreground">
													{JSON.stringify(m.config)}
												</code>
												<CopyButton
													value={JSON.stringify(m.config, null, 2)}
													className="absolute right-1 top-1 bg-background/70 backdrop-blur-sm"
												/>
											</div>
										)}
									</td>
								</tr>
							))}
						</tbody>
					</table>
				</ScrollArea>
			)}
		</div>
	);
}

export function MountsTabConnected({ actorId }: { actorId: string }) {
	const { data } = useSuspenseQuery(agentOsSource.mountsQueryOptions(actorId));
	return <MountsTab mounts={data} />;
}
