#!/usr/bin/env node

import {
	type Agent,
	AgentSideConnection,
	RequestError,
	type AuthenticateRequest,
	type AuthenticateResponse,
	type CancelNotification,
	type InitializeRequest,
	type InitializeResponse,
	type NewSessionRequest,
	type NewSessionResponse,
	type PromptRequest,
	type PromptResponse,
	ndJsonStream,
} from "@agentclientprotocol/sdk";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { randomUUID } from "node:crypto";
import { createInterface } from "node:readline";

type HistoryEntry = { role: "user" | "assistant"; content: string };
type EngineMessage = Record<string, unknown> & { type?: string };

function promptText(params: PromptRequest): string {
	return (params.prompt ?? [])
		.map((part) => (part.type === "text" ? part.text : ""))
		.join("");
}

class CodexSession {
	private readonly history: HistoryEntry[] = [];
	private child: ChildProcessWithoutNullStreams | null = null;
	private cancelled = false;

	constructor(
		private readonly conn: AgentSideConnection,
		readonly sessionId: string,
		private readonly cwd: string,
		private readonly model: string,
	) {}

	async cancel(): Promise<void> {
		this.cancelled = true;
		this.child?.kill("SIGTERM");
	}

	async prompt(params: PromptRequest): Promise<PromptResponse> {
		if (this.child) {
			throw RequestError.invalidRequest(
				{ sessionId: this.sessionId },
				"Codex session already has an active prompt",
			);
		}

		this.cancelled = false;
		const text = promptText(params);
		const env = { ...process.env };
		// Nested WASI processes cannot canonicalize all dynamically mounted guest
		// paths. The VM root is stable and the session-turn engine disables the
		// unsupported shell snapshot feature internally.
		env.CODEX_HOME = "/";

		const command = process.env.CODEX_EXEC_COMMAND ?? "codex-exec";
		const child = spawn(command, ["--session-turn"], {
			cwd: this.cwd,
			env,
			stdio: ["pipe", "pipe", "pipe"],
		});
		this.child = child;

		let stderr = "";
		child.stderr.on("data", (chunk) => {
			stderr += String(chunk);
		});
		const spawnError = new Promise<never>((_, reject) => {
			child.once("error", reject);
			child.stdin.once("error", reject);
		});

		child.stdin.write(
			`${JSON.stringify({
				type: "start",
				cwd: this.cwd,
				model: this.model,
				prompt: text,
				history: this.history,
			})}\n`,
		);

		const lines = createInterface({ input: child.stdout });
		const seenTools = new Set<string>();
		let assistantText = "";
		let terminal: "done" | "error" | null = null;
		let engineError = "";

		try {
			await Promise.race([
				(async () => {
					for await (const line of lines) {
						let message: EngineMessage;
						try {
							message = JSON.parse(line) as EngineMessage;
						} catch {
							continue;
						}

						switch (message.type) {
							case "text_delta": {
								const delta = String(message.delta ?? "");
								assistantText += delta;
								await this.conn.sessionUpdate({
									sessionId: this.sessionId,
									update: {
										sessionUpdate: "agent_message_chunk",
										content: { type: "text", text: delta },
									},
								});
								break;
							}

							case "tool_call_update": {
								const toolCallId = String(message.tool_call_id ?? "");
								const status = String(message.status ?? "in_progress") as
									| "pending"
									| "in_progress"
									| "completed"
									| "failed";
								const first = !seenTools.has(toolCallId);
								seenTools.add(toolCallId);
								await this.conn.sessionUpdate({
									sessionId: this.sessionId,
									update: first
										? {
												sessionUpdate: "tool_call",
												toolCallId,
												kind: "execute",
												status,
												title: "Shell",
											}
										: {
												sessionUpdate: "tool_call_update",
												toolCallId,
												status,
											},
								});
								break;
							}

							case "permission_request": {
								const toolCallId = String(message.tool_call_id ?? "");
								const response = await this.conn.requestPermission({
									sessionId: this.sessionId,
									options: [
										{ optionId: "allow", name: "Allow", kind: "allow_once" },
										{ optionId: "deny", name: "Deny", kind: "reject_once" },
									],
									toolCall: {
										toolCallId,
										kind: "execute",
										status: "pending",
										title: "Shell",
									},
								});
								const allowed =
									response.outcome.outcome === "selected" &&
									response.outcome.optionId === "allow";
								child.stdin.write(
									`${JSON.stringify({ decision: allowed ? "allow" : "deny" })}\n`,
								);
								break;
							}

							case "done":
								terminal = "done";
								return;

							case "error":
								terminal = "error";
								engineError = String(message.message ?? "Codex turn failed");
								return;
						}
					}
				})(),
				spawnError,
			]);

			if (this.cancelled) return { stopReason: "cancelled" };
			if (terminal === "error") {
				throw RequestError.internalError(
					{ stderr: stderr.trim() },
					engineError,
				);
			}
			if (terminal !== "done") {
				throw RequestError.internalError(
					{ stderr: stderr.trim() },
					"codex-exec exited before completing the turn",
				);
			}

			if (text) this.history.push({ role: "user", content: text });
			if (assistantText) {
				this.history.push({ role: "assistant", content: assistantText });
			}
			return { stopReason: "end_turn" };
		} finally {
			lines.close();
			if (terminal === null) child.stdin.end();
			this.child = null;
		}
	}
}

class CodexAgent implements Agent {
	private readonly sessions = new Map<string, CodexSession>();

	constructor(private readonly conn: AgentSideConnection) {
		setTimeout(() => {
			void this.conn.closed.then(() => {
				for (const session of this.sessions.values()) void session.cancel();
				this.sessions.clear();
			});
		}, 0);
	}

	async initialize(_params: InitializeRequest): Promise<InitializeResponse> {
		return {
			protocolVersion: 1,
			agentInfo: {
				name: "codex-acp",
				title: "Codex ACP adapter",
				version: "0.1.0",
			},
			agentCapabilities: {
				promptCapabilities: {
					audio: false,
					embeddedContext: false,
					image: false,
				},
			},
		};
	}

	async newSession(params: NewSessionRequest): Promise<NewSessionResponse> {
		const sessionId = randomUUID();
		this.sessions.set(
			sessionId,
			new CodexSession(
				this.conn,
				sessionId,
				params.cwd,
				process.env.CODEX_MODEL ?? "gpt-5",
			),
		);
		return { sessionId };
	}

	async prompt(params: PromptRequest): Promise<PromptResponse> {
		const session = this.sessions.get(params.sessionId);
		if (!session) {
			throw RequestError.invalidParams(
				{ sessionId: params.sessionId },
				"Unknown Codex session",
			);
		}
		return session.prompt(params);
	}

	async cancel(params: CancelNotification): Promise<void> {
		await this.sessions.get(params.sessionId)?.cancel();
	}

	async authenticate(
		_params: AuthenticateRequest,
	): Promise<AuthenticateResponse | void> {
	}
}

const input = new WritableStream<Uint8Array>({
	write(chunk) {
		return new Promise<void>((resolve) => {
			process.stdout.write(chunk, () => resolve());
		});
	},
});
const output = new ReadableStream<Uint8Array>({
	start(controller) {
		process.stdin.on("data", (chunk: Buffer) => {
			controller.enqueue(new Uint8Array(chunk));
		});
		process.stdin.on("end", () => controller.close());
		process.stdin.on("error", (error: Error) => controller.error(error));
	},
});

const connection = new AgentSideConnection(
	(conn) => new CodexAgent(conn),
	ndJsonStream(input, output),
);

process.stdin.resume();
process.stdin.on("end", () => process.exit(0));
void connection.closed;
