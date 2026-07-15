import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { lazy, type ComponentType, StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { isInspectorActionError, tabIdFromUrl } from "./lib/actor-client";
import { RivetProvider } from "./lib/rivet";
import { PermissionPrompts } from "./permission-prompts";
import { TabBoundary } from "./tab-boundary";
import { VmStatusStrip } from "./vm-status-strip";
import React from "react";

import "./styles.css";

// Tab registry: id → lazy component. Add a tab here + register the same id in
// actor.ts `inspectorTabs` pointing `source` at this shared asset dir.
const TABS: Record<string, () => Promise<{ default: ComponentType<{ actorId: string }> }>> = {
	transcript: () =>
		import("./tabs/transcript").then((m) => ({ default: m.TranscriptTabConnected })),
	terminal: () =>
		import("./tabs/terminal").then((m) => ({ default: m.TerminalTabConnected })),
	filesystem: () =>
		import("./tabs/filesystem").then((m) => ({ default: m.FilesystemTabConnected })),
	processes: () =>
		import("./tabs/processes").then((m) => ({ default: m.ProcessesTabConnected })),
	system: () =>
		import("./tabs/system").then((m) => ({ default: m.SystemTabConnected })),
};

// Hosts vendor built copies of this bundle and pin their tab-id config at
// server start, so ids that existed in older configs must keep rendering.
// Software and Mounts merged into System; route their ids there.
const LEGACY_TAB_ALIASES: Record<string, string> = {
	software: "system",
	mounts: "system",
};

const queryClient = new QueryClient({
	defaultOptions: {
		queries: {
			// VM-backed actions (listProcesses/readdir/readFile/stat) time out on
			// the FIRST call after the actor's VM hibernates and cold-wakes;
			// retry so the query recovers once the VM is warm. Contract/auth
			// failures are permanent for this actor — retrying only delays the
			// error UI by the full backoff, so fail those immediately.
			retry: (failureCount, error) => {
				if (
					isInspectorActionError(error) &&
					(error.layer === "contract" || error.layer === "auth")
				) {
					return false;
				}
				return failureCount < 3;
			},
			retryDelay: (attempt) => Math.min(1500 * 2 ** attempt, 8000),
			refetchOnWindowFocus: false,
			staleTime: 5_000,
		},
	},
});

function App() {
	const [auth, setAuthState] = useState<{ actorId: string; authToken: string }>();

	useEffect(() => {
		// Tell the dashboard we're ready (clears its "Connecting…" overlay).
		parent.postMessage({ type: "ready", v: 1 }, "*");
		const onMessage = (event: MessageEvent) => {
			const d = event.data || {};
			if (d.type === "init" && d.actorId && d.authToken) {
				setAuthState({ actorId: d.actorId, authToken: d.authToken });
			}
		};
		window.addEventListener("message", onMessage);
		return () => window.removeEventListener("message", onMessage);
	}, []);

	const tabId = tabIdFromUrl();
	const loader = tabId ? TABS[LEGACY_TAB_ALIASES[tabId] ?? tabId] : undefined;

	if (!loader) {
		return <div style={{ padding: 16 }}>Unknown inspector tab: {String(tabId)}</div>;
	}
	if (!auth) {
		return <div style={{ padding: 16, color: "#8a8a90" }}>Connecting…</div>;
	}

	const Tab = lazy(loader);
	// RivetProvider owns the shared rivetkit client (typed `useActor` + the
	// `callAction` transport). One boundary catches both the code-split chunk
	// load and the tab's useSuspenseQuery data load. The status strip sits
	// outside the boundary so a broken tab still shows VM health.
	return (
		<RivetProvider actorId={auth.actorId} authToken={auth.authToken}>
			<div className="flex h-full min-h-0 flex-col">
				<VmStatusStrip actorId={auth.actorId} />
				{/* Permission prompts sit with the strip, outside the boundary: an
				    agent blocked on approval must stay answerable from every tab even
				    when the tab body itself is broken. */}
				<PermissionPrompts actorId={auth.actorId} />
				<div className="min-h-0 flex-1">
					<TabBoundary>
						<Tab actorId={auth.actorId} />
					</TabBoundary>
				</div>
			</div>
		</RivetProvider>
	);
}

createRoot(document.getElementById("root")!).render(
	<StrictMode>
		<QueryClientProvider client={queryClient}>
			<App />
		</QueryClientProvider>
	</StrictMode>,
);
