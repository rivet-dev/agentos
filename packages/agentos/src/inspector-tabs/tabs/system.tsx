// "What is this VM made of": installed software bundles and configured mounts
// in one scroll view. Both are low-frequency configuration views — merged so
// the tab bar stays flat as higher-traffic tabs (terminal, processes) land.
import { useSuspenseQueries } from "@tanstack/react-query";
import { useState } from "react";
import { ChevronRight, CopyButton } from "../common";
import { cn } from "../lib/cn";
import { agentOsSource } from "../lib/source";
import type { MountInfo, SoftwareBundle } from "../lib/types";
import { Badge } from "../ui/badge";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "../ui/collapsible";
import { ScrollArea } from "../ui/scroll-area";
import React from "react";

function SoftwareRow({ bundle }: { bundle: SoftwareBundle }) {
	const [open, setOpen] = useState(false);
	const hasBinaries = bundle.binaries.length > 0;

	return (
		<Collapsible open={open} onOpenChange={setOpen} className="border-none">
			<CollapsibleTrigger
				className="flex w-full items-center gap-2 px-4 py-3 text-left transition-colors hover:bg-muted/50"
				disabled={!hasBinaries}
			>
				<ChevronRight
					className={cn(
						"size-3 shrink-0 text-muted-foreground transition-transform",
						open && "rotate-90",
						!hasBinaries && "opacity-0",
					)}
				/>
				<span className="flex-1 truncate font-mono text-sm">{bundle.name}</span>
				{hasBinaries ? (
					<span className="text-[11px] tabular-nums text-muted-foreground/70">
						{bundle.binaries.length} cmd{bundle.binaries.length === 1 ? "" : "s"}
					</span>
				) : null}
				<span className="text-xs tabular-nums text-muted-foreground">{bundle.version}</span>
				<Badge variant={bundle.source === "user" ? "outline" : "secondary"}>
					{bundle.source}
				</Badge>
			</CollapsibleTrigger>
			{hasBinaries ? (
				<CollapsibleContent>
					<div className="flex flex-wrap gap-1.5 px-4 pb-3 pl-9">
						{bundle.binaries.map((bin) => (
							<span
								key={bin}
								className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs text-muted-foreground"
							>
								{bin}
							</span>
						))}
					</div>
				</CollapsibleContent>
			) : null}
		</Collapsible>
	);
}

const isEmptyConfig = (config: unknown): config is null | undefined =>
	typeof config === "undefined" ||
	config === null ||
	Object.keys(config as object).length === 0;

function MountsTable({ mounts }: { mounts: MountInfo[] }) {
	return (
		<table className="w-full text-sm">
			<thead className="text-[11px] text-muted-foreground">
				<tr className="border-b">
					<th className="px-4 py-2 text-left font-medium">Path</th>
					<th className="px-3 py-2 text-left font-medium">Kind</th>
					<th className="px-3 py-2 text-left font-medium">Access</th>
					<th className="px-3 py-2 text-left font-medium">Config</th>
				</tr>
			</thead>
			<tbody>
				{mounts.map((m) => (
					<tr
						key={m.path}
						className="border-b border-foreground/[0.06] align-top last:border-0 hover:bg-muted/50"
					>
						<td className="px-4 py-2 font-mono text-xs">{m.path}</td>
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
	);
}

function SectionHeader({ children }: { children: string }) {
	return (
		<div className="border-b bg-muted/30 px-4 py-1.5 text-[11px] font-medium text-muted-foreground">
			{children}
		</div>
	);
}

export function SystemTabConnected({ actorId }: { actorId: string }) {
	const [software, mounts] = useSuspenseQueries({
		queries: [
			agentOsSource.softwareQueryOptions(actorId),
			agentOsSource.mountsQueryOptions(actorId),
		],
	});
	return (
		<ScrollArea className="h-full min-h-0">
			<section>
				<SectionHeader>Software</SectionHeader>
				{software.data.length === 0 ? (
					<div className="px-4 py-3 text-sm text-muted-foreground">
						No software bundles installed.
					</div>
				) : (
					<div className="divide-y">
						{software.data.map((bundle) => (
							<SoftwareRow key={bundle.name} bundle={bundle} />
						))}
					</div>
				)}
			</section>
			<section>
				<SectionHeader>Mounts</SectionHeader>
				{mounts.data.length === 0 ? (
					<div className="px-4 py-3 text-sm text-muted-foreground">
						No mounts configured on this actor.
					</div>
				) : (
					<MountsTable mounts={mounts.data} />
				)}
			</section>
		</ScrollArea>
	);
}
