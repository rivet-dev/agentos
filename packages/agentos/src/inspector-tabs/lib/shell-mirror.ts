// Headless terminal mirror for one shell — the piece that makes hidden shells
// behave. The VM shell's line editor (reedline) queries the terminal (cursor
// position reports, "ESC[6n") and, when no answer arrives, falls back to
// repainting its prompt with a home+clear-screen sequence — wiping everything
// on the next replay. A visible xterm answers those queries automatically; a
// mirror answers them for shells nobody is looking at (background shells in
// the terminal tab, and every shell while another inspector tab is open), and
// its serialized buffer is the clean scrollback snapshot that gets persisted.
//
// This module is imported lazily (dynamic import) so the headless terminal
// stays out of the shared main bundle, mirroring the xterm.js confinement of
// the terminal chunk.
import { SerializeAddon } from "@xterm/addon-serialize";
import { Terminal as HeadlessTerminal } from "@xterm/headless";

export class ShellMirror {
	private readonly term: HeadlessTerminal;
	private readonly serializer: SerializeAddon;
	/** Gate query answers: off while seeding persisted scrollback (answers to
	 * long-gone queries must not reach the live shell) and for the shell the
	 * visible xterm is rendering (it answers; double replies corrupt input). */
	private answering = false;
	/** True once the seed snapshot has been processed — `serialize()` before
	 * that returns an empty screen, so callers fall back to the raw text. */
	private ready = false;

	constructor(opts: {
		cols: number;
		rows: number;
		/** Forward terminal query replies (DSR/CPR, DA, …) to the shell. */
		onReply: (data: string) => void;
	}) {
		this.term = new HeadlessTerminal({
			cols: opts.cols,
			rows: opts.rows,
			scrollback: 1000,
			allowProposedApi: true,
		});
		this.serializer = new SerializeAddon();
		this.term.loadAddon(this.serializer);
		this.term.onData((data) => {
			if (this.answering) opts.onReply(data);
		});
	}

	/** Seed persisted scrollback; enables query answering once processed. */
	seed(snapshot: string, answerQueries: boolean): void {
		if (!snapshot) {
			this.ready = true;
			this.answering = answerQueries;
			return;
		}
		this.term.write(snapshot, () => {
			this.ready = true;
			this.answering = answerQueries;
		});
	}

	isReady(): boolean {
		return this.ready;
	}

	setAnswering(on: boolean): void {
		this.answering = on;
	}

	write(text: string): void {
		this.term.write(text);
	}

	resize(cols: number, rows: number): void {
		if (cols > 0 && rows > 0 && (this.term.cols !== cols || this.term.rows !== rows)) {
			this.term.resize(cols, rows);
		}
	}

	/** ANSI-complete snapshot of scrollback + screen, safe to replay: it
	 * reconstructs state and contains no queries or destructive clears. */
	serialize(): string {
		return this.serializer.serialize();
	}

	dispose(): void {
		this.term.dispose();
	}
}
