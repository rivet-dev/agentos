import { useEffect, useRef, useState } from "react";

type TerminalKind = "shell" | "pi";

interface LocalTab {
	id: string;
	kind: TerminalKind;
	title: string;
}

type LocalFrameWindow = Window;

declare global {
	interface Window {
		__agentOSBrowserTerminalDemo?: {
			screens(): Record<string, string>;
			write(id: string, data: string): Promise<void>;
			askPi(id: string, prompt: string): Promise<unknown>;
		};
	}
}

let nextTab = 1;

export function BrowserApp() {
	const [tabs, setTabs] = useState<LocalTab[]>([]);
	const [active, setActive] = useState<string | null>(null);
	const frames = useRef(new Map<string, HTMLIFrameElement>());

	useEffect(() => {
		window.__agentOSBrowserTerminalDemo = {
			screens: () =>
				Object.fromEntries(
					tabs.map((tab) => {
						const frame = frames.current.get(tab.id)?.contentWindow as LocalFrameWindow | null;
						return [
							tab.id,
							tab.kind === "shell"
								? (frame?.__realTerminal?.screen() ?? "")
								: (frame?.__piTui?.screen() ?? ""),
						];
					}),
				),
			write: async (id, data) => {
				const tab = tabs.find((candidate) => candidate.id === id);
				const frame = frames.current.get(id)?.contentWindow as LocalFrameWindow | null;
				if (!tab || !frame) throw new Error(`unknown browser-local terminal ${id}`);
				if (tab.kind === "shell") await frame.__realTerminal?.write(data);
				else await frame.__piTui?.write(data);
			},
			askPi: async (id, prompt) => {
				const frame = frames.current.get(id)?.contentWindow as LocalFrameWindow | null;
				if (!frame?.__piTui) throw new Error(`Pi terminal ${id} is not ready`);
				return frame.__piTui.ask(prompt);
			},
		};
		return () => {
			delete window.__agentOSBrowserTerminalDemo;
		};
	}, [tabs]);

	const open = (kind: TerminalKind) => {
		const sequence = nextTab++;
		const tab: LocalTab = {
			id: `browser-${kind}-${sequence}`,
			kind,
			title: kind === "pi" ? "pi" : `shell ${sequence}`,
		};
		setTabs((current) => [...current, tab]);
		setActive(tab.id);
	};

	const close = (id: string) => {
		frames.current.delete(id);
		setTabs((current) => {
			const next = current.filter((tab) => tab.id !== id);
			setActive((selected) => selected === id ? (next.at(-1)?.id ?? null) : selected);
			return next;
		});
	};

	return (
		<div className="app">
			<aside className="sidebar">
				<div className="sidebar-header">
					<span className="brand">Agent OS</span>
					<span className="brand-sub">In-browser VM terminal</span>
				</div>
				<div className="runtime-card runtime-browser">
					<strong>Browser-local VM</strong>
					<span>WASM sidecar + kernel + VFS + PTY</span>
					<code>execution: this tab</code>
				</div>
				<div className="sidebar-foot">
					Shell and Pi execute locally in browser memory. Pi uses a clearly labeled deterministic demo model for its offline E2E response.
				</div>
				<a className="mode-link" href="/actor.html">Open Actor API version →</a>
			</aside>

			<main className="main">
				<div className="actor-view">
					<div className="mode-banner">
						<span className="mode-badge browser-badge">IN-BROWSER VM</span>
						<span>No Actor API. Runtime and PTYs execute in this tab.</span>
					</div>
					<div className="tabbar">
						{tabs.map((tab) => (
							<div key={tab.id} className={`tab ${tab.id === active ? "tab-active" : ""}`} onClick={() => setActive(tab.id)}>
								<span className="tab-title">{tab.title}</span>
								<button type="button" className="tab-close" title="Close terminal" onClick={(event) => { event.stopPropagation(); close(tab.id); }}>×</button>
							</div>
						))}
						<button type="button" className="tab-new" onClick={() => open("shell")}>+ shell</button>
						<button type="button" className="tab-new" onClick={() => open("pi")}>+ pi</button>
						<span className="conn-status">local · ready</span>
					</div>
					<div className="terminals">
						{tabs.length === 0 && <div className="empty-hint">No terminals yet — open a browser-local shell or Pi PTY.</div>}
						{tabs.map((tab) => (
							<iframe
								key={tab.id}
								ref={(frame) => { if (frame) frames.current.set(tab.id, frame); else frames.current.delete(tab.id); }}
								className="local-terminal-frame"
								style={{ display: tab.id === active ? "block" : "none" }}
								src={tab.kind === "shell" ? "/local-shell.html" : "/local-pi.html?mockModel=1"}
								title={`Browser-local ${tab.kind} terminal`}
							/>
						))}
					</div>
				</div>
			</main>
		</div>
	);
}
