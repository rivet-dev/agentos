// Actor list persisted in localStorage. Each entry is one Agent OS VM; its
// `id` is used verbatim as the RivetKit actor key, so the same VM is reachable
// across page reloads (and its running shells can be reconnected).

export interface ActorEntry {
	id: string;
	name: string;
}

const KEY = "agentos.browser-terminal.actors";

export function loadActors(): ActorEntry[] {
	try {
		const raw = localStorage.getItem(KEY);
		if (!raw) return [];
		const parsed = JSON.parse(raw);
		if (!Array.isArray(parsed)) return [];
		return parsed.filter(
			(e): e is ActorEntry =>
				e && typeof e.id === "string" && typeof e.name === "string",
		);
	} catch {
		return [];
	}
}

export function saveActors(actors: ActorEntry[]): void {
	localStorage.setItem(KEY, JSON.stringify(actors));
}

function randomId(): string {
	if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
		return crypto.randomUUID();
	}
	return `vm-${Math.random().toString(36).slice(2, 10)}`;
}

export function createActor(existing: ActorEntry[]): ActorEntry {
	const n = existing.length + 1;
	return { id: `vm-${randomId()}`, name: `VM ${n}` };
}
