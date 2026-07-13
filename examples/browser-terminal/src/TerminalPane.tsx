import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef } from "react";

declare global {
	interface Window {
		__agentOSTerminalDemo?: {
			screens(): Record<string, string>;
			write(shellId: string, text: string): Promise<void>;
		};
	}
}

const terminalScreens = new Map<string, () => string>();
const terminalInputs = new Map<
	string,
	(text: string) => void | Promise<void>
>();

window.__agentOSTerminalDemo ??= {
	screens: () =>
		Object.fromEntries(
			[...terminalScreens].map(([shellId, read]) => [shellId, read()]),
		),
	write: async (shellId, text) => {
		const input = terminalInputs.get(shellId);
		if (!input) throw new Error(`terminal ${shellId} is not mounted`);
		await input(text);
	},
};

function readTerminal(term: Terminal): string {
	const buffer = term.buffer.active;
	const lines: string[] = [];
	for (let index = 0; index < buffer.length; index += 1) {
		lines.push(buffer.getLine(index)?.translateToString(true) ?? "");
	}
	return lines.join("\n");
}

export interface TerminalPaneProps {
	shellId: string;
	active: boolean;
	onInput: (text: string) => void | Promise<void>;
	onError: (error: unknown) => void;
	onResize: (cols: number, rows: number) => void;
	subscribe: (onData: (bytes: Uint8Array) => void) => () => void;
}

export function TerminalPane(props: TerminalPaneProps) {
	const { shellId, active, onInput, onError, onResize, subscribe } = props;
	const containerRef = useRef<HTMLDivElement | null>(null);
	const termRef = useRef<Terminal | null>(null);
	const fitRef = useRef<FitAddon | null>(null);

	const onInputRef = useRef(onInput);
	const onErrorRef = useRef(onError);
	const onResizeRef = useRef(onResize);
	const subscribeRef = useRef(subscribe);
	onInputRef.current = onInput;
	onErrorRef.current = onError;
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
		terminalScreens.set(shellId, () => readTerminal(term));
		let inputQueue = Promise.resolve();
		const enqueueInput = (text: string): Promise<void> => {
			const request = inputQueue.then(() => onInputRef.current(text));
			inputQueue = request.catch((error: unknown) => {
				onErrorRef.current(error);
			});
			return request;
		};
		// Test/demo automation must use the same queue as real xterm input. In
		// particular, a command cannot overtake xterm's DSR cursor-position reply
		// while that reply is crossing the Actor API boundary.
		terminalInputs.set(shellId, enqueueInput);
		if (containerRef.current) term.open(containerRef.current);
		// Register before the initial fit: FitAddon.resize() emits synchronously,
		// and missing that first event leaves the VM at xterm's 80x24 default even
		// when the rendered pane is much larger. Full-screen programs would then
		// address row 24 inside a taller xterm and the next prompt could overwrite
		// their output instead of scrolling naturally.
		term.onResize(({ cols, rows }) => onResizeRef.current(cols, rows));
		try {
			fit.fit();
		} catch (error) {
			onErrorRef.current(error);
		}

		term.onData((data) => {
			void enqueueInput(data).catch(() => {
				// enqueueInput already reports the error and recovers the queue.
			});
		});

		const unsubscribe = subscribeRef.current((bytes) => {
			term.write(bytes);
		});

		return () => {
			unsubscribe();
			term.dispose();
			terminalScreens.delete(shellId);
			terminalInputs.delete(shellId);
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
			} catch (error) {
				onErrorRef.current(error);
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
