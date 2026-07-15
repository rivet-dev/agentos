// "What is this VM made of and what is it doing": the live process tree plus
// installed software, configured mounts, preview links, and the actor id in
// one scroll view, keeping the tab bar to the high-traffic surfaces
// (transcript, terminal, filesystem).
import { useQuery, useSuspenseQueries } from "@tanstack/react-query";
import { type ReactNode, useState } from "react";
import { ActionErrorNote, ChevronRight, CopyButton } from "../common";
import { cn } from "../lib/cn";
import { agentOsSource, healthQueryOptions } from "../lib/source";
import type { MountInfo, SignedPreviewUrl, SoftwareBundle } from "../lib/types";
import { Badge } from "../ui/badge";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "../ui/collapsible";
import { ScrollArea } from "../ui/scroll-area";
import { VmBootGate } from "../vm-boot-gate";
import { VmStatusBadges } from "../vm-status-badges";
import { ProcessTable, useProcessCounts } from "./processes";
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

// Signed preview links: proxy HTTP to a port inside the VM. Local state only —
// links created here are revocable here; links created by code are not listed
// (there is no enumeration action).
function PreviewLinks() {
	const [portDraft, setPortDraft] = useState("3000");
	const [links, setLinks] = useState<SignedPreviewUrl[]>([]);
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<unknown>(null);

	const create = async () => {
		const port = Number.parseInt(portDraft, 10);
		if (!Number.isInteger(port) || port < 1 || port > 65535) {
			setError(new Error(`Not a valid port: ${portDraft}`));
			return;
		}
		setBusy(true);
		setError(null);
		try {
			const link = await agentOsSource.createSignedPreviewUrl(port, 900);
			setLinks((prev) => [...prev.filter((l) => l.token !== link.token), link]);
		} catch (err) {
			setError(err);
		} finally {
			setBusy(false);
		}
	};
	const revoke = async (link: SignedPreviewUrl) => {
		setError(null);
		try {
			await agentOsSource.expireSignedPreviewUrl(link.token);
			setLinks((prev) => prev.filter((l) => l.token !== link.token));
		} catch (err) {
			setError(err);
		}
	};

	return (
		<div className="px-4 py-3 text-xs">
			<div className="flex items-center gap-2">
				<label className="text-muted-foreground" htmlFor="agentos-preview-port">
					Port
				</label>
				<input
					id="agentos-preview-port"
					value={portDraft}
					onChange={(e) => setPortDraft(e.target.value)}
					onKeyDown={(e) => e.key === "Enter" && void create()}
					inputMode="numeric"
					className="w-20 rounded border bg-background px-2 py-1 font-mono focus:outline-none"
				/>
				<button
					type="button"
					disabled={busy}
					onClick={() => void create()}
					className="rounded-md border px-2.5 py-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-40"
				>
					{busy ? "Creating…" : "Create preview link"}
				</button>
				<span className="text-muted-foreground/60">
					Signed URL to an HTTP server on that port, valid 15 minutes. Boots the VM if asleep.
				</span>
			</div>
			{error ? <ActionErrorNote error={error} className="py-2" /> : null}
			{links.length > 0 ? (
				<div className="mt-2 flex flex-col gap-1">
					{links.map((link) => (
						<div key={link.token} className="flex items-center gap-2">
							<a
								href={link.path}
								target="_blank"
								rel="noreferrer"
								className="truncate font-mono text-primary hover:underline"
							>
								port {link.port}: {link.path}
							</a>
							<span className="text-muted-foreground/60">
								expires {new Date(link.expiresAt).toLocaleTimeString()}
							</span>
							<button
								type="button"
								onClick={() => void revoke(link)}
								className="rounded border px-1.5 py-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
							>
								Revoke
							</button>
						</div>
					))}
				</div>
			) : null}
		</div>
	);
}

function SectionHeader({ children, right }: { children: string; right?: ReactNode }) {
	return (
		<div className="flex items-center border-b bg-muted/30 px-4 py-1.5 text-[11px] font-medium text-muted-foreground">
			{children}
			{right ? <span className="ml-auto">{right}</span> : null}
		</div>
	);
}

export function SystemTabConnected({ actorId }: { actorId: string }) {
	// Every section here (process tree, software commands, mounts) is
	// enumerated by the running VM, so opening the tab would wake a sleeping
	// one. Gate first.
	return (
		<VmBootGate
			actorId={actorId}
			note="VM not booted — processes, software, and mounts are enumerated by the running VM."
			actionLabel="Boot the VM and show system info"
		>
			<SystemLoaded actorId={actorId} />
		</VmBootGate>
	);
}

/** Label-over-value cell for the overview grid. */
function OverviewField({ label, children }: { label: string; children: ReactNode }) {
	return (
		<div className="min-w-0">
			<div className="text-muted-foreground/70">{label}</div>
			<div className="flex min-w-0 items-center gap-1 font-mono">{children}</div>
		</div>
	);
}

function SystemLoaded({ actorId }: { actorId: string }) {
	const [software, mounts] = useSuspenseQueries({
		queries: [
			agentOsSource.softwareQueryOptions(actorId),
			agentOsSource.mountsQueryOptions(actorId),
		],
	});
	const processCounts = useProcessCounts(actorId);
	const health = useQuery(healthQueryOptions(actorId));
	const sessions = health.data?.sessions;
	return (
		<ScrollArea className="h-full min-h-0">
			<section>
				<SectionHeader right={<VmStatusBadges actorId={actorId} />}>Overview</SectionHeader>
				{/* Inside-the-VM facts only: actor identity (id, key, runner) lives
				    in the dashboard's own Metadata tab. */}
				<div className="grid grid-cols-2 gap-x-8 gap-y-2 border-b px-4 py-3 text-xs sm:grid-cols-3">
					<OverviewField label="Live sessions">
						{sessions == null ? "—" : String(sessions)}
					</OverviewField>
					<OverviewField label="Processes">
						{`${processCounts.running} running / ${processCounts.total}`}
					</OverviewField>
					<OverviewField label="Software / mounts">
						{`${software.data.length} / ${mounts.data.length}`}
					</OverviewField>
				</div>
			</section>
			<section>
				<SectionHeader>Processes</SectionHeader>
				<div className="border-b">
					<ProcessTable actorId={actorId} />
				</div>
			</section>
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
			<section>
				<SectionHeader>Preview links</SectionHeader>
				<PreviewLinks />
			</section>
		</ScrollArea>
	);
}
