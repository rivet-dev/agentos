// Shared persisted-shells store for the terminal tab AND the background
// output capture that runs in every OTHER tab document. The dashboard swaps
// the inspector iframe per tab, so shell state lives in sessionStorage keyed
// by actor; whichever tab document is currently alive keeps appending live
// `shellData` output to it. That is what makes a tab switch lossless: the
// terminal reattaches to a scrollback that never stopped recording.

export const MAX_SCROLLBACK_CHARS = 128 * 1024;

export const shellsKey = (actorId: string) => `agentos-inspector:shells:${actorId}`;

export interface PersistedShellRecord {
	shellId: string;
	name?: string;
	openedAt?: number;
	scrollback: string;
	status?: "live" | "exited" | "dead";
	exitCode?: number;
}

export interface PersistedShells {
	shells: PersistedShellRecord[];
	active: string | null;
	counter?: number;
	/** When this payload was written — lets the terminal detect a real capture
	 * gap (no inspector document alive) vs an ordinary tab switch. */
	savedAt?: number;
	/** Terminal dimensions at save time, so capture mirrors (lib/shell-mirror)
	 * emulate at the size the PTYs were actually resized to. */
	cols?: number;
	rows?: number;
}

export function loadPersistedShells(actorId: string): PersistedShells | null {
	try {
		const raw = sessionStorage.getItem(shellsKey(actorId));
		if (!raw) return null;
		const parsed = JSON.parse(raw) as PersistedShells;
		return Array.isArray(parsed?.shells) && parsed.shells.length > 0 ? parsed : null;
	} catch {
		return null;
	}
}

export function savePersistedShells(actorId: string, payload: PersistedShells): void {
	try {
		sessionStorage.setItem(
			shellsKey(actorId),
			JSON.stringify({ ...payload, savedAt: Date.now() }),
		);
	} catch (error) {
		console.warn("agentos inspector: failed to persist shells", error);
	}
}

export function clearPersistedShells(actorId: string): void {
	sessionStorage.removeItem(shellsKey(actorId));
}

// Cap scrollback without starting the kept tail mid-character or (bounded
// search) mid-line: a cut surrogate pair or half an ANSI escape replays as
// garbage on the next shell switch.
export function trimScrollback(text: string): string {
	if (text.length <= MAX_SCROLLBACK_CHARS) return text;
	let out = text.slice(-MAX_SCROLLBACK_CHARS);
	const first = out.charCodeAt(0);
	if (first >= 0xdc00 && first <= 0xdfff) out = out.slice(1);
	const nl = out.indexOf("\n");
	if (nl !== -1 && nl < 4096) out = out.slice(nl + 1);
	return out;
}
