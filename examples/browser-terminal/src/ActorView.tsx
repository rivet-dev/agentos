import { useCallback, useEffect, useRef, useState } from "react";
import { ACTOR_NAME, useActor } from "./rivet";
import { type ReadResult, TerminalPane } from "./TerminalPane";

interface Tab {
	shellId: string;
	title: string;
}

export function ActorView({ actorId }: { actorId: string }) {
	const agent = useActor({ name: ACTOR_NAME, key: actorId });
	const conn = agent.connection;

	const [tabs, setTabs] = useState<Tab[]>([]);
	const [active, setActive] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const initedRef = useRef(false);

	// On (re)connect, adopt any shells already running in the VM.
	useEffect(() => {
		if (!conn || initedRef.current) return;
		initedRef.current = true;
		conn
			.listShells()
			.then((shells: { shellId: string; title: string }[]) => {
				if (shells.length === 0) return;
				setTabs(shells.map((s) => ({ shellId: s.shellId, title: s.title })));
				setActive(shells[0].shellId);
			})
			.catch((e: unknown) => setError(String(e)));
	}, [conn]);

	// Reset when switching to a different actor.
	useEffect(() => {
		initedRef.current = false;
		setTabs([]);
		setActive(null);
		setError(null);
	}, [actorId]);

	const openShell = useCallback(async () => {
		if (!conn) return;
		setBusy(true);
		setError(null);
		try {
			const { shellId } = await conn.openShell({ cols: 80, rows: 24 });
			setTabs((prev) => [
				...prev,
				{ shellId, title: `shell ${prev.length + 1}` },
			]);
			setActive(shellId);
		} catch (e) {
			setError(String(e));
		} finally {
			setBusy(false);
		}
	}, [conn]);

	const dropTab = useCallback((shellId: string) => {
		setTabs((prev) => {
			const next = prev.filter((t) => t.shellId !== shellId);
			setActive((cur) =>
				cur === shellId ? (next[next.length - 1]?.shellId ?? null) : cur,
			);
			return next;
		});
	}, []);

	const closeTab = useCallback(
		async (shellId: string) => {
			dropTab(shellId);
			try {
				await conn?.closeShell(shellId);
			} catch {
				// already gone
			}
		},
		[conn, dropTab],
	);

	return (
		<div className="actor-view">
			<div className="tabbar">
				{tabs.map((t) => (
					<div
						key={t.shellId}
						className={`tab ${t.shellId === active ? "tab-active" : ""}`}
						onClick={() => setActive(t.shellId)}
					>
						<span className="tab-title">{t.title}</span>
						<button
							type="button"
							className="tab-close"
							title="Close terminal"
							onClick={(e) => {
								e.stopPropagation();
								void closeTab(t.shellId);
							}}
						>
							×
						</button>
					</div>
				))}
				<button
					type="button"
					className="tab-new"
					disabled={!conn || busy}
					onClick={() => void openShell()}
					title="New terminal"
				>
					+
				</button>
				<span className="conn-status">
					{conn ? "connected" : "connecting…"}
				</span>
			</div>

			{error && <div className="error-banner">{error}</div>}

			<div className="terminals">
				{tabs.length === 0 && (
					<div className="empty-hint">
						{conn
							? "No terminals yet — click + to open one."
							: "Connecting to the VM…"}
					</div>
				)}
				{tabs.map((t) => (
					<TerminalPane
						key={t.shellId}
						shellId={t.shellId}
						active={t.shellId === active}
						// TerminalPane does local echo + line editing and sends whole
						// `\n`-terminated lines (and control bytes) here.
						onInput={(text) => conn?.writeShell(t.shellId, text)}
						onResize={(cols, rows) => conn?.resizeShell(t.shellId, cols, rows)}
						readShell={async (fromOffset): Promise<ReadResult | undefined> =>
							conn?.readShell(t.shellId, fromOffset)
						}
						onGone={dropTab}
					/>
				))}
			</div>
		</div>
	);
}
