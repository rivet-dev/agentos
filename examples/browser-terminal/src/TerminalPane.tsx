import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef } from "react";

export interface TerminalPaneProps {
	shellId: string;
	active: boolean;
	onInput: (text: string) => void;
	onResize: (cols: number, rows: number) => void;
	subscribe: (onData: (bytes: Uint8Array) => void) => () => void;
}

export function TerminalPane(props: TerminalPaneProps) {
	const { shellId, active, onInput, onResize, subscribe } = props;
	const containerRef = useRef<HTMLDivElement | null>(null);
	const termRef = useRef<Terminal | null>(null);
	const fitRef = useRef<FitAddon | null>(null);

	const onInputRef = useRef(onInput);
	const onResizeRef = useRef(onResize);
	const subscribeRef = useRef(subscribe);
	onInputRef.current = onInput;
	onResizeRef.current = onResize;
	subscribeRef.current = subscribe;

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
		} catch {}

		let line = "";
		const encoder = new TextEncoder();
		const suppress: number[] = [];
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
					line = "";
					onInputRef.current(String.fromCharCode(3));
				} else if (code >= 0x20) {
					line += ch;
					term.write(ch);
				}
			}
		});

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
					if (b === 0x0d) continue;
					suppress.length = 0;
				}
				out.push(b);
			}
			if (out.length > 0) term.write(new Uint8Array(out));
		};

		term.onResize(({ cols, rows }) => onResizeRef.current(cols, rows));

		const unsubscribe = subscribeRef.current(writeOutput);

		return () => {
			unsubscribe();
			term.dispose();
			termRef.current = null;
			fitRef.current = null;
		};
	}, [shellId]);

	useEffect(() => {
		if (!active) return;
		const refit = () => {
			try {
				fitRef.current?.fit();
				termRef.current?.focus();
			} catch {}
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
