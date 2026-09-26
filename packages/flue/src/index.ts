import { createHash } from "node:crypto";
import { posix } from "node:path";
import {
	createSandboxSessionEnv,
	type FileStat,
	type SandboxApi,
	type SandboxFactory,
	type ShellResult,
} from "@flue/runtime";
import {
	type AgentOsActorCreateInput,
	type AgentOsClient,
	createAgentOsClient,
	type AgentOsActorConnection as GeneratedAgentOsActorConnection,
} from "@rivet-dev/agentos";
import type { AgentOs, VirtualStat } from "@rivet-dev/agentos-core";

const DEFAULT_CWD = "/workspace";
type AgentOsClientConfig = Parameters<typeof createAgentOsClient>[0];

interface AgentOSActorConnection {
	readonly ready: PromiseLike<void>;
	exec(
		command: string,
		options?: {
			cwd?: string;
			env?: Record<string, string>;
			timeout?: number;
			captureStdio?: boolean;
		},
	): Promise<ShellResult>;
	readFile(path: string): Promise<Uint8Array>;
	writeFile(path: string, content: string | Uint8Array): Promise<void>;
	stat(path: string): Promise<VirtualStat>;
	readdir(path: string): Promise<string[]>;
	exists(path: string): Promise<boolean>;
	mkdir(path: string, options?: { recursive?: boolean }): Promise<void>;
	remove(path: string, options?: { recursive?: boolean }): Promise<void>;
}

export interface AgentOSSandboxOptions {
	/** Creation input used only when the actor does not already exist. */
	createInput?: AgentOsActorCreateInput;
	/** RivetKit client configuration for the deployed agentOS actor. */
	clientConfig?: AgentOsClientConfig;
	/** Base directory exposed to Flue. Defaults to `/workspace`. */
	cwd?: string;
	/** Advanced: an existing generated agentOS client. */
	client?: AgentOsClient;
}

export interface AgentOSCoreSandboxOptions {
	/**
	 * Returns the caller-owned agentOS Core VM for a Flue context. The caller
	 * must retain and dispose VMs because Flue has no sandbox disposal hook.
	 */
	create(input: { id: string }): AgentOs | Promise<AgentOs>;
	/** Base directory exposed to Flue. Defaults to `/workspace`. */
	cwd?: string;
}

export class AgentOSFlueConfigurationError extends Error {
	readonly code = "agentos_flue_configuration";

	constructor(message: string, options?: ErrorOptions) {
		super(message, options);
		this.name = "AgentOSFlueConfigurationError";
	}
}

/** Uses the deployed Rust agentOS actor as the VM for each Flue context. */
export function agentOSSandbox(
	options: AgentOSSandboxOptions = {},
): SandboxFactory {
	const cwd = sandboxCwd(options.cwd);
	let client = options.client;
	const getClient = () =>
		(client ??= createAgentOsClient(options.clientConfig));

	return {
		async createSessionEnv({ id }) {
			const connection = await connectActor(
				getClient(),
				actorKey(id),
				options.createInput,
			);
			return createSandboxSessionEnv(actorSandboxApi(connection), cwd);
		},
	};
}

/** Uses caller-created standalone agentOS Core VMs as Flue sandboxes. */
export function agentOSCoreSandbox(
	options: AgentOSCoreSandboxOptions,
): SandboxFactory {
	if (!options || typeof options.create !== "function") {
		throw new TypeError("agentOSCoreSandbox requires a create factory");
	}
	const cwd = sandboxCwd(options.cwd);

	return {
		async createSessionEnv({ id }) {
			const vm = await options.create({ id });
			assertCoreVm(vm);
			return createSandboxSessionEnv(coreSandboxApi(vm), cwd);
		},
	};
}

async function connectActor(
	client: AgentOsClient,
	key: string[],
	createInput: AgentOsActorCreateInput | undefined,
): Promise<AgentOSActorConnection> {
	try {
		const connection = client.agentOS
			.getOrCreate(key, { createWithInput: createInput })
			.connect();
		await connection.ready;
		return actorConnection(connection);
	} catch (cause) {
		if (cause instanceof AgentOSFlueConfigurationError) throw cause;
		throw new AgentOSFlueConfigurationError(
			`the deployed agentOS actor could not be used: ${cause instanceof Error ? cause.message : String(cause)}`,
			{ cause },
		);
	}
}

function actorConnection(
	connection: GeneratedAgentOsActorConnection,
): AgentOSActorConnection {
	return {
		ready: connection.ready,
		async exec(command, options) {
			const result = await connection.process.run({
				command: "sh",
				args: ["-c", command],
				options: {
					cwd: options?.cwd,
					env: options?.env ?? {},
					timeoutMs: options?.timeout,
					captureStdio: options?.captureStdio,
				},
			});
			return {
				exitCode: result.status.exitCode,
				stdout: result.stdout,
				stderr: result.stderr,
			};
		},
		readFile: (path) => connection.filesystem.readFile({ path }),
		async writeFile(path, content) {
			await connection.filesystem.writeFile({ path, content });
		},
		async stat(path) {
			const stat = await connection.filesystem.stat({ path });
			return {
				...stat,
				size: Number(stat.sizeBytes),
				blocks: Number(stat.blocks),
				dev: Number(stat.dev),
				rdev: Number(stat.rdev),
				atimeMs: Number(stat.atimeMs),
				mtimeMs: Number(stat.mtimeMs),
				ctimeMs: Number(stat.ctimeMs),
				birthtimeMs: Number(stat.birthtimeMs),
				ino: Number(stat.ino),
				nlink: Number(stat.nlink),
			};
		},
		readdir: (path) => connection.filesystem.readdir({ path }),
		exists: (path) => connection.filesystem.exists({ path }),
		async mkdir(path, options) {
			await connection.filesystem.mkdir({
				path,
				recursive: options?.recursive ?? false,
			});
		},
		async remove(path, options) {
			await connection.filesystem.remove({
				path,
				recursive: options?.recursive ?? false,
			});
		},
	};
}

function assertCoreVm(vm: AgentOs): void {
	const methods = vm as unknown as Record<string, unknown>;
	for (const method of [
		"exec",
		"readFile",
		"writeFile",
		"stat",
		"readdir",
		"exists",
		"mkdir",
		"remove",
	]) {
		if (typeof methods?.[method] !== "function") {
			throw new TypeError(
				"agentOSCoreSandbox create() must return an AgentOs instance",
			);
		}
	}
}

function actorSandboxApi(connection: AgentOSActorConnection): SandboxApi {
	return sandboxApi({
		exec: (command, options) => connection.exec(command, options),
		readFile: (path) => connection.readFile(path),
		writeFile: (path, content) => connection.writeFile(path, content),
		stat: (path) => connection.stat(path),
		readdir: (path) => connection.readdir(path),
		exists: (path) => connection.exists(path),
		mkdir: (path, options) => connection.mkdir(path, options),
		remove: (path, options) => connection.remove(path, options),
	});
}

function coreSandboxApi(vm: AgentOs): SandboxApi {
	return sandboxApi({
		exec: (command, options) => vm.exec(command, options),
		readFile: (path) => vm.readFile(path),
		writeFile: (path, content) => vm.writeFile(path, content),
		stat: (path) => vm.stat(path),
		readdir: (path) => vm.readdir(path),
		exists: (path) => vm.exists(path),
		mkdir: (path, options) => vm.mkdir(path, options),
		remove: (path, options) => vm.remove(path, options),
	});
}

interface AgentOSSandboxApiSource {
	exec(
		command: string,
		options?: {
			cwd?: string;
			env?: Record<string, string>;
			timeout?: number;
			captureStdio?: boolean;
		},
	): Promise<ShellResult>;
	readFile(path: string): Promise<Uint8Array>;
	writeFile(path: string, content: string | Uint8Array): Promise<void>;
	stat(path: string): Promise<VirtualStat>;
	readdir(path: string): Promise<string[]>;
	exists(path: string): Promise<boolean>;
	mkdir(path: string, options?: { recursive?: boolean }): Promise<void>;
	remove(path: string, options?: { recursive?: boolean }): Promise<void>;
}

function sandboxApi(source: AgentOSSandboxApiSource): SandboxApi {
	return {
		async readFile(path) {
			return new TextDecoder().decode(await source.readFile(path));
		},
		readFileBuffer: source.readFile,
		writeFile: source.writeFile,
		async stat(path): Promise<FileStat> {
			const stat = await source.stat(path);
			return {
				isFile: (stat.mode & 0o170000) === 0o100000,
				isDirectory: stat.isDirectory,
				isSymbolicLink: stat.isSymbolicLink,
				size: stat.size,
				mtime: new Date(stat.mtimeMs),
			};
		},
		readdir: source.readdir,
		exists: source.exists,
		mkdir: source.mkdir,
		async rm(path, options) {
			if (options?.force && !(await source.exists(path))) return;
			try {
				await source.remove(path, { recursive: options?.recursive });
			} catch (error) {
				if (options?.force && !(await source.exists(path))) return;
				throw error;
			}
		},
		exec(command, options) {
			return source.exec(command, {
				cwd: options?.cwd,
				env: options?.env,
				timeout: options?.timeoutMs,
				captureStdio: true,
			});
		},
	};
}

function actorKey(id: string): string[] {
	return ["flue", "sandbox", stableId(id)];
}

function stableId(value: string): string {
	return createHash("sha256").update(value).digest("hex");
}

function sandboxCwd(value: string | undefined): string {
	const cwd = value ?? DEFAULT_CWD;
	if (!posix.isAbsolute(cwd)) {
		throw new TypeError("agentOS Flue sandbox cwd must be an absolute path");
	}
	return posix.normalize(cwd);
}
