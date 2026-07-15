// Global permission-approval banner, mounted in main.tsx above every tab (each
// tab is its own iframe, so "global" means "in every iframe's chrome"). An
// agent blocked on a `permissionRequest` broadcast otherwise looks frozen: the
// turn hangs until the runtime auto-rejects at its permission timeout (~120s).
// Cards come from two sources merged on `sessionId:permissionId`: a one-off
// `listPendingPermissions` backfill on mount (requests broadcast while no
// inspector iframe was open) plus the live `permissionRequest` stream. The
// `permissionResolved` broadcast drops cards another viewer already answered.
import { useQuery } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { ChevronRight } from "./common";
import { isInspectorActionError } from "./lib/actor-client";
import { useAgentOsActor } from "./lib/rivet";
import { agentOsSource, pendingPermissionsQueryOptions } from "./lib/source";
import type { PermissionRequestPayload, PermissionResolvedPayload } from "./lib/types";
import React from "react";

// Mirrors the runtime's PERMISSION_TIMEOUT_MS: past this the reply slot is
// gone server-side, so the card flips to expired instead of offering buttons.
const PERMISSION_TIMEOUT_MS = 120_000;
// Bounded queue: drop the oldest card beyond this (they expire server-side
// anyway; an unbounded queue would just stack dead cards).
const MAX_CARDS = 32;

interface PermissionCard {
	sessionId: string;
	permissionId: string;
	description?: string;
	params: Record<string, unknown>;
	receivedAt: number;
	state: "pending" | "busy" | "expired" | "failed";
	error?: string;
}

function cardKey(card: Pick<PermissionCard, "sessionId" | "permissionId">): string {
	return `${card.sessionId}:${card.permissionId}`;
}

const REPLIES = [
	{ reply: "once", label: "Approve once" },
	{ reply: "always", label: "Always allow" },
	{ reply: "reject", label: "Deny" },
] as const;

export function PermissionPrompts({ actorId }: { actorId: string }) {
	const [cards, setCards] = useState<PermissionCard[]>([]);
	const cardsRef = useRef(cards);
	cardsRef.current = cards;

	// One-off backfill of requests broadcast before this iframe subscribed.
	// `data === null` means the runtime predates `listPendingPermissions`
	// (contract-layer error) — stay live-only silently.
	const backfill = useQuery(pendingPermissionsQueryOptions(actorId));
	const backfillRows = backfill.data;
	useEffect(() => {
		if (!backfillRows || backfillRows.length === 0) return;
		setCards((prev) => {
			// Live cards win the dedupe: a broadcast that raced the fetch already
			// carries the same request (identity is sessionId:permissionId).
			const have = new Set(prev.map(cardKey));
			const added = backfillRows
				.filter((row) => !have.has(cardKey(row)))
				.map(
					(row): PermissionCard => ({
						sessionId: row.sessionId,
						permissionId: row.permissionId,
						description: row.description,
						params: row.params ?? {},
						// Runtime receipt time, so the expiry countdown measures the
						// request's real age rather than when this viewer opened.
						receivedAt: row.requestedAt,
						state: "pending",
					}),
				);
			if (added.length === 0) return prev;
			return [...prev, ...added]
				.sort((a, b) => a.receivedAt - b.receivedAt)
				.slice(-MAX_CARDS);
		});
	}, [backfillRows]);

	// Broadcast names aren't in the typed event schema (Rust owns broadcasts) —
	// same cast pattern as transcript.tsx / vm-status-strip.tsx.
	const actor = useAgentOsActor();
	const useAgentEvent = actor.useEvent as (
		name: string,
		handler: (payload: unknown) => void,
	) => void;
	useAgentEvent("permissionRequest", (payload) => {
		const p = payload as PermissionRequestPayload | undefined;
		const request = p?.request;
		if (!p?.sessionId || !request?.permissionId) return;
		setCards((prev) => {
			const next: PermissionCard = {
				sessionId: p.sessionId,
				permissionId: request.permissionId,
				description: request.description,
				params: request.params ?? {},
				receivedAt: Date.now(),
				state: "pending",
			};
			const deduped = prev.filter((c) => cardKey(c) !== cardKey(next));
			return [...deduped, next].slice(-MAX_CARDS);
		});
	});
	// Another viewer (or a headless client) answered: its card is stale here,
	// so drop it outright — an untouched card needs no "handled elsewhere"
	// residue. A busy card stays: its own in-flight respond reports the
	// outcome (the "already answered or expired" error → quiet expired state).
	useAgentEvent("permissionResolved", (payload) => {
		const p = payload as PermissionResolvedPayload | undefined;
		if (!p?.sessionId || !p?.permissionId) return;
		const key = cardKey(p);
		setCards((prev) => prev.filter((c) => cardKey(c) !== key || c.state === "busy"));
	});

	// Flip pending cards to expired once the runtime's reply slot is gone.
	useEffect(() => {
		if (!cards.some((c) => c.state === "pending" || c.state === "busy")) return;
		const timer = setInterval(() => {
			const now = Date.now();
			setCards((prev) =>
				prev.map((c) =>
					c.state === "pending" && now - c.receivedAt > PERMISSION_TIMEOUT_MS
						? { ...c, state: "expired" as const }
						: c,
				),
			);
		}, 5_000);
		return () => clearInterval(timer);
	}, [cards]);

	const respond = async (card: PermissionCard, reply: "once" | "always" | "reject") => {
		const key = cardKey(card);
		const patch = (changes: Partial<PermissionCard>) =>
			setCards((prev) => prev.map((c) => (cardKey(c) === key ? { ...c, ...changes } : c)));
		patch({ state: "busy" });
		try {
			await agentOsSource.respondPermission(card.sessionId, card.permissionId, reply);
			setCards((prev) => prev.filter((c) => cardKey(c) !== key));
		} catch (error) {
			// A missing reply slot means the request timed out server-side or another
			// viewer answered first — quiet outcome, not an error card.
			const message = error instanceof Error ? error.message : String(error);
			if (/pending|not found|expired|timed out|already/i.test(message)) {
				patch({ state: "expired" });
				return;
			}
			const hint = isInspectorActionError(error) ? ` — ${error.hint}` : "";
			patch({ state: "failed", error: `${message}${hint}` });
		}
	};

	const dismiss = (card: PermissionCard) =>
		setCards((prev) => prev.filter((c) => cardKey(c) !== cardKey(card)));

	if (cards.length === 0) return null;

	return (
		<div className="shrink-0 border-b">
			{cards.map((card) => (
				<div
					key={cardKey(card)}
					className="flex flex-col gap-1.5 border-b border-amber-500/20 bg-amber-500/5 px-3 py-2 text-xs last:border-b-0"
				>
					<div className="flex items-start gap-2">
						<div className="min-w-0 flex-1">
							<div className="text-amber-500">
								Agent requests permission
								<span className="ml-2 font-mono text-[10px] text-muted-foreground/70">
									session …{card.sessionId.slice(-10)}
								</span>
							</div>
							<div className="mt-0.5 text-foreground/90">
								{card.description ?? card.permissionId}
							</div>
							{Object.keys(card.params).length > 0 ? (
								<details className="group mt-1 text-muted-foreground/70">
									<summary className="flex cursor-pointer list-none items-center gap-1 [&::-webkit-details-marker]:hidden">
										<ChevronRight className="size-3 transition-transform group-open:rotate-90" />
										Details
									</summary>
									<pre className="mt-1 max-h-40 overflow-y-auto whitespace-pre-wrap break-words rounded bg-muted/50 p-2 font-mono text-[11px] leading-relaxed">
										{JSON.stringify(card.params, null, 2)}
									</pre>
								</details>
							) : null}
						</div>
						{card.state === "pending" || card.state === "busy" ? (
							<div className="flex shrink-0 items-center gap-1.5">
								{REPLIES.map(({ reply, label }) => (
									<button
										key={reply}
										type="button"
										disabled={card.state === "busy"}
										onClick={() => void respond(card, reply)}
										className={
											reply === "once"
												? "rounded-md bg-primary px-2.5 py-1 text-primary-foreground transition-opacity hover:opacity-90 disabled:opacity-40"
												: reply === "reject"
													? "rounded-md border border-destructive/50 px-2.5 py-1 text-destructive transition-colors hover:bg-destructive/10 disabled:opacity-40"
													: "rounded-md border px-2.5 py-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-40"
										}
									>
										{label}
									</button>
								))}
							</div>
						) : (
							<div className="flex shrink-0 items-center gap-2">
								<span className="text-muted-foreground">
									{card.state === "expired"
										? "Expired or handled elsewhere"
										: (card.error ?? "Reply failed")}
								</span>
								<button
									type="button"
									onClick={() => dismiss(card)}
									className="rounded border px-2 py-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
								>
									Dismiss
								</button>
							</div>
						)}
					</div>
				</div>
			))}
		</div>
	);
}
