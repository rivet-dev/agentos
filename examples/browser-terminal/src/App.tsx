import { useEffect, useState } from "react";
import { ActorView } from "./ActorView";
import { type ActorEntry, createActor, loadActors, saveActors } from "./store";

export function App() {
	const [actors, setActors] = useState<ActorEntry[]>(() => loadActors());
	const [selected, setSelected] = useState<string | null>(
		() => loadActors()[0]?.id ?? null,
	);

	useEffect(() => {
		saveActors(actors);
	}, [actors]);

	const addActor = () => {
		setActors((prev) => {
			const entry = createActor(prev);
			setSelected(entry.id);
			return [...prev, entry];
		});
	};

	const removeActor = (id: string) => {
		setActors((prev) => {
			const next = prev.filter((a) => a.id !== id);
			setSelected((cur) => (cur === id ? (next[0]?.id ?? null) : cur));
			return next;
		});
	};

	return (
		<div className="app">
			<aside className="sidebar">
				<div className="sidebar-header">
					<span className="brand">Agent OS</span>
					<span className="brand-sub">Terminals</span>
				</div>
				<button type="button" className="new-vm" onClick={addActor}>
					+ New VM
				</button>
				<div className="actor-list">
					{actors.length === 0 && (
						<div className="actor-empty">No VMs yet.</div>
					)}
					{actors.map((a) => (
						<div
							key={a.id}
							className={`actor-item ${a.id === selected ? "actor-active" : ""}`}
							onClick={() => setSelected(a.id)}
						>
							<div className="actor-name">{a.name}</div>
							<div className="actor-id">{a.id}</div>
							<button
								type="button"
								className="actor-remove"
								title="Remove VM from list"
								onClick={(e) => {
									e.stopPropagation();
									removeActor(a.id);
								}}
							>
								×
							</button>
						</div>
					))}
				</div>
				<div className="sidebar-foot">
					VM ids persist in localStorage; reopen one to reconnect to its
					running terminals.
				</div>
			</aside>

			<main className="main">
				{selected ? (
					<ActorView key={selected} actorId={selected} />
				) : (
					<div className="no-actor">
						Create a VM to open a terminal.
					</div>
				)}
			</main>
		</div>
	);
}
