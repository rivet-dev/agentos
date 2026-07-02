import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { lazy, type ComponentType, StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { setAuth, tabIdFromUrl } from "./lib/actor-client";
import { isMockMode, mockTabId } from "./lib/mock";
import { TabBoundary } from "./tab-boundary";
import React from "react";

import "./styles.css";

// Tab registry: id → lazy component. Add a tab here + register the same id in
// actor.ts `inspectorTabs` pointing `source` at this shared asset dir.
const TABS: Record<string, () => Promise<{ default: ComponentType<{ actorId: string }> }>> = {
	software: () =>
		import("./tabs/software").then((m) => ({ default: m.SoftwareTabConnected })),
	processes: () =>
		import("./tabs/processes").then((m) => ({ default: m.ProcessesTabConnected })),
	filesystem: () =>
		import("./tabs/filesystem").then((m) => ({ default: m.FilesystemTabConnected })),
	mounts: () =>
		import("./tabs/mounts").then((m) => ({ default: m.MountsTabConnected })),
	transcript: () =>
		import("./tabs/transcript").then((m) => ({ default: m.TranscriptTabConnected })),
};

const queryClient = new QueryClient({
	defaultOptions: {
		queries: {
			// VM-backed actions (listProcesses/readdir/readFile/stat) time out on
			// the FIRST call after the actor's VM hibernates and cold-wakes;
			// retry so the query recovers once the VM is warm.
			retry: 3,
			retryDelay: (attempt) => Math.min(1500 * 2 ** attempt, 8000),
			refetchOnWindowFocus: false,
			staleTime: 5_000,
		},
	},
});

function App() {
	const [actorId, setActorId] = useState<string>();

	useEffect(() => {
		// Dummy mode: no actor/handshake — render immediately against fixtures.
		if (isMockMode()) {
			setActorId("mock-actor");
			return;
		}
		// Tell the dashboard we're ready (clears its "Connecting…" overlay).
		parent.postMessage({ type: "ready", v: 1 }, "*");
		const onMessage = (event: MessageEvent) => {
			const d = event.data || {};
			if (d.type === "init" && d.actorId && d.authToken) {
				setAuth({ actorId: d.actorId, authToken: d.authToken });
				setActorId(d.actorId);
			}
		};
		window.addEventListener("message", onMessage);
		return () => window.removeEventListener("message", onMessage);
	}, []);

	// In mock mode the URL usually has no `custom-tabs/<id>` segment, so fall
	// back to `?tab=<id>`.
	const tabId = tabIdFromUrl() ?? (isMockMode() ? mockTabId() : undefined);
	const loader = tabId ? TABS[tabId] : undefined;

	if (!loader) {
		return <div style={{ padding: 16 }}>Unknown inspector tab: {String(tabId)}</div>;
	}
	if (!actorId) {
		return <div style={{ padding: 16, color: "#8a8a90" }}>Connecting…</div>;
	}

	const Tab = lazy(loader);
	// One boundary catches both the code-split chunk load and the tab's
	// useSuspenseQuery data load.
	return (
		<TabBoundary>
			<Tab actorId={actorId} />
		</TabBoundary>
	);
}

createRoot(document.getElementById("root")!).render(
	<StrictMode>
		<QueryClientProvider client={queryClient}>
			<App />
		</QueryClientProvider>
	</StrictMode>,
);
