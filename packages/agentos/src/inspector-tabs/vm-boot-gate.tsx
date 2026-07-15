// Observe-first gate for tabs whose queries boot a sleeping VM (every
// non-observe action runs ensure_vm before dispatch). When the health action
// reports the VM asleep, hold the tab body behind an explicit button so merely
// opening a tab never wakes anything. Runtimes without getRuntimeHealth (or
// while health is still loading) skip the gate and behave as before.
import { useQuery } from "@tanstack/react-query";
import { type ReactNode, useState } from "react";
import { AgentOsEmpty, AgentOsWordmark } from "./common";
import { healthQueryOptions } from "./lib/source";
import React from "react";

export function VmBootGate({
	actorId,
	note,
	actionLabel,
	children,
}: {
	actorId: string;
	/** What the user is missing while the VM sleeps. */
	note: string;
	/** Button label; must say it boots the VM. */
	actionLabel: string;
	children: ReactNode;
}) {
	const health = useQuery(healthQueryOptions(actorId));
	const [proceed, setProceed] = useState(false);
	if (health.data && !health.data.booted && !proceed) {
		return (
			<AgentOsEmpty>
				<div className="flex max-w-sm flex-col items-center gap-2">
					<AgentOsWordmark className="mb-3 w-44" />
					<span>{note}</span>
					<button
						type="button"
						onClick={() => setProceed(true)}
						className="rounded-md border px-2.5 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
					>
						{actionLabel}
					</button>
				</div>
			</AgentOsEmpty>
		);
	}
	return <>{children}</>;
}
