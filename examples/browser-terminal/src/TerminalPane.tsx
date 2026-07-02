import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef } from "react";

function base64ToBytes(b64: string): Uint8Array {
	const bin = atob(b64);
	const bytes = new Uint8Array(bin.length);
	for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
	return bytes;
}

/** How often to pull new shell output. */
const POLL_MS = 60;

export interface ReadResult {
	gone?: boolean;
	offset?: number;
	data?: string;
}

export interface TerminalPaneProps {
	shellId: string;
	active: boolean;
	/** Send raw bytes (already `\n`-terminated) to the shell. */
	onInput: (text: string) => void;
	onResize: (cols: number, rows: number) => void;
	/** Pull output emitted after `fromOffset`. */
	readShell: (fromOffset: number) => Promise<ReadResult | undefined>;
	/** Called when the shell no longer exists on the server. */
	onGone: (shellId: string) => void;
}

export function TerminalPane(props: TerminalPaneProps) {
	const { shellId, active, onInput, onResize, readShell, onGone } = props;
	const containerRef = useRef<HTMLDivElement | null>(null);
	const termRef = useRef<Terminal | null>(null);
	const fitRef = useRef<FitAddon | null>(null);

	const onInputRef = useRef(onInput);
	const onResizeRef = useRef(onResize);
	const readShellRef = useRef(readShell);
	const onGoneRef = useRef(onGone);
	onInputRef.current = onInput;
	onResizeRef.current = onResize;
	readShellRef.current = readShell;
	onGoneRef.current = onGone;

	// biome-ignore lint/correctness/useExhaustiveDependencies: one-time xterm setup keyed by shellId.
	useEffect(() => {
		const term = new Terminal({
			convertEol: true,
			cursorBlink: true,
			fontFamily:
				'ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace',
			fontSize: 13,
			theme: { background: "#0b0e14", foreground: "#c7d0e0" },
			scrollback: 5000,
		});
		const fit = new FitAddon();
		term.loadAddon(fit);
		termRef.current = term;
		fitRef.current = fit;
		if (containerRef.current) term.open(containerRef.current);
		try {
			fit.fit();
		} catch {
			// not measurable yet
		}

		// ── Local echo + line editing ─────────────────────────────────────
		// The VM shell is line-buffered and only echoes a line once you press
		// Enter, so keystrokes wouldn't otherwise appear as you type. We echo
		// input locally and send whole lines to the shell, then suppress the
		// shell's own echo of that line (below) so it isn't shown twice.
		let line = "";
		const encoder = new TextEncoder();
		const suppress: number[] = []; // bytes of shell-echo still to swallow
		term.onData((data) => {
			for (const ch of data) {
				const code = ch.codePointAt(0) ?? 0;
				if (ch === "\r" || ch === "\n") {
					term.write("\r\n");
					const cmd = line;
					line = "";
					for (const b of encoder.encode(`${cmd}\n`)) suppress.push(b);
					onInputRef.current(`${cmd}\n`);
				} else if (code === 0x7f || ch === "\b") {
					if (line.length > 0) {
						line = line.slice(0, -1);
						term.write("\b \b");
					}
				} else if (code === 0x03) {
					// Ctrl-C: abandon the line, let the shell reset to a new prompt.
					line = "";
					onInputRef.current(String.fromCharCode(3));
				} else if (code >= 0x20) {
					line += ch;
					term.write(ch);
				}
				// Other control sequences (arrows, tab, …) are intentionally ignored.
			}
		});

		// Write shell output, swallowing the shell's echo of what we just sent.
		const writeOutput = (bytes: Uint8Array) => {
			if (suppress.length === 0) {
				term.write(bytes);
				return;
			}
			const out: number[] = [];
			for (let i = 0; i < bytes.length; i++) {
				const b = bytes[i];
				if (suppress.length > 0) {
					if (b === suppress[0]) {
						suppress.shift();
						continue;
					}
					if (b === 0x0d) continue; // CR from the shell's CRLF echo of our LF
					suppress.length = 0; // echo diverged — stop swallowing
				}
				out.push(b);
			}
			if (out.length > 0) term.write(new Uint8Array(out));
		};

		term.onResize(({ cols, rows }) => onResizeRef.current(cols, rows));

		// Poll for output from offset 0 (replays scrollback on attach, then live).
		let disposed = false;
		let offset = 0;
		let timer: ReturnType<typeof setTimeout> | undefined;
		const tick = async () => {
			try {
				const res = await readShellRef.current(offset);
				if (disposed) return;
				if (res?.gone) {
					onGoneRef.current(shellId);
					return;
				}
				if (res?.data) writeOutput(base64ToBytes(res.data));
				if (typeof res?.offset === "number") offset = res.offset;
			} catch {
				// transient; keep polling
			}
			if (!disposed) timer = setTimeout(tick, POLL_MS);
		};
		timer = setTimeout(tick, 0);

		return () => {
			disposed = true;
			if (timer) clearTimeout(timer);
			term.dispose();
			termRef.current = null;
			fitRef.current = null;
		};
	}, [shellId]);

	// Refit + focus whenever this pane becomes active or the window resizes.
	useEffect(() => {
		if (!active) return;
		const refit = () => {
			try {
				fitRef.current?.fit();
				termRef.current?.focus();
			} catch {
				// ignore
			}
		};
		refit();
		window.addEventListener("resize", refit);
		return () => window.removeEventListener("resize", refit);
	}, [active]);

	return (
		<div
			className="terminal-pane"
			style={{ display: active ? "block" : "none" }}
			ref={containerRef}
		/>
	);
}
