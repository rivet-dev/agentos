import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { StatusDot } from "./common";
import { useAgentOsActor } from "./lib/rivet";
import { healthQueryOptions } from "./lib/source";

export function VmStatusBadges({ actorId }: { actorId: string; align?: "left" | "right" }) {
	const status = useQuery(healthQueryOptions(actorId));
	const [eventState, setEventState] = useState<string | null>(null);
	const actor = useAgentOsActor();
	actor.useEvent("vmBooted", () => {
		setEventState("ready");
		void status.refetch();
	});
	actor.useEvent("vmShutdown", () => {
		setEventState("stopped");
		void status.refetch();
	});

	if (status.error) {
		return (
		<span className="inline-flex items-center gap-1.5 rounded border px-2 py-0.5 text-[11px] text-muted-foreground">
			<StatusDot color="muted" /> status unavailable
		</span>
		);
	}
	const data = status.data;
	if (!data) return null;
	const lifecycle = eventState ?? data.lifecycle;
	const issueCount = data.issues.length;
	const tone = lifecycle === "ready" && issueCount === 0 ? "green" : lifecycle === "failed" ? "red" : "muted";
	return (
		<span className="inline-flex items-center gap-1.5 rounded border px-2 py-0.5 text-[11px] text-muted-foreground">
			<StatusDot color={tone} />
			{lifecycle}
			{issueCount > 0 ? ` · ${issueCount} issue${issueCount === 1 ? "" : "s"}` : ""}
		</span>
	);
}
