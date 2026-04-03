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
	ndJsonStream,
	type PromptRequest,
	type PromptResponse,
	type SessionNotification,
	type SetSessionConfigOptionRequest,
	type SetSessionConfigOptionResponse,
	type SetSessionModeRequest,
	type SetSessionModeResponse,
} from "@agentclientprotocol/sdk";
import { spawn, type ChildProcess } from "node:child_process";
import { randomUUID } from "node:crypto";
import {
	chmodSync,
	closeSync,
	createReadStream,
	existsSync,
	mkdirSync,
	openSync,
	readFileSync,
	statSync,
	writeSync,
} from "node:fs";
import {
	basename,
	isAbsolute,
	join,
	resolve as resolvePath,
} from "node:path";

type JsonRecord = Record<string, unknown>;
type ThinkingLevel = "off" | "minimal" | "low" | "medium" | "high" | "xhigh";

type RpcModel = {
	id: string;
	name?: string;
	provider: string;
	reasoning: boolean;
};

type RpcResponse = {
	type: "response";
	id?: string;
	command: string;
	success: boolean;
	data?: JsonRecord | null;
	error?: string;
	errorHints?: unknown;
};

type PendingRequest = {
	command: string;
	resolve: (value: JsonRecord | undefined) => void;
	reject: (reason?: unknown) => void;
};

type PromptExecution = {
	cancelRequested: boolean;
	resolve: (value: PromptResponse) => void;
	reject: (reason?: unknown) => void;
	promise: Promise<PromptResponse>;
};

type PiLiteSessionState = {
	sessionId: string;
	cwd: string;
	rpc: RpcProcess;
	currentModel: RpcModel | null;
	availableModels: RpcModel[];
	thinkingLevel: ThinkingLevel;
	activePrompt: PromptExecution | null;
	currentToolCalls: Map<string, string>;
	editSnapshots: Map<string, { path: string; oldText: string }>;
	lastEmit: Promise<void>;
};

let appendSystemPrompt: string | undefined;
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i++) {
	if (argv[i] === "--append-system-prompt" && i + 1 < argv.length) {
		appendSystemPrompt = argv[i + 1];
		i++;
	}
}

function thinkingModesFor(model: RpcModel | null): ThinkingLevel[] {
	if (!model?.reasoning) return ["off"];
	const levels: ThinkingLevel[] = [
		"off",
		"minimal",
		"low",
		"medium",
		"high",
	];
	if (
		new Set([
			"gpt-5.1-codex-max",
			"gpt-5.2",
			"gpt-5.4",
			"gpt-5.2-codex",
			"gpt-5.3-codex",
			"gpt-5.3-codex-spark",
		]).has(model.id)
	) {
		levels.push("xhigh");
	}
	return levels;
}

function formatThinkingLabel(level: ThinkingLevel): string {
	switch (level) {
		case "off":
			return "Off";
		case "minimal":
			return "Minimal";
		case "low":
			return "Low";
		case "medium":
			return "Medium";
		case "high":
			return "High";
		case "xhigh":
			return "XHigh";
	}
}

function modelOptionValue(model: RpcModel): string {
	return `${model.provider}/${model.id}`;
}

function parseModelOptionValue(value: string): { provider: string; modelId: string } {
	const slash = value.indexOf("/");
	if (slash <= 0 || slash === value.length - 1) {
		throw RequestError.invalidParams({ value }, "model value must be provider/model");
	}
	return {
		provider: value.slice(0, slash),
		modelId: value.slice(slash + 1),
	};
}

function parseThinkingLevel(value: string): ThinkingLevel {
	const normalized = value.trim().toLowerCase();
	if (
		normalized === "off" ||
		normalized === "minimal" ||
		normalized === "low" ||
		normalized === "medium" ||
		normalized === "high" ||
		normalized === "xhigh"
	) {
		return normalized;
	}
	throw RequestError.invalidParams({ value }, "unsupported thinking level");
}

function parseRpcModel(value: unknown): RpcModel | null {
	if (!value || typeof value !== "object") return null;
	const record = value as JsonRecord;
	const id = typeof record.id === "string" ? record.id : null;
	const provider = typeof record.provider === "string" ? record.provider : null;
	if (!id || !provider) return null;
	return {
		id,
		name: typeof record.name === "string" ? record.name : undefined,
		provider,
		reasoning: record.reasoning === true,
	};
}

function createModes(session: PiLiteSessionState) {
	return {
		currentModeId: session.thinkingLevel,
		availableModes: thinkingModesFor(session.currentModel).map((id) => ({
			id,
			name: `Thinking: ${formatThinkingLabel(id)}`,
			label: formatThinkingLabel(id),
		})),
	};
}

function createConfigOptions(session: PiLiteSessionState) {
	const options: Array<Record<string, unknown>> = [
		{
			type: "select",
			id: "thought_level",
			name: "Thought Level",
			category: "thought_level",
			currentValue: session.thinkingLevel,
			options: thinkingModesFor(session.currentModel).map((value) => ({
				value,
				name: formatThinkingLabel(value),
			})),
		},
	];

	if (session.availableModels.length > 0) {
		options.unshift({
			type: "select",
			id: "model",
			name: "Model",
			category: "model",
			currentValue: session.currentModel
				? modelOptionValue(session.currentModel)
				: undefined,
			options: session.availableModels.map((model) => ({
				value: modelOptionValue(model),
				name: model.name
					? `${model.provider}/${model.id} (${model.name})`
					: `${model.provider}/${model.id}`,
			})),
		});
	}

	return options;
}

function sendLine(stream: NodeJS.WritableStream, value: JsonRecord): void {
	stream.write(`${JSON.stringify(value)}\n`);
}

class RpcProcess {
	private child: ChildProcess;
	private stdoutBuffer = "";
	private stderrBuffer = "";
	private closed = false;
	private readonly pending = new Map<string, PendingRequest>();
	private eventHandler: (event: JsonRecord) => void = () => {};
	private closeHandler: (error: RequestError) => void = () => {};

	constructor(
		command: string,
		args: string[],
		cwd: string,
		libraryPath?: string,
	) {
		const childEnv = { ...process.env };
		const effectiveLibraryPath =
			libraryPath ?? process.env.PI_LITE_LIBRARY_PATH;
		if (effectiveLibraryPath) {
			childEnv.LD_LIBRARY_PATH = [
				effectiveLibraryPath,
				process.env.LD_LIBRARY_PATH,
			]
				.filter(Boolean)
				.join(":");
		}
		this.child = spawn(command, args, {
			cwd,
			env: childEnv,
			stdio: ["pipe", "pipe", "pipe"],
		});

		this.child.stdout?.on("data", (chunk) => {
			this.stdoutBuffer += Buffer.from(chunk).toString("utf8");
			this.processStdoutBuffer();
		});
		this.child.stderr?.on("data", (chunk) => {
			this.stderrBuffer += Buffer.from(chunk).toString("utf8");
		});
		this.child.on("error", (error) => {
			const requestError = RequestError.internalError(
				{ cause: error.message, stderr: this.stderr() },
				"failed to spawn pi-lite",
			);
			this.failPending(requestError);
			this.closed = true;
			this.closeHandler(requestError);
		});
		this.child.on("exit", (code, signal) => {
			if (this.closed) return;
			this.closed = true;
			const requestError = RequestError.internalError(
				{ code, signal, stderr: this.stderr() },
				"pi-lite exited unexpectedly",
			);
			this.failPending(requestError);
			this.closeHandler(requestError);
		});
	}

	setEventHandler(handler: (event: JsonRecord) => void): void {
		this.eventHandler = handler;
	}

	setCloseHandler(handler: (error: RequestError) => void): void {
		this.closeHandler = handler;
	}

	request(command: string, payload: JsonRecord = {}): Promise<JsonRecord | undefined> {
		if (this.closed || !this.child.stdin) {
			return Promise.reject(
				RequestError.internalError(
					{ stderr: this.stderr() },
					"pi-lite process is not available",
				),
			);
		}

		const id = randomUUID();
		return new Promise<JsonRecord | undefined>((resolve, reject) => {
			this.pending.set(id, { command, resolve, reject });
			sendLine(this.child.stdin!, {
				id,
				type: command,
				...payload,
			});
		});
	}

	close(): void {
		if (this.closed) return;
		this.closed = true;
		this.child.kill("SIGTERM");
		this.failPending(
			RequestError.internalError(
				{ stderr: this.stderr() },
				"pi-lite process closed",
			),
		);
	}

	stderr(): string {
		return this.stderrBuffer.trim();
	}

	private processStdoutBuffer(): void {
		while (true) {
			const newline = this.stdoutBuffer.indexOf("\n");
			if (newline === -1) break;
			const line = this.stdoutBuffer.slice(0, newline).trim();
			this.stdoutBuffer = this.stdoutBuffer.slice(newline + 1);
			if (!line) continue;

			let value: JsonRecord;
			try {
				value = JSON.parse(line) as JsonRecord;
			} catch {
				continue;
			}

			if (value.type === "response") {
				this.handleResponse(value as RpcResponse);
				continue;
			}

			this.eventHandler(value);
		}
	}

	private handleResponse(response: RpcResponse): void {
		if (!response.id) return;
		const pending = this.pending.get(response.id);
		if (!pending) return;
		this.pending.delete(response.id);

		if (response.success) {
			pending.resolve(
				response.data && typeof response.data === "object"
					? (response.data as JsonRecord)
					: undefined,
			);
			return;
		}

		pending.reject(
			RequestError.internalError(
				{
					command: pending.command,
					error: response.error,
					errorHints: response.errorHints,
					stderr: this.stderr(),
				},
				response.error ?? `pi-lite ${pending.command} failed`,
			),
		);
	}

	private failPending(error: RequestError): void {
		for (const pending of this.pending.values()) {
			pending.reject(error);
		}
		this.pending.clear();
	}
}

function isModuleOverlayPath(path: string): boolean {
	return path.startsWith("/root/node_modules/");
}

async function copyFileChunked(
	sourcePath: string,
	destinationPath: string,
): Promise<void> {
	const sourceStat = statSync(sourcePath);
	const destinationExists =
		existsSync(destinationPath) &&
		statSync(destinationPath).size === sourceStat.size;
	if (destinationExists) return;

	const fd = openSync(destinationPath, "w", 0o755);
	try {
		await new Promise<void>((resolve, reject) => {
			const stream = createReadStream(sourcePath, {
				highWaterMark: 1024 * 1024,
			});
			stream.on("data", (chunk: string | Buffer) => {
				const buffer = Buffer.isBuffer(chunk)
					? chunk
					: Buffer.from(chunk);
				writeSync(fd, buffer, 0, buffer.length);
			});
			stream.on("end", resolve);
			stream.on("error", reject);
		});
	} finally {
		closeSync(fd);
	}

	chmodSync(destinationPath, sourceStat.mode & 0o777);
}

async function materializePiLiteLaunch(): Promise<{
	command: string;
	libraryPath?: string;
}> {
	const command = process.env.PI_LITE_COMMAND ?? "pi-lite";
	const libraryPath =
		process.env.PI_LITE_LIBRARY_PATH_HOST ??
		process.env.PI_LITE_LIBRARY_PATH;

	if (!isModuleOverlayPath(command) && !libraryPath) {
		return { command, libraryPath };
	}

	const tempDir = `/tmp/agent-os-pi-lite-${process.pid}`;
	mkdirSync(tempDir, { recursive: true });

	let effectiveCommand = command;
	if (isModuleOverlayPath(command)) {
		effectiveCommand = join(tempDir, basename(command));
		await copyFileChunked(command, effectiveCommand);
	}

	let effectiveLibraryPath = libraryPath;
	if (libraryPath && isModuleOverlayPath(libraryPath)) {
		const sqliteSource = join(libraryPath, "libsqlite3.so");
		const sqliteDestination = join(tempDir, "libsqlite3.so");
		await copyFileChunked(sqliteSource, sqliteDestination);
		effectiveLibraryPath = tempDir;
	}

	return {
		command: effectiveCommand,
		libraryPath: effectiveLibraryPath,
	};
}

class PiLiteAgent implements Agent {
	private readonly sessions = new Map<string, PiLiteSessionState>();

	constructor(private readonly conn: AgentSideConnection) {
		setTimeout(() => {
			void this.conn.closed.then(() => {
				for (const session of this.sessions.values()) {
					session.rpc.close();
				}
				this.sessions.clear();
			});
		}, 0);
	}

	async initialize(
		_params: InitializeRequest,
	): Promise<InitializeResponse> {
		return {
			protocolVersion: 1,
			agentInfo: {
				name: "pi-lite-acp",
				title: "Pi Lite ACP adapter",
				version: "0.1.0",
			},
			agentCapabilities: {
				tool_calls: true,
				text_messages: true,
				reasoning: true,
				streaming_deltas: true,
				session_lifecycle: true,
				promptCapabilities: {
					audio: false,
					embeddedContext: false,
					image: false,
				},
				sessionCapabilities: {
					close: {},
				},
			} as any,
		};
	}

	async newSession(
		params: NewSessionRequest,
	): Promise<NewSessionResponse> {
		const launch = await materializePiLiteLaunch();
		const args = [
			"--mode",
			"rpc",
			"--no-session",
			"--no-extensions",
			"--no-skills",
			"--no-prompt-templates",
			"--no-themes",
			"--hide-cwd-in-prompt",
			"--no-migrations",
		];
		if (appendSystemPrompt) {
			args.push("--append-system-prompt", appendSystemPrompt);
		}

		const rpc = new RpcProcess(
			launch.command,
			args,
			params.cwd,
			launch.libraryPath,
		);
		const session: PiLiteSessionState = {
			sessionId: randomUUID(),
			cwd: params.cwd,
			rpc,
			currentModel: null,
			availableModels: [],
			thinkingLevel: "off",
			activePrompt: null,
			currentToolCalls: new Map(),
			editSnapshots: new Map(),
			lastEmit: Promise.resolve(),
		};
		rpc.setEventHandler((event) => this.handleRpcEvent(session, event));
		rpc.setCloseHandler((error) => {
			session.activePrompt?.reject(error);
		});

		try {
			await this.refreshState(session);
		} catch (error) {
			rpc.close();
			throw error;
		}

		this.sessions.set(session.sessionId, session);

		return {
			sessionId: session.sessionId,
			modes: createModes(session) as any,
			configOptions: createConfigOptions(session) as any,
		};
	}

	async prompt(params: PromptRequest): Promise<PromptResponse> {
		const session = this.requireSession(params.sessionId);
		if (session.activePrompt) {
			throw RequestError.invalidRequest(
				{ sessionId: params.sessionId },
				"session already has an active prompt",
			);
		}

		const text = (params.prompt ?? [])
			.map((part: { type?: string; text?: string }) =>
				part.type === "text" ? (part.text ?? "") : "",
			)
			.join("");

		const execution = this.createPromptExecution();
		session.activePrompt = execution;
		session.currentToolCalls.clear();
		session.editSnapshots.clear();

		try {
			await session.rpc.request("prompt", { message: text });
			const response = await execution.promise;
			await session.lastEmit;
			return response;
		} catch (error) {
			execution.reject(error);
			throw error;
		} finally {
			session.activePrompt = null;
		}
	}

	async cancel(params: CancelNotification): Promise<void> {
		const session = this.requireSession(params.sessionId);
		if (session.activePrompt) {
			session.activePrompt.cancelRequested = true;
		}
		await session.rpc.request("abort").catch(() => undefined);
	}

	async setSessionMode(
		params: SetSessionModeRequest,
	): Promise<SetSessionModeResponse | void> {
		const session = this.requireSession(params.sessionId);
		const level = parseThinkingLevel(params.modeId);
		await session.rpc.request("set_thinking_level", { level });
		await this.refreshState(session);
		await this.conn.sessionUpdate({
			sessionId: session.sessionId,
			update: {
				sessionUpdate: "current_mode_update",
				currentModeId: session.thinkingLevel,
			},
		});
		await this.conn.sessionUpdate({
			sessionId: session.sessionId,
			update: {
				sessionUpdate: "config_option_update",
				configOptions: createConfigOptions(session) as any,
			},
		});
		return {};
	}

	async setSessionConfigOption(
		params: SetSessionConfigOptionRequest,
	): Promise<SetSessionConfigOptionResponse> {
		const session = this.requireSession(params.sessionId);
		if (typeof params.value !== "string") {
			throw RequestError.invalidParams(
				{ value: params.value },
				"pi-lite config options must be strings",
			);
		}

		if (params.configId === "model") {
			const { provider, modelId } = parseModelOptionValue(params.value);
			await session.rpc.request("set_model", {
				provider,
				modelId,
			});
		} else if (params.configId === "thought_level") {
			await session.rpc.request("set_thinking_level", {
				level: parseThinkingLevel(params.value),
			});
		} else {
			throw RequestError.invalidParams(
				{ configId: params.configId },
				"unsupported pi-lite config option",
			);
		}

		await this.refreshState(session);
		const configOptions = createConfigOptions(session);
		await this.conn.sessionUpdate({
			sessionId: session.sessionId,
			update: {
				sessionUpdate: "current_mode_update",
				currentModeId: session.thinkingLevel,
			},
		});
		await this.conn.sessionUpdate({
			sessionId: session.sessionId,
			update: {
				sessionUpdate: "config_option_update",
				configOptions: configOptions as any,
			},
		});
		return {
			configOptions: configOptions as any,
		};
	}

	async authenticate(
		_params: AuthenticateRequest,
	): Promise<AuthenticateResponse | void> {
	}

	private requireSession(sessionId: string): PiLiteSessionState {
		const session = this.sessions.get(sessionId);
		if (!session) {
			throw RequestError.invalidParams({ sessionId }, "unknown session");
		}
		return session;
	}

	private createPromptExecution(): PromptExecution {
		let resolve!: (value: PromptResponse) => void;
		let reject!: (reason?: unknown) => void;
		const promise = new Promise<PromptResponse>((res, rej) => {
			resolve = res;
			reject = rej;
		});
		return {
			cancelRequested: false,
			resolve,
			reject,
			promise,
		};
	}

	private async refreshState(session: PiLiteSessionState): Promise<void> {
		const [state, modelsPayload] = await Promise.all([
			session.rpc.request("get_state"),
			session.rpc.request("get_available_models"),
		]);
		const models = Array.isArray(modelsPayload?.models)
			? modelsPayload.models.map(parseRpcModel).filter(Boolean)
			: [];
		session.availableModels = models as RpcModel[];
		session.currentModel = parseRpcModel(state?.model);
		session.thinkingLevel =
			typeof state?.thinkingLevel === "string"
				? parseThinkingLevel(state.thinkingLevel)
				: "off";
		if (typeof state?.sessionId === "string" && state.sessionId.trim()) {
			if (session.sessionId !== state.sessionId) {
				this.sessions.delete(session.sessionId);
				session.sessionId = state.sessionId;
				this.sessions.set(session.sessionId, session);
			}
		}
	}

	private emit(
		session: PiLiteSessionState,
		update: SessionNotification["update"],
	): Promise<void> {
		session.lastEmit = session.lastEmit
			.then(() =>
				this.conn.sessionUpdate({
					sessionId: session.sessionId,
					update,
				}),
			)
			.catch(() => {});
		return session.lastEmit;
	}

	private handleRpcEvent(session: PiLiteSessionState, event: JsonRecord): void {
		switch (event.type) {
			case "message_update": {
				const assistantMessageEvent =
					event.assistantMessageEvent &&
					typeof event.assistantMessageEvent === "object"
						? (event.assistantMessageEvent as JsonRecord)
						: null;
				if (!assistantMessageEvent) break;

				if (assistantMessageEvent.type === "text_delta") {
					this.emit(session, {
						sessionUpdate: "agent_message_chunk",
						content: {
							type: "text",
							text: String(assistantMessageEvent.delta ?? ""),
						},
					});
				} else if (assistantMessageEvent.type === "thinking_delta") {
					this.emit(session, {
						sessionUpdate: "agent_thought_chunk",
						content: {
							type: "text",
							text: String(assistantMessageEvent.delta ?? ""),
						},
					});
				} else if (
					assistantMessageEvent.type === "toolcall_start" ||
					assistantMessageEvent.type === "toolcall_delta" ||
					assistantMessageEvent.type === "toolcall_end"
				) {
					this.handleToolCallMessage(session, assistantMessageEvent);
				}
				break;
			}

			case "tool_execution_start":
				this.handleToolExecutionStart(
					session,
					event as unknown as {
						toolCallId: string;
						toolName: string;
						args: unknown;
					},
				);
				break;

			case "tool_execution_update":
				this.handleToolExecutionUpdate(
					session,
					event as unknown as {
						toolCallId: string;
						partialResult: unknown;
					},
				);
				break;

			case "tool_execution_end":
				this.handleToolExecutionEnd(
					session,
					event as unknown as {
						toolCallId: string;
						result: unknown;
						isError: boolean;
					},
				);
				break;

			case "agent_end": {
				const execution = session.activePrompt;
				if (!execution) break;
				const error =
					typeof event.error === "string" && event.error.trim()
						? event.error
						: null;
				if (error) {
					execution.reject(
						RequestError.internalError(
							{ error, stderr: session.rpc.stderr() },
							error,
						),
					);
				} else {
					execution.resolve({
						stopReason: execution.cancelRequested ? "cancelled" : "end_turn",
					});
				}
				break;
			}
		}
	}

	private handleToolCallMessage(
		session: PiLiteSessionState,
		assistantMessageEvent: JsonRecord,
	): void {
		const toolCall =
			(assistantMessageEvent.toolCall as JsonRecord | undefined) ??
			(
				((assistantMessageEvent.partial as JsonRecord | undefined)?.content as
					| Array<JsonRecord>
					| undefined)
			)?.[(assistantMessageEvent.contentIndex as number) ?? 0];
		if (!toolCall) return;

		const toolCallId = String(toolCall.id ?? "");
		const toolName = String(toolCall.name ?? "tool");
		if (!toolCallId) return;

		const rawInput = this.parseToolArgs(toolCall);
		const locations = this.toToolCallLocations(session, rawInput);
		const existingStatus = session.currentToolCalls.get(toolCallId);
		const status = existingStatus ?? "pending";

		if (!existingStatus) {
			session.currentToolCalls.set(toolCallId, "pending");
			this.emit(session, {
				sessionUpdate: "tool_call",
				toolCallId,
				title: toolName,
				kind: toToolKind(toolName),
				status: "pending",
				locations,
				rawInput,
			});
		} else {
			this.emit(session, {
				sessionUpdate: "tool_call_update",
				toolCallId,
				status: status as "pending",
				locations,
				rawInput,
			});
		}
	}

	private handleToolExecutionStart(
		session: PiLiteSessionState,
		event: { toolCallId: string; toolName: string; args: unknown },
	): void {
		const { toolCallId, toolName, args } = event;
		const rawInput =
			args && typeof args === "object" ? (args as JsonRecord) : undefined;

		if (toolName === "edit" && rawInput) {
			const path = typeof rawInput.path === "string" ? rawInput.path : undefined;
			if (path) {
				try {
					const absolutePath = isAbsolute(path)
						? path
						: resolvePath(session.cwd, path);
					const oldText = readFileSync(absolutePath, "utf8");
					session.editSnapshots.set(toolCallId, { path, oldText });
				} catch {
				}
			}
		}

		const locations = this.toToolCallLocations(session, rawInput);
		if (!session.currentToolCalls.has(toolCallId)) {
			session.currentToolCalls.set(toolCallId, "in_progress");
			this.emit(session, {
				sessionUpdate: "tool_call",
				toolCallId,
				title: toolName,
				kind: toToolKind(toolName),
				status: "in_progress",
				locations,
				rawInput,
			});
		} else {
			session.currentToolCalls.set(toolCallId, "in_progress");
			this.emit(session, {
				sessionUpdate: "tool_call_update",
				toolCallId,
				status: "in_progress",
				locations,
				rawInput,
			});
		}
	}

	private handleToolExecutionUpdate(
		session: PiLiteSessionState,
		event: { toolCallId: string; partialResult: unknown },
	): void {
		const text = toolResultToText(event.partialResult);
		this.emit(session, {
			sessionUpdate: "tool_call_update",
			toolCallId: event.toolCallId,
			status: "in_progress",
			content: text
				? [{ type: "content", content: { type: "text", text } }]
				: undefined,
			rawOutput:
				event.partialResult && typeof event.partialResult === "object"
					? (event.partialResult as JsonRecord)
					: undefined,
		});
	}

	private handleToolExecutionEnd(
		session: PiLiteSessionState,
		event: { toolCallId: string; result: unknown; isError: boolean },
	): void {
		const text = toolResultToText(event.result);
		const snapshot = session.editSnapshots.get(event.toolCallId);

		let content:
			| Array<
					| { type: "diff"; path: string; oldText: string; newText: string }
					| { type: "content"; content: { type: "text"; text: string } }
			  >
			| undefined;

		if (!event.isError && snapshot) {
			try {
				const absolutePath = isAbsolute(snapshot.path)
					? snapshot.path
					: resolvePath(session.cwd, snapshot.path);
				const newText = readFileSync(absolutePath, "utf8");
				if (newText !== snapshot.oldText) {
					content = [
						{
							type: "diff",
							path: snapshot.path,
							oldText: snapshot.oldText,
							newText,
						},
						...(text
							? [
									{
										type: "content" as const,
										content: {
											type: "text" as const,
											text,
										},
									},
								]
							: []),
					];
				}
			} catch {
			}
		}

		if (!content && text) {
			content = [
				{
					type: "content",
					content: {
						type: "text",
						text,
					},
				},
			];
		}

		this.emit(session, {
			sessionUpdate: "tool_call_update",
			toolCallId: event.toolCallId,
			status: event.isError ? "failed" : "completed",
			content,
			rawOutput:
				event.result && typeof event.result === "object"
					? (event.result as JsonRecord)
					: undefined,
		});

		session.currentToolCalls.delete(event.toolCallId);
		session.editSnapshots.delete(event.toolCallId);
	}

	private parseToolArgs(toolCall: JsonRecord): JsonRecord | undefined {
		if (toolCall.arguments && typeof toolCall.arguments === "object") {
			return toolCall.arguments as JsonRecord;
		}
		const partialArgs = String(toolCall.partialArgs ?? "");
		if (!partialArgs) return undefined;
		try {
			return JSON.parse(partialArgs) as JsonRecord;
		} catch {
			return { partialArgs };
		}
	}

	private toToolCallLocations(
		session: PiLiteSessionState,
		args: JsonRecord | undefined,
	): Array<{ path: string; line?: number }> | undefined {
		const path = typeof args?.path === "string" ? args.path : undefined;
		if (!path) return undefined;
		return [
			{
				path: isAbsolute(path) ? path : resolvePath(session.cwd, path),
			},
		];
	}
}

function toToolKind(toolName: string): "read" | "edit" | "other" {
	if (toolName === "read") return "read";
	if (toolName === "write" || toolName === "edit") return "edit";
	return "other";
}

function toolResultToText(result: unknown): string {
	if (!result || typeof result !== "object") return "";
	const record = result as JsonRecord;
	const content = record.content;
	if (Array.isArray(content)) {
		const texts = content
			.map((item) =>
				item &&
				typeof item === "object" &&
				(item as JsonRecord).type === "text" &&
				typeof (item as JsonRecord).text === "string"
					? String((item as JsonRecord).text)
					: "",
			)
			.filter(Boolean);
		if (texts.length > 0) return texts.join("");
	}

	const details =
		record.details && typeof record.details === "object"
			? (record.details as JsonRecord)
			: undefined;
	const stdout =
		(typeof details?.stdout === "string" ? details.stdout : undefined) ??
		(typeof record.stdout === "string" ? record.stdout : undefined) ??
		(typeof details?.output === "string" ? details.output : undefined) ??
		(typeof record.output === "string" ? record.output : undefined);
	const stderr =
		(typeof details?.stderr === "string" ? details.stderr : undefined) ??
		(typeof record.stderr === "string" ? record.stderr : undefined);
	const exitCode =
		(typeof details?.exitCode === "number" ? details.exitCode : undefined) ??
		(typeof record.exitCode === "number" ? record.exitCode : undefined) ??
		(typeof details?.code === "number" ? details.code : undefined) ??
		(typeof record.code === "number" ? record.code : undefined);

	if (
		(typeof stdout === "string" && stdout.trim()) ||
		(typeof stderr === "string" && stderr.trim())
	) {
		const parts: string[] = [];
		if (typeof stdout === "string" && stdout.trim()) parts.push(stdout);
		if (typeof stderr === "string" && stderr.trim()) {
			parts.push(`stderr:\n${stderr}`);
		}
		if (typeof exitCode === "number") {
			parts.push(`exit code: ${exitCode}`);
		}
		return parts.join("\n\n").trimEnd();
	}

	return typeof record.error === "string" ? record.error : "";
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

const stream = ndJsonStream(input, output);
const connection = new AgentSideConnection(
	(conn: AgentSideConnection) => new PiLiteAgent(conn),
	stream,
);

process.stdin.resume();
process.stdin.on("end", () => {
	process.exit(0);
});

void connection.closed;
