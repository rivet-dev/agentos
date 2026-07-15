// Shared VM status strip rendered above every tab (mounted in main.tsx):
// lifecycle dot + session count on the left, runtime warnings badge + panel on
// the right. Data comes from the observe-only `getRuntimeHealth` action polled
// at 5s (see lib/source.ts) with instant flips from the `vmBooted`/`vmShutdown`
// broadcasts; when the actor doesn't provide the action (older runtime) the
// strip renders nothing, preserving stock behavior.
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { relativeTime, StatusDot, type DotColor } from "./common";
import { isMissingHealthAction } from "./lib/health";
import { healthQueryOptions } from "./lib/source";
import { useAgentOsActor } from "./lib/rivet";
import type { RuntimeHealth, VmShutdownPayload } from "./lib/types";
import React from "react";

type LiveVmState =
	| { kind: "booted" }
	| { kind: "shutdown"; reason?: string }
	| null;

// `null` = nothing worth a bar: the VM is simply healthy (or still being
// checked). The strip renders only when it carries signal — the session count
// lives in the transcript's Sessions column, not here.
function stripState(
	health: RuntimeHealth | undefined,
	live: LiveVmState,
): { color: DotColor; label: string } | null {
	if (live?.kind === "shutdown") {
		const reason = live.reason ?? "unknown";
		return reason === "error"
			? { color: "red", label: "VM shut down after an error" }
			: { color: "muted", label: `VM shut down (${reason})` };
	}
	if (!health) return null;
	if (health.sidecar && health.sidecar.state !== "ready") {
		return { color: "red", label: `sidecar ${health.sidecar.state}` };
	}
	if (!health.booted && live?.kind !== "booted") {
		return { color: "muted", label: "VM not booted (boots on first session)" };
	}
	return null;
}

export function VmStatusStrip({ actorId }: { actorId: string }) {
	const health = useQuery(healthQueryOptions(actorId));
	const [live, setLive] = useState<LiveVmState>(null);
	const [panelOpen, setPanelOpen] = useState(false);

	// Live lifecycle flips between polls. Broadcast names aren't in the typed
	// event schema (Rust owns broadcasts) — same cast pattern as transcript.tsx.
	const actor = useAgentOsActor();
	const useAgentEvent = actor.useEvent as (
		name: string,
		handler: (payload: unknown) => void,
	) => void;
	useAgentEvent("vmBooted", () => {
		setLive({ kind: "booted" });
		void health.refetch();
	});
	useAgentEvent("vmShutdown", (payload) => {
		setLive({ kind: "shutdown", reason: (payload as VmShutdownPayload | undefined)?.reason });
		void health.refetch();
	});

	// Actor doesn't provide getRuntimeHealth → hide the strip entirely.
	if (health.error && isMissingHealthAction(health.error)) return null;

	const data = health.data;
	const state = health.error
		? { color: "amber" as DotColor, label: "health unavailable" }
		: stripState(data, live);
	const warningCount = (data?.warnings.length ?? 0) + (data?.agentExits.length ?? 0);
	// Nothing to say (healthy VM, no warnings) → no bar at all.
	if (!state && warningCount === 0) return null;
	const { color, label } = state ?? { color: "amber" as DotColor, label: "" };

	return (
		<div className="shrink-0 border-b bg-muted/30">
			<div className="flex items-center gap-2 px-3 py-1.5 text-xs">
				<StatusDot color={warningCount > 0 && color === "green" ? "amber" : color} />
				<span className="text-muted-foreground">{label}</span>
				<span className="ml-auto" />
				{warningCount > 0 ? (
					<button
						type="button"
						onClick={() => setPanelOpen((v) => !v)}
						className="inline-flex items-center gap-1.5 rounded border border-amber-500/40 px-2 py-0.5 text-[11px] text-amber-500 transition-colors hover:bg-amber-500/10"
					>
						<StatusDot color="amber" className="size-1.5" />
						{warningCount} warning{warningCount === 1 ? "" : "s"}
						<span aria-hidden="true">{panelOpen ? "▾" : "▸"}</span>
					</button>
				) : null}
			</div>
			{panelOpen && data ? (
				<div className="max-h-48 overflow-y-auto border-t px-3 py-2 text-[11px] leading-relaxed">
					{data.warnings.length > 0 ? (
						<div className="mb-2">
							<div className="mb-1 font-medium uppercase tracking-wider text-muted-foreground">
								Limit warnings
							</div>
							{data.warnings.map((w, i) => (
								<div key={`w-${i}`} className="flex items-center gap-2 font-mono">
									<StatusDot color="amber" className="size-1.5" />
									<span>
										{w.limit} {w.observed}/{w.capacity} · {w.fillPercent}%
									</span>
									<span className="ml-auto text-muted-foreground">{relativeTime(w.ts)}</span>
								</div>
							))}
						</div>
					) : null}
					{data.agentExits.length > 0 ? (
						<div className="mb-2">
							<div className="mb-1 font-medium uppercase tracking-wider text-muted-foreground">
								Agent exits
							</div>
							{data.agentExits.map((e, i) => (
								<div key={`e-${i}`} className="flex items-center gap-2 font-mono">
									<StatusDot color={e.restart === "restarted" ? "amber" : "red"} className="size-1.5" />
									<span>
										{e.agentType} exit {e.exitCode ?? "?"} · {e.restart} ({e.restartCount})
									</span>
									<span className="ml-auto text-muted-foreground">{relativeTime(e.ts)}</span>
								</div>
							))}
						</div>
					) : null}
					{data.stderrTail.length > 0 ? (
						<div>
							<div className="mb-1 font-medium uppercase tracking-wider text-muted-foreground">
								Adapter stderr (tail)
							</div>
							<pre className="whitespace-pre-wrap break-words font-mono text-muted-foreground">
								{data.stderrTail.map((l) => l.line).join("\n")}
							</pre>
						</div>
					) : null}
				</div>
			) : null}
		</div>
	);
}
