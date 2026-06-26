import { useSuspenseQuery } from "@tanstack/react-query";
import { useState } from "react";
import { AgentOsEmpty, ChevronRight } from "../common";
import { cn } from "../lib/cn";
import { agentOsSource } from "../lib/source";
import type { SoftwareBundle } from "../lib/types";
import { Badge } from "../ui/badge";
import {
	Collapsible,
	CollapsibleContent,
	CollapsibleTrigger,
} from "../ui/collapsible";
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
				<span className="text-xs tabular-nums text-muted-foreground">
					{bundle.version}
				</span>
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

export function SoftwareTab({ software }: { software: SoftwareBundle[] }) {
	return (
		<div className="flex h-full flex-col">
			{software.length === 0 ? (
				<AgentOsEmpty>No software bundles installed.</AgentOsEmpty>
			) : (
				<ScrollArea className="min-h-0 flex-1">
					<div className="divide-y">
						{software.map((bundle) => (
							<SoftwareRow key={bundle.name} bundle={bundle} />
						))}
					</div>
				</ScrollArea>
			)}
		</div>
	);
}

export function SoftwareTabConnected({ actorId }: { actorId: string }) {
	const { data } = useSuspenseQuery(agentOsSource.softwareQueryOptions(actorId));
	return <SoftwareTab software={data} />;
}
