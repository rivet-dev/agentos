import type {
	DriverProcess,
	KernelInterface,
	KernelRuntimeDriver,
	ProcessContext,
} from "@secure-exec/core";
import { spawn as hostSpawn } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname } from "node:path";

function resolveHostCwd(cwd: string): string {
	return existsSync(cwd) ? cwd : process.cwd();
}

class HostBinRuntime implements KernelRuntimeDriver {
	readonly name = "host-bin";
	readonly commands: string[];
	private readonly binaries: Map<string, string>;

	constructor(commands: Map<string, string>) {
		this.binaries = new Map(commands);
		this.commands = [...this.binaries.keys()];
	}

	async init(_kernel: KernelInterface): Promise<void> {
	}

	spawn(
		command: string,
		args: string[],
		ctx: ProcessContext,
	): DriverProcess {
		const commandName = command.includes("/")
			? (command.split("/").pop() ?? command)
			: command;
		const hostPath = this.binaries.get(commandName);
		if (!hostPath) {
			throw new Error(`Unknown host binary command: ${command}`);
		}

		const env = { ...ctx.env };
		env.LD_LIBRARY_PATH = env.LD_LIBRARY_PATH
			? `${dirname(hostPath)}:${env.LD_LIBRARY_PATH}`
			: dirname(hostPath);

		const child = hostSpawn(hostPath, args, {
			cwd: resolveHostCwd(ctx.cwd),
			env,
			stdio: ["pipe", "pipe", "pipe"],
		});

		let resolveExit!: (code: number) => void;
		const exitPromise = new Promise<number>((resolve) => {
			resolveExit = resolve;
		});

		const proc: DriverProcess = {
			onStdout: null,
			onStderr: null,
			onExit: null,
			writeStdin(data) {
				child.stdin?.write(data);
			},
			closeStdin() {
				child.stdin?.end();
			},
			kill(signal) {
				try {
					child.kill(signal > 0 ? signal : undefined);
				} catch {
				}
			},
			wait() {
				return exitPromise;
			},
		};

		child.stdout?.on("data", (chunk) => {
			const data = new Uint8Array(chunk);
			ctx.onStdout?.(data);
			proc.onStdout?.(data);
		});
		child.stderr?.on("data", (chunk) => {
			const data = new Uint8Array(chunk);
			ctx.onStderr?.(data);
			proc.onStderr?.(data);
		});
		child.on("error", (error) => {
			const data = new TextEncoder().encode(`${error.message}\n`);
			ctx.onStderr?.(data);
			proc.onStderr?.(data);
			resolveExit(1);
			proc.onExit?.(1);
		});
		child.on("close", (code, signal) => {
			const exitCode =
				typeof code === "number"
					? code
					: signal
						? 128 + 15
						: 1;
			resolveExit(exitCode);
			proc.onExit?.(exitCode);
		});

		return proc;
	}

	async dispose(): Promise<void> {
	}
}

export function createHostBinRuntime(
	commands: Map<string, string>,
): KernelRuntimeDriver {
	return new HostBinRuntime(commands);
}
