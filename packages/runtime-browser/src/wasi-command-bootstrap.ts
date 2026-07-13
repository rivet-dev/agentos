export interface WasiCommandBootstrapOptions {
	/** Main WASI command source URL, usually a staged browser asset such as /commands/sh. */
	commandSource: string;
	/** argv[0] for the main command. */
	command: string;
	/** Additional argv entries for the main command. */
	args?: string[];
	/** Command registry used by host_process.proc_spawn for child WASI commands. */
	commands?: Record<string, string>;
	/**
	 * Real WASM command files to stage in the guest VFS before the main command
	 * starts. Child process resolution then follows the executable path selected
	 * by the guest shell instead of a host-side basename registry.
	 */
	commandFiles?: Record<string, string>;
	/**
	 * Commands delegated to the browser SystemDriver CommandExecutor. This is the
	 * JavaScript-command counterpart to `commands`: argv and stdio still originate
	 * in the guest shell, while the trusted browser driver selects the executor.
	 */
	externalCommands?: string[];
	env?: Record<string, string>;
	cwd?: string;
	preopens?: Record<string, string>;
	bootMessage?: string;
	bytesMessagePrefix?: string;
	startMessage?: string;
	exitMessagePrefix?: string;
	errorMessagePrefix?: string;
}

function json(value: unknown): string {
	return JSON.stringify(value);
}

/**
 * Builds the guest JavaScript that launches a real WASI command in the browser
 * executor with the production `secure-exec:wasi-command-host` spawn/FD bridge.
 */
export function createWasiCommandBootstrapScript(
	options: WasiCommandBootstrapOptions,
): string {
	const env = options.env ?? {};
	const cwd = options.cwd ?? "/";
	const preopens = options.preopens ?? { "/": "/" };
	const args = [options.command, ...(options.args ?? [])];
	const commands = options.commands ?? {};
	const commandFiles = options.commandFiles ?? {};
	const externalCommands = options.externalCommands ?? [];
	const bootMessage = options.bootMessage ?? "";
	const bytesMessagePrefix = options.bytesMessagePrefix ?? "";
	const startMessage = options.startMessage ?? "";
	const exitMessagePrefix = options.exitMessagePrefix ?? "WASI_EXIT:";
	const errorMessagePrefix =
		options.errorMessagePrefix ?? "WASI_COMMAND_ERROR:";

	return `
	(async () => {
		try {
			const commandSource = ${json(options.commandSource)};
			if (${json(bootMessage)}) process.stdout.write(${json(bootMessage)} + "\\n");
			const response = await fetch(commandSource);
			if (!response.ok) {
				throw new Error("failed to fetch real command wasm " + commandSource + ": " + response.status);
			}
			let bytes = new Uint8Array(await response.arrayBuffer());
			if (response.headers.get("x-body-encoding") === "base64") {
				const encoded = new TextDecoder().decode(bytes);
				bytes = Uint8Array.from(atob(encoded), (char) => char.charCodeAt(0));
			}
				if (${json(bytesMessagePrefix)}) {
					process.stdout.write(${json(bytesMessagePrefix)} + bytes.byteLength + "\\n");
				}
				const commandFiles = ${json(commandFiles)};
				if (Object.keys(commandFiles).length > 0) {
					const fs = require("node:fs");
					const path = require("node:path");
					for (const [guestPath, source] of Object.entries(commandFiles)) {
						let commandBytes = bytes;
						if (source !== commandSource) {
							const commandResponse = await fetch(source);
							if (!commandResponse.ok) {
								throw new Error("failed to fetch real command wasm " + source + ": " + commandResponse.status);
							}
							commandBytes = new Uint8Array(await commandResponse.arrayBuffer());
							if (commandResponse.headers.get("x-body-encoding") === "base64") {
								const encoded = new TextDecoder().decode(commandBytes);
								commandBytes = Uint8Array.from(atob(encoded), (char) => char.charCodeAt(0));
							}
						}
						fs.mkdirSync(path.dirname(guestPath), { recursive: true });
						fs.writeFileSync(guestPath, commandBytes);
						fs.chmodSync(guestPath, 0o755);
					}
				}
				const WASI = globalThis.__agentOSWasiModule && globalThis.__agentOSWasiModule.WASI;
			if (typeof WASI !== "function") {
				throw new Error("browser AgentOS WASI command host is not initialized");
			}
			const createWasiCommandHost = globalThis.__agentOSWasiCommandHost && globalThis.__agentOSWasiCommandHost.createWasiCommandHost;
			if (typeof createWasiCommandHost !== "function") {
				throw new Error("browser AgentOS WASI process host is not initialized");
			}
			const commandHost = await createWasiCommandHost({
				WASI,
				commands: ${json(commands)},
				externalCommands: ${json(externalCommands)},
				env: ${json(env)},
				cwd: ${json(cwd)},
			});
			commandHost.installBlockingStdin(process);
			const wasi = new WASI({
				returnOnExit: true,
				args: ${json(args)},
				env: ${json(env)},
				preopens: ${json(preopens)},
			});
			commandHost.setParentWasi(wasi);
			const { instance } = await WebAssembly.instantiate(bytes, {
				wasi_snapshot_preview1: wasi.wasiImport,
				...commandHost.imports,
			});
			commandHost.setMemory(instance.exports.memory);
			if (${json(startMessage)}) process.stdout.write(${json(startMessage)} + "\\n");
			const exitCode = wasi.start(instance);
			process.stdout.write(${json(exitMessagePrefix)} + exitCode + "\\n");
		} catch (error) {
			process.stderr.write(${json(errorMessagePrefix)} + (error && error.stack ? error.stack : String(error)) + "\\n");
			process.exit(1);
		}
	})();
`;
}
