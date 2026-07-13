import { useCallback, useEffect, useRef, useState } from "react";
import { ACTOR_NAME, useActor } from "./rivet";
import { loadShellIds, saveShellIds } from "./store";
import { TerminalPane } from "./TerminalPane";

interface Tab {
	shellId: string;
	title: string;
}

interface ShellDataPayload {
	shellId: string;
	data: unknown;
}
interface ShellExitPayload {
	shellId: string;
}
interface ProcessOutputPayload {
	pid: number;
	stream: "stdout" | "stderr";
	data: unknown;
}
interface ActorProcessInfo {
	pid: number;
	command: string;
	args: string[];
	running: boolean;
}

const ACTOR_PI_ENV = {
	HOME: "/home/agentos",
	PI_CODING_AGENT_DIR: "/home/agentos/.pi/agent",
};
const ACTOR_PI_AGENT_DIR = ACTOR_PI_ENV.PI_CODING_AGENT_DIR;
const ACTOR_PI_MODEL_SERVER = "/home/agentos/actor-demo-model.mjs";
const ACTOR_PI_PROVIDER = "actor-demo";
const ACTOR_PI_MODEL = "deterministic";
const ACTOR_PI_MODEL_SERVER_SOURCE = `
import { createServer } from "node:http";
const MAX_REQUEST_BYTES = 1024 * 1024;
function event(name, data) { return "event: " + name + "\\ndata: " + JSON.stringify(data) + "\\n\\n"; }
function reply(prompt) {
  const exact = prompt.match(/reply exactly\\s+([A-Z0-9_]+)/i)?.[1];
  return "[MOCK actor model — not real] " + (exact ?? ("Received: " + prompt.trim().slice(0, 240)));
}
function sse(text) {
  return [
    event("message_start", { type: "message_start", message: { id: "msg_actor_demo", type: "message", role: "assistant", model: "actor-demo", content: [], stop_reason: null, stop_sequence: null, usage: { input_tokens: 1, output_tokens: 1 } } }),
    event("content_block_start", { type: "content_block_start", index: 0, content_block: { type: "text", text: "" } }),
    event("content_block_delta", { type: "content_block_delta", index: 0, delta: { type: "text_delta", text } }),
    event("content_block_stop", { type: "content_block_stop", index: 0 }),
    event("message_delta", { type: "message_delta", delta: { stop_reason: "end_turn", stop_sequence: null }, usage: { output_tokens: 1 } }),
    event("message_stop", { type: "message_stop" }),
  ].join("");
}
const server = createServer((request, response) => {
  if (request.method !== "POST" || request.url !== "/v1/messages") {
    response.writeHead(404).end("not found");
    return;
  }
  let size = 0;
  const chunks = [];
  request.on("data", (chunk) => {
    size += chunk.length;
    if (size > MAX_REQUEST_BYTES) {
      response.writeHead(413).end("request too large");
      request.destroy();
      return;
    }
    chunks.push(chunk);
  });
  request.on("end", () => {
    if (response.writableEnded) return;
    try {
      const body = JSON.parse(Buffer.concat(chunks).toString("utf8"));
      const message = [...(body.messages ?? [])].reverse().find((entry) => entry.role === "user");
      const prompt = typeof message?.content === "string"
        ? message.content
        : (message?.content ?? []).map((part) => part?.text ?? "").join("");
      response.writeHead(200, { "Content-Type": "text/event-stream; charset=utf-8", "Cache-Control": "no-store" });
      response.end(sse(reply(prompt)));
    } catch (error) {
      console.error("actor demo model rejected malformed input", error);
      response.writeHead(400).end("malformed request");
    }
  });
  request.on("error", (error) => console.error("actor demo model request failed", error));
});
await new Promise((resolve, reject) => {
  server.once("error", reject);
  server.listen(6431, "127.0.0.1", resolve);
});
console.error("actor demo model ready on 6431");
await new Promise(() => {});
`;
const ACTOR_PI_MODELS = JSON.stringify({
	providers: {
		[ACTOR_PI_PROVIDER]: {
			baseUrl: "http://127.0.0.1:6431",
			api: "anthropic-messages",
			apiKey: "sk-agentos-actor-demo",
			models: [
				{
					id: ACTOR_PI_MODEL,
					name: "Actor demo model (mock)",
					reasoning: false,
					input: ["text"],
					contextWindow: 8192,
					maxTokens: 1024,
				},
			],
		},
	},
});
const ACTOR_PI_SETTINGS = JSON.stringify({
	defaultProvider: ACTOR_PI_PROVIDER,
	defaultModel: ACTOR_PI_MODEL,
	enabledModels: [`${ACTOR_PI_PROVIDER}/${ACTOR_PI_MODEL}`],
});

function toBytes(data: unknown): Uint8Array {
	if (data instanceof Uint8Array) return data;
	if (data instanceof ArrayBuffer) return new Uint8Array(data);
	if (Array.isArray(data)) return new Uint8Array(data as number[]);
	if (typeof data === "string") {
		const bin = atob(data);
		const bytes = new Uint8Array(bin.length);
		for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
		return bytes;
	}
	return new Uint8Array();
}

export function ActorView({ actorId }: { actorId: string }) {
	const agent = useActor({ name: ACTOR_NAME, key: actorId });
	const conn = agent.connection;

	const [tabs, setTabs] = useState<Tab[]>([]);
	const [active, setActive] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const initedRef = useRef(false);
	const modelServerPidRef = useRef<number | null>(null);

	const writers = useRef<Map<string, (bytes: Uint8Array) => void>>(new Map());
	const pending = useRef<Map<string, Uint8Array[]>>(new Map());

	const dispatchData = useCallback((shellId: string, bytes: Uint8Array) => {
		const writer = writers.current.get(shellId);
		if (writer) {
			writer(bytes);
			return;
		}
		const buf = pending.current.get(shellId) ?? [];
		buf.push(bytes);
		pending.current.set(shellId, buf);
	}, []);

	const dropTab = useCallback(
		(shellId: string) => {
			writers.current.delete(shellId);
			pending.current.delete(shellId);
			setTabs((prev) => {
				const next = prev.filter((t) => t.shellId !== shellId);
				saveShellIds(
					actorId,
					next.map((t) => t.shellId),
				);
				setActive((cur) =>
					cur === shellId ? (next[next.length - 1]?.shellId ?? null) : cur,
				);
				return next;
			});
		},
		[actorId],
	);

	const subscribe = useCallback(
		(shellId: string) => (onData: (bytes: Uint8Array) => void) => {
			writers.current.set(shellId, onData);
			const buf = pending.current.get(shellId);
			if (buf) {
				for (const b of buf) onData(b);
				pending.current.delete(shellId);
			}
			return () => {
				writers.current.delete(shellId);
			};
		},
		[],
	);

	useEffect(() => {
		if (!conn) return;
		const events = conn as unknown as {
			on(name: string, cb: (p: never) => void): () => void;
		};
		const offData = events.on("shellData", (p: ShellDataPayload) =>
			dispatchData(p.shellId, toBytes(p.data)),
		);
		const offExit = events.on("shellExit", (p: ShellExitPayload) =>
			dropTab(p.shellId),
		);
		const offProcessOutput = events.on(
			"processOutput",
			(payload: ProcessOutputPayload) => {
				if (payload.stream !== "stderr") return;
				const message = new TextDecoder().decode(toBytes(payload.data)).trim();
				if (!message) return;
				if (message.includes("actor demo model ready")) {
					console.info(`actor process ${payload.pid}: ${message}`);
					return;
				}
				console.error(`actor process ${payload.pid}: ${message}`);
				setError(`Actor Pi demo model: ${message}`);
			},
		);
		return () => {
			offData();
			offExit();
			offProcessOutput();
		};
	}, [conn, dispatchData, dropTab]);

	useEffect(() => {
		if (!conn || initedRef.current) return;
		initedRef.current = true;
		const ids = loadShellIds(actorId);
		if (ids.length === 0) return;
		Promise.all(
			ids.map(async (shellId) => {
				try {
					await conn.resizeShell(shellId, 80, 24);
					return shellId;
				} catch {
					return null;
				}
			}),
		)
			.then((probed) => {
				const live = probed.filter((id): id is string => id !== null);
				saveShellIds(actorId, live);
				if (live.length === 0) return;
				setTabs(live.map((id, i) => ({ shellId: id, title: `shell ${i + 1}` })));
				setActive(live[0]);
			})
			.catch((e: unknown) => setError(String(e)));
	}, [conn, actorId]);

	useEffect(() => {
		initedRef.current = false;
		writers.current.clear();
		pending.current.clear();
		setTabs([]);
		setActive(null);
		setError(null);
		modelServerPidRef.current = null;
	}, [actorId]);

	const openShell = useCallback(async (command?: string) => {
		if (!conn) return;
		setBusy(true);
		setError(null);
		try {
			if (command === "pi") {
				for (const directory of ["/home/agentos/.pi", ACTOR_PI_AGENT_DIR]) {
					if (!(await conn.exists(directory))) await conn.mkdir(directory);
				}
				await conn.writeFiles([
					{ path: `${ACTOR_PI_AGENT_DIR}/models.json`, content: ACTOR_PI_MODELS },
					{
						path: `${ACTOR_PI_AGENT_DIR}/settings.json`,
						content: ACTOR_PI_SETTINGS,
					},
					{
						path: ACTOR_PI_MODEL_SERVER,
						content: ACTOR_PI_MODEL_SERVER_SOURCE,
					},
				]);
				if (modelServerPidRef.current === null) {
					const runningServer = (await conn.listProcesses()).find(
						(process: ActorProcessInfo) =>
							process.running &&
							process.command === "node" &&
							process.args.includes(ACTOR_PI_MODEL_SERVER),
					);
					if (runningServer) {
						modelServerPidRef.current = runningServer.pid;
					} else {
						const { pid } = await conn.spawn("node", [ACTOR_PI_MODEL_SERVER]);
						modelServerPidRef.current = pid;
						await new Promise((resolve) => setTimeout(resolve, 250));
					}
				}
			}
			const { shellId } = await conn.openShell({
				...(command ? { command } : {}),
				...(!command
					? { args: ["--input-backend", "minimal", "-i"] }
					: {}),
				...(command === "pi" ? { env: ACTOR_PI_ENV } : {}),
				cols: 80,
				rows: 24,
			});
			setTabs((prev) => {
				const next = [
					...prev,
					{
						shellId,
						title: command === "pi" ? "pi" : `shell ${prev.length + 1}`,
					},
				];
				saveShellIds(
					actorId,
					next.map((t) => t.shellId),
				);
				return next;
			});
			setActive(shellId);
		} catch (e) {
			setError(String(e));
		} finally {
			setBusy(false);
		}
	}, [conn, actorId]);

	const closeTab = useCallback(
		async (shellId: string) => {
			dropTab(shellId);
			try {
				await conn?.closeShell(shellId);
			} catch (cause) {
				setError(`Failed to close terminal: ${String(cause)}`);
			}
		},
		[conn, dropTab],
	);

	return (
		<div className="actor-view">
			<div className="mode-banner">
				<span className="mode-badge actor-badge">ACTOR API</span>
				<span>
					Runtime and PTYs execute behind the RivetKit actor. Pi uses the
					 labeled deterministic demo model.
				</span>
			</div>
			<div className="tabbar">
				{tabs.map((t) => (
					<div
						key={t.shellId}
						className={`tab ${t.shellId === active ? "tab-active" : ""}`}
						onClick={() => setActive(t.shellId)}
					>
						<span className="tab-title">{t.title}</span>
						<button
							type="button"
							className="tab-close"
							title="Close terminal"
							onClick={(e) => {
								e.stopPropagation();
								void closeTab(t.shellId);
							}}
						>
							×
						</button>
					</div>
				))}
				<button
					type="button"
					className="tab-new"
					disabled={!conn || busy}
					onClick={() => void openShell()}
					title="New shell terminal"
				>
					+ shell
				</button>
				<button
					type="button"
					className="tab-new"
					disabled={!conn || busy}
					onClick={() => void openShell("pi")}
					title="New Pi terminal"
				>
					+ pi
				</button>
				<span className="conn-status">
					{conn ? "connected" : "connecting…"}
				</span>
			</div>

			{error && <div className="error-banner">{error}</div>}

			<div className="terminals">
				{tabs.length === 0 && (
					<div className="empty-hint">
						{conn
							? "No terminals yet — click + to open one."
							: "Connecting to the VM…"}
					</div>
				)}
				{tabs.map((t) => (
					<TerminalPane
						key={t.shellId}
						shellId={t.shellId}
						active={t.shellId === active}
						onInput={(text) => conn?.writeShell(t.shellId, text)}
						onError={(cause) => setError(String(cause))}
						onResize={(cols, rows) => conn?.resizeShell(t.shellId, cols, rows)}
						subscribe={subscribe(t.shellId)}
					/>
				))}
			</div>
		</div>
	);
}
