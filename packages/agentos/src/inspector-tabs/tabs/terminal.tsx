import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef, useState } from "react";
import { AgentOsEmpty } from "../common";
import { useAgentOsActor } from "../lib/rivet";
import type { ShellDataPayload } from "../lib/types";
import React from "react";

// The `shellData`/`shellStderr` payloads carry `data` as a Uint8Array, but the
// JSON wire encoding may hand it back as `["$Uint8Array", base64]`.
function toBytes(data: unknown): Uint8Array {
	if (data instanceof Uint8Array) return data;
	if (Array.isArray(data) && data[0] === "$Uint8Array" && typeof data[1] === "string") {
		const bin = atob(data[1]);
		const bytes = new Uint8Array(bin.length);
		for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
		return bytes;
	}
	if (Array.isArray(data)) return Uint8Array.from(data as number[]);
	return new Uint8Array();
}

// xterm pane with local line-editing (echo + backspace + Ctrl-C), mirroring the
// browser-terminal example. `onInput` sends a completed line to the shell;
// `subscribe` streams shell output; a `suppress` buffer swallows the shell's
// echo of our own just-sent input so it isn't printed twice.
function TerminalPane({
	shellId,
	onInput,
	onResize,
	subscribe,
}: {
	shellId: string;
	onInput: (text: string) => void;
	onResize: (cols: number, rows: number) => void;
	subscribe: (onData: (bytes: Uint8Array) => void) => () => void;
}) {
	const containerRef = useRef<HTMLDivElement | null>(null);
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
		if (containerRef.current) term.open(containerRef.current);
		try {
			fit.fit();
		} catch {}
		term.focus();

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
		onResizeRef.current(term.cols, term.rows);
		const unsubscribe = subscribeRef.current(writeOutput);

		const refit = () => {
			try {
				fit.fit();
			} catch {}
		};
		window.addEventListener("resize", refit);
		return () => {
			window.removeEventListener("resize", refit);
			unsubscribe();
			term.dispose();
		};
	}, [shellId]);

	return <div className="h-full w-full" ref={containerRef} />;
}

export function TerminalTabConnected(_props: { actorId: string }) {
	const actor = useAgentOsActor();
	// biome-ignore lint/suspicious/noExplicitAny: untyped actor connection (shell actions + events)
	const conn = actor.connection as any;
	const [shellId, setShellId] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);

	// Open a shell once the connection is live; close it on unmount.
	useEffect(() => {
		if (!conn || shellId) return;
		let cancelled = false;
		conn
			// Default `sh` launches brush interactively, which requests the
			// `reedline` input backend; the shipped wasm build omits it and the
			// shell dies with "requested input backend type not supported". This
			// pane does its own local line-editing (echo/backspace/Ctrl-C, and it
			// only sends completed lines), so the `minimal` backend is both the fix
			// and the correct match — reedline would double-handle input.
			.openShell({
				command: "sh",
				args: ["-i", "--input-backend", "minimal"],
				cols: 80,
				rows: 24,
			})
			.then((r: { shellId: string }) => {
				if (!cancelled) setShellId(r.shellId);
			})
			.catch((e: unknown) => {
				if (!cancelled) setError(String((e as Error)?.message ?? e));
			});
		return () => {
			cancelled = true;
		};
	}, [conn, shellId]);

	useEffect(() => {
		return () => {
			if (shellId && conn) conn.closeShell(shellId).catch(() => {});
		};
	}, [shellId, conn]);

	if (error) {
		return <AgentOsEmpty>Terminal failed to start: {error}</AgentOsEmpty>;
	}
	if (!conn || !shellId) {
		return <AgentOsEmpty>Starting terminal…</AgentOsEmpty>;
	}

	return (
		<div className="h-full min-h-0 bg-[#0b0e14] p-2">
			<TerminalPane
				shellId={shellId}
				onInput={(text) => conn.writeShell(shellId, text)}
				onResize={(cols, rows) => conn.resizeShell(shellId, cols, rows)}
				subscribe={(onData) => {
					const off1 = conn.on("shellData", (p: ShellDataPayload) => {
						if (p?.shellId === shellId) onData(toBytes(p.data));
					});
					const off2 = conn.on("shellStderr", (p: ShellDataPayload) => {
						if (p?.shellId === shellId) onData(toBytes(p.data));
					});
					return () => {
						off1?.();
						off2?.();
					};
				}}
			/>
		</div>
	);
}
