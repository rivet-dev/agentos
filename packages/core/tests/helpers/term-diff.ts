import xterm from "@xterm/headless";

const { Terminal } = xterm;

export interface Grid {
	rows: string[];
	cursor: { x: number; y: number };
}

/**
 * Render a raw terminal byte stream through a fresh xterm emulator and snapshot
 * the resulting screen grid + cursor. Both the native-vim reference stream and
 * the wasm-vim stream are rendered through THIS SAME emulator, so any 1:1
 * divergence is a real difference in the escape sequences the two vims emit —
 * not an artifact of two different terminal parsers.
 */
function snapshot(
	term: InstanceType<typeof Terminal>,
	rows: number,
): Grid {
	const buf = term.buffer.active;
	const out: string[] = [];
	for (let y = 0; y < rows; y++) {
		const line = buf.getLine(y);
		out.push((line ? line.translateToString(true) : "").replace(/\s+$/, ""));
	}
	return { rows: out, cursor: { x: buf.cursorX, y: buf.cursorY } };
}

/**
 * A persistent terminal emulator that processes bytes exactly once, like a real
 * terminal. Feeding a stream as incremental deltas (not re-replaying the whole
 * cumulative buffer into a fresh emulator each step) is what makes the snapshot
 * faithful: vim's redraws assume prior screen state, so full-replay into a fresh
 * emulator double-draws scrolled content. Both the native reference and the wasm
 * candidate are driven through this same emulator type.
 */
export function createTerm(cols = 80, rows = 24) {
	const term = new Terminal({ cols, rows, allowProposedApi: true });
	return {
		write(delta: Uint8Array): Promise<void> {
			return new Promise((r) => term.write(Buffer.from(delta), () => r()));
		},
		grid(): Grid {
			return snapshot(term, rows);
		},
	};
}

/** One-shot render of a full stream (kept for simple non-incremental callers). */
export function renderGrid(
	raw: Uint8Array,
	cols = 80,
	rows = 24,
): Promise<Grid> {
	const t = createTerm(cols, rows);
	return t.write(raw).then(() => t.grid());
}

/**
 * Normalize the one unavoidable difference between the reference and the build
 * under test: the vim version string on the startup splash. Everything else
 * (geometry, ruler, insert indicator, typed text placement, cursor) must match
 * byte-for-cell.
 */
export function normalizeVersion(rows: string[]): string[] {
	return rows.map((r) =>
		r
			.replace(/VIM - Vi IMproved\s+version\s+[\d.]+/i, "VIM - Vi IMproved")
			.replace(/version\s+[\d.]+/i, "version")
			.replace(/\bIMproved\s+[\d.]+/i, "IMproved"),
	);
}

/**
 * Normalize the file path shown in the status/ruler line. The native reference
 * and the in-VM build necessarily open files at different paths (host /tmp vs
 * VM /work), so replace any `"...basename"` occurrence with a stable token. The
 * BASENAME is asserted separately; here we only neutralize the directory so the
 * rest of the line (byte counts, [New], ruler) compares 1:1.
 */
export function normalizePaths(rows: string[], basename: string): string[] {
	const q = basename.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
	const re = new RegExp(`"[^"]*${q}"`, "g");
	return rows.map((r) => r.replace(re, `"FILE"`));
}

/**
 * Collapse runs of interior whitespace to a single space. The status/ruler line
 * is horizontally aligned at a column vim computes slightly differently across
 * point releases (9.0 vs 9.2) — pure cosmetic alignment, not content. Collapsing
 * interior spacing makes the comparison alignment-independent while preserving
 * every token and its order (an empty content row stays empty; `~` stays `~`).
 */
export function normalizeSpacing(rows: string[]): string[] {
	return rows.map((r) => r.replace(/ {2,}/g, " ").trimEnd());
}

export interface GridDiff {
	equal: boolean;
	lines: string[];
}

/** Diff two grids row-by-row, returning a human-readable report. */
export function diffGrids(
	label: string,
	reference: Grid,
	candidate: Grid,
	{
		ignoreVersion = true,
		normalizeBasename,
	}: { ignoreVersion?: boolean; normalizeBasename?: string } = {},
): GridDiff {
	let refRows = reference.rows;
	let candRows = candidate.rows;
	if (normalizeBasename) {
		refRows = normalizePaths(refRows, normalizeBasename);
		candRows = normalizePaths(candRows, normalizeBasename);
	}
	refRows = normalizeSpacing(refRows);
	candRows = normalizeSpacing(candRows);
	const ref = ignoreVersion ? normalizeVersion(refRows) : refRows;
	const cand = ignoreVersion ? normalizeVersion(candRows) : candRows;
	const lines: string[] = [];
	let equal = true;
	const n = Math.max(ref.length, cand.length);
	for (let y = 0; y < n; y++) {
		const a = ref[y] ?? "";
		const b = cand[y] ?? "";
		if (a !== b) {
			equal = false;
			lines.push(`  row ${y}:`);
			lines.push(`    native: ${JSON.stringify(a)}`);
			lines.push(`    wasm:   ${JSON.stringify(b)}`);
		}
	}
	if (reference.cursor.x !== candidate.cursor.x || reference.cursor.y !== candidate.cursor.y) {
		equal = false;
		lines.push(
			`  cursor: native (${reference.cursor.x},${reference.cursor.y}) vs wasm (${candidate.cursor.x},${candidate.cursor.y})`,
		);
	}
	return {
		equal,
		lines: equal ? [`${label}: 1:1 match`] : [`${label}: MISMATCH`, ...lines],
	};
}
