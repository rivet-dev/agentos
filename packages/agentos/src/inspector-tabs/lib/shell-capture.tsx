// Background shell recorder, mounted in every inspector tab document EXCEPT
// the terminal (which owns the store while it is on screen). The dashboard
// swaps the iframe per tab, so while the user is on Filesystem or Transcript
// this component keeps each live shell's state current in a headless terminal
// mirror (lib/shell-mirror) — answering the shell's terminal queries so its
// line editor does not fall back to destructive clear-screen repaints — and
// persists serialized snapshots on pagehide. Switching back to the terminal
// then replays a scrollback with no gap and no corruption.
import { useEffect, useRef } from "react";
import { useAgentOsActor } from "./rivet";
import type { ShellMirror } from "./shell-mirror";
import {
	loadPersistedShells,
	type PersistedShells,
	savePersistedShells,
	trimScrollback,
} from "./shell-store";
import { agentOsSource, decodeActionBytes } from "./source";
import type { ShellDataPayload, ShellExitPayload } from "./types";

interface CaptureStore {
	payload: PersistedShells;
	/** One headless mirror per live shell; exited/dead shells keep their final
	 * snapshot in the payload record instead. */
	mirrors: Map<string, ShellMirror>;
	/** Streaming decoders per shell per stream — chunk boundaries can split
	 * multibyte UTF-8, and stdout/stderr interleave. */
	decoders: Map<string, { out: TextDecoder; err: TextDecoder }>;
}

export function ShellOutputCapture({ actorId }: { actorId: string }) {
	const storeRef = useRef<CaptureStore | null>(null);
	// The headless-terminal module loads lazily (it must stay out of the shared
	// main bundle); events arriving before it is ready are queued, bounded.
	const pendingRef = useRef<{ shellId: string; stream: "out" | "err"; data: Uint8Array }[]>([]);
	const readyRef = useRef(false);
	// Query replies are separate HTTP requests; serialize them so two in flight
	// cannot reorder the shell's input bytes. The chain never rejects.
	const writeChainRef = useRef<Promise<void>>(Promise.resolve());

	const reply = (shellId: string, data: string) => {
		writeChainRef.current = writeChainRef.current.then(() =>
			agentOsSource.writeShell(shellId, data).then(
				() => undefined,
				(error) => {
					console.warn(`agentos inspector: capture reply to ${shellId} failed`, error);
				},
			),
		);
	};

	const decodersFor = (s: CaptureStore, shellId: string) => {
		let d = s.decoders.get(shellId);
		if (!d) {
			d = { out: new TextDecoder(), err: new TextDecoder() };
			s.decoders.set(shellId, d);
		}
		return d;
	};

	const feed = (shellId: string, stream: "out" | "err", data: Uint8Array) => {
		const s = storeRef.current;
		if (!s) return;
		const mirror = s.mirrors.get(shellId);
		if (!mirror) return;
		mirror.write(decodersFor(s, shellId)[stream].decode(data, { stream: true }));
	};

	// Boot the store once: load the persisted payload, then the mirror module,
	// then a seeded mirror per live shell. All later events feed the mirrors.
	useEffect(() => {
		const payload = loadPersistedShells(actorId);
		if (!payload || !payload.shells.some((s) => (s.status ?? "live") === "live")) return;
		let disposed = false;
		void import("./shell-mirror").then(({ ShellMirror: Mirror }) => {
			if (disposed) return;
			const mirrors = new Map<string, ShellMirror>();
			for (const record of payload.shells) {
				if ((record.status ?? "live") !== "live") continue;
				const mirror = new Mirror({
					cols: payload.cols ?? 80,
					rows: payload.rows ?? 24,
					onReply: (data) => reply(record.shellId, data),
				});
				// Answer queries once the seed replay is done: this document is
				// the only terminal the shell has while the tab is hidden.
				mirror.seed(record.scrollback, true);
				mirrors.set(record.shellId, mirror);
			}
			storeRef.current = { payload, mirrors, decoders: new Map() };
			readyRef.current = true;
			for (const event of pendingRef.current) feed(event.shellId, event.stream, event.data);
			pendingRef.current = [];
		});
		return () => {
			disposed = true;
			const s = storeRef.current;
			if (s) {
				for (const mirror of s.mirrors.values()) mirror.dispose();
				s.mirrors.clear();
			}
			storeRef.current = null;
			readyRef.current = false;
		};
	}, [actorId]);

	const actor = useAgentOsActor();
	const useAgentEvent = actor.useEvent as (
		name: string,
		handler: (payload: unknown) => void,
	) => void;

	const onShellOutput = (raw: unknown, stream: "out" | "err") => {
		const payload = raw as ShellDataPayload | undefined;
		if (!payload?.shellId) return;
		const data = decodeActionBytes(payload.data);
		if (!readyRef.current) {
			if (pendingRef.current.length < 1024) pendingRef.current.push({ shellId: payload.shellId, stream, data });
			return;
		}
		feed(payload.shellId, stream, data);
	};
	useAgentEvent("shellData", (raw) => onShellOutput(raw, "out"));
	useAgentEvent("shellStderr", (raw) => onShellOutput(raw, "err"));

	// A shell that ends while hidden: freeze its final snapshot (plus the note
	// the terminal would have shown) into the record and save immediately.
	const finish = (shellId: string, status: "exited" | "dead", note: string, exitCode?: number) => {
		const s = storeRef.current;
		const record = s?.payload.shells.find((sh) => sh.shellId === shellId);
		if (!s || !record || (record.status ?? "live") !== "live") return;
		const mirror = s.mirrors.get(shellId);
		record.status = status;
		record.exitCode = exitCode;
		record.scrollback = trimScrollback((mirror ? mirror.serialize() : record.scrollback) + note);
		if (mirror) {
			mirror.dispose();
			s.mirrors.delete(shellId);
		}
		savePersistedShells(actorId, s.payload);
	};
	useAgentEvent("shellExit", (raw) => {
		const payload = raw as ShellExitPayload | undefined;
		if (!payload?.shellId) return;
		finish(
			payload.shellId,
			"exited",
			`\r\n\x1b[2m[shell exited with code ${payload.exitCode}]\x1b[0m\r\n`,
			payload.exitCode,
		);
	});
	useAgentEvent("vmShutdown", (raw) => {
		const s = storeRef.current;
		if (!s) return;
		const reason = (raw as { reason?: string } | undefined)?.reason;
		const note = `\r\n\x1b[2m[VM shut down${reason ? ` (${reason})` : ""} — shell terminated]\x1b[0m\r\n`;
		for (const record of [...s.payload.shells]) {
			if ((record.status ?? "live") === "live") finish(record.shellId, "dead", note);
		}
	});

	// Persist on every pagehide — even with no new output, the fresh savedAt
	// attests that a capture document was alive (the terminal's reattach uses
	// that to decide whether a real gap note is warranted).
	useEffect(() => {
		const flush = () => {
			const s = storeRef.current;
			if (!s) return;
			for (const record of s.payload.shells) {
				const mirror = s.mirrors.get(record.shellId);
				if (mirror) record.scrollback = trimScrollback(mirror.serialize());
			}
			savePersistedShells(actorId, s.payload);
		};
		window.addEventListener("pagehide", flush);
		return () => {
			window.removeEventListener("pagehide", flush);
			flush();
		};
	}, [actorId]);

	return null;
}
