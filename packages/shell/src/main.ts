#!/usr/bin/env node

/**
 * Goal: `agentos-shell` should feel like the VM equivalent of `docker run`.
 *
 * Keep the CLI surface intentionally close to Docker's process flags:
 * `-i/--interactive` keeps stdin attached, `-t/--tty` connects a terminal,
 * `-e/--env` and `--env-file` inject environment variables, `-v/--volume`
 * and `--mount type=bind,...` mount host paths, and `-w/--workdir` chooses
 * the guest cwd. When TTY mode is requested, the guest command goes through
 * Agent OS's terminal API instead of a custom prompt or line editor; non-TTY
 * commands use process spawn with Docker-like stdin attachment rules.
 */

import {
	cpSync,
	existsSync,
	mkdirSync,
	mkdtempSync,
	readFileSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import codex from "@agentos-software/codex-cli";
import coreutils from "@agentos-software/coreutils";
import curl from "@agentos-software/curl";
import diffutils from "@agentos-software/diffutils";
import fd from "@agentos-software/fd";
import file from "@agentos-software/file";
import findutils from "@agentos-software/findutils";
import gawk from "@agentos-software/gawk";
import git from "@agentos-software/git";
import grep from "@agentos-software/grep";
import gzip from "@agentos-software/gzip";
import httpGet from "@agentos-software/http-get";
import jq from "@agentos-software/jq";
import ripgrep from "@agentos-software/ripgrep";
import sed from "@agentos-software/sed";
import sqlite3 from "@agentos-software/sqlite3";
import tar from "@agentos-software/tar";
import tree from "@agentos-software/tree";
import unzip from "@agentos-software/unzip";
import yq from "@agentos-software/yq";
import zip from "@agentos-software/zip";
import type { MountConfig, SoftwareInput } from "@rivet-dev/agentos-core";
import { AgentOs } from "@rivet-dev/agentos-core";
import { allowAll } from "@rivet-dev/agentos-core/internal/runtime-compat";
import { Command, Option } from "commander";
import { createActorShellVm, type ShellVmHandle } from "./actor-vm.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const workspaceRoot = resolve(__dirname, "../../..");
const fallbackCommandDirs = [
	resolve(
		workspaceRoot,
		"registry/native/target/wasm32-wasip1/release/commands",
	),
	resolve(
		workspaceRoot,
		"../secure-exec/registry/native/target/wasm32-wasip1/release/commands",
	),
];
const INTERACTIVE_SHELL_WASM_FUEL_MS = 24 * 60 * 60 * 1000;
const BRUSH_SHELL_COMMANDS = new Set(["bash", "sh"]);
const SHELL_OPTIONS_WITH_VALUES = new Set([
	"--command",
	"--debuglog-enable",
	"--init-file",
	"--input-backend",
	"--log-disable",
	"--rcfile",
	"-c",
]);

interface CliOptions {
	interactive: boolean;
	tty: boolean;
	actor: boolean;
	workdir: string;
	env: string[];
	envFile: string[];
	volume: string[];
	mount: string[];
	rm: boolean;
	name?: string;
	command: string;
	args: string[];
}

// The `@agentos-software/*` packages default-export a package descriptor
// pointing at their self-contained package directory (a `bin/` of wasm files +
// an `agentos-package.json`). Current packages export `{ packageDir }`; older
// builds exported `{ name, dir }` — accept both.
interface RegistryPackage {
	name?: string;
	dir?: string;
	packageDir?: string;
}

// The sidecar requires an agentos-package.json manifest in every package dir.
function isUsablePackageDir(dir: string | undefined): dir is string {
	return dir !== undefined && existsSync(resolve(dir, "agentos-package.json"));
}

// Published packages ship their package dir materialized. Workspace packages
// may not have run the native build yet, so their dir can be missing — fall
// back to the native command-build output when it is. A package whose dir
// exists but predates the agentos-package.json manifest requirement is skipped
// (with a warning) rather than aborting VM creation for the whole shell.
function withLocalCommandFallback(pkg: RegistryPackage): SoftwareInput | null {
	const packageDir = pkg.packageDir ?? pkg.dir;
	if (isUsablePackageDir(packageDir)) {
		return { packageDir };
	}
	const fallbackCommandDir = fallbackCommandDirs.find(isUsablePackageDir);
	if (fallbackCommandDir !== undefined) {
		return { packageDir: fallbackCommandDir };
	}
	console.warn(
		`agentos-shell: skipping software package without agentos-package.json: ${
			pkg.name ?? packageDir ?? "<unknown>"
		}`,
	);
	return null;
}

const software: SoftwareInput[] = [
	coreutils,
	sed,
	grep,
	gawk,
	findutils,
	diffutils,
	tar,
	gzip,
	curl,
	zip,
	unzip,
	jq,
	ripgrep,
	fd,
	tree,
	file,
	yq,
	codex,
	git,
	httpGet,
	sqlite3,
]
	.map(withLocalCommandFallback)
	.filter((input): input is SoftwareInput => input !== null);

// Local-only: minimal vi-like editors (vix, vim) built to wasm32-wasip1, used to
// prove raw-mode PTY round-trips (insert mode, :wq, file written). The raw
// binaries live in a workspace-local command dir; the sidecar wants a
// self-contained package directory, so materialize one with a `bin/<cmd>` layout
// and an `agentos-package.json`, then reference it via `{ packageDir }`.
const VIX_COMMAND_DIR = resolve(workspaceRoot, ".local-cmds");
const localEditors = (["vix", "vim"] as const).filter((name) =>
	existsSync(resolve(VIX_COMMAND_DIR, name)),
);

// Bare `vim` sources `$VIMRUNTIME/defaults.vim` on startup; without a runtime
// tree in the VM it fails with `E1187: Failed to source defaults.vim` and drops
// to the "Press ENTER" prompt. Provide a host vim runtime read-only through the
// package `provides` field and point `VIMRUNTIME` straight at it (which bypasses
// vim's version-dir search, so a 9.0/9.1 runtime sources cleanly under our 9.2
// binary).
const VIM_RUNTIME_GUEST_DIR = "/usr/local/share/vim/vim92";
const vimRuntimeHostDir = localEditors.includes("vim")
	? [
			resolve(VIX_COMMAND_DIR, "vim-runtime"),
			"/usr/share/vim/vim92",
			"/usr/share/vim/vim91",
			"/usr/share/vim/vim90",
			"/usr/local/share/vim/vim92",
		].find((dir) => existsSync(resolve(dir, "defaults.vim")))
	: undefined;

if (localEditors.length > 0) {
	// Assemble the package directory: bin/<cmd> for each editor, a package.json,
	// and an agentos-package.json carrying the runtime `provides` (env + files).
	const packageDir = mkdtempSync(join(tmpdir(), "agentos-local-editors-"));
	const binDir = join(packageDir, "bin");
	mkdirSync(binDir, { recursive: true });
	for (const name of localEditors) {
		cpSync(resolve(VIX_COMMAND_DIR, name), join(binDir, name));
	}
	writeFileSync(
		join(packageDir, "package.json"),
		JSON.stringify({ name: "local-editors", version: "0.0.0" }),
	);
	// The runtime tree is overlaid read-only into the package under `runtime/`,
	// so `provides.files.source` is a package-relative path the sidecar can read.
	let provides:
		| {
				env: Record<string, string>;
				files: Array<{ source: string; target: string }>;
		  }
		| undefined;
	if (vimRuntimeHostDir) {
		cpSync(vimRuntimeHostDir, join(packageDir, "runtime"), { recursive: true });
		provides = {
			env: {
				VIMRUNTIME: VIM_RUNTIME_GUEST_DIR,
				VIM: "/usr/local/share/vim",
			},
			files: [{ source: "runtime", target: VIM_RUNTIME_GUEST_DIR }],
		};
	}
	writeFileSync(
		join(packageDir, "agentos-package.json"),
		JSON.stringify({
			name: "local-editors",
			...(provides ? { provides } : {}),
		}),
	);
	software.push({ packageDir });
}

function createShellDiagnosticStripper(): (
	data: Uint8Array,
) => Uint8Array | null {
	let suppressUntilNewline = false;
	return (data: Uint8Array) => {
		let text = Buffer.from(data).toString("utf8");
		let output = "";

		while (text.length > 0) {
			if (suppressUntilNewline) {
				const newlineIndex = text.indexOf("\n");
				if (newlineIndex < 0) {
					return output.length > 0 ? Buffer.from(output, "utf8") : null;
				}
				text = text.slice(newlineIndex + 1);
				suppressUntilNewline = false;
				continue;
			}

			const warningIndex = text.indexOf("WARN could not retrieve pid");
			if (warningIndex < 0) {
				output += text;
				break;
			}

			const lineStartIndex = text.lastIndexOf("\n", warningIndex);
			const lineStart = lineStartIndex < 0 ? 0 : lineStartIndex + 1;
			output += text.slice(0, lineStart);

			const lineEnd = text.indexOf("\n", warningIndex);
			if (lineEnd < 0) {
				suppressUntilNewline = true;
				break;
			}
			text = text.slice(lineEnd + 1);
		}

		return output.length > 0 ? Buffer.from(output, "utf8") : null;
	};
}

function collectOption(value: string, previous: string[]): string[] {
	previous.push(value);
	return previous;
}

function parseCli(argv: string[]): CliOptions {
	const program = new Command()
		.name("agentos-shell")
		.description("Run a command or terminal inside an Agent OS VM.")
		.exitOverride()
		.passThroughOptions()
		.allowExcessArguments()
		.argument("[command]", "guest command to run", "bash")
		.argument("[args...]", "guest command arguments")
		.addOption(
			new Option("-i, --interactive", "keep stdin attached").default(false),
		)
		.addOption(new Option("-t, --tty", "connect a terminal").default(false))
		.addOption(
			new Option(
				"--actor",
				"run through the RivetKit agentOS actor (engine + dylib plugin) instead of the in-process core client",
			).default(false),
		)
		.option(
			"-e, --env <env>",
			"set environment variable (KEY=VALUE or KEY to copy from host)",
			collectOption,
			[],
		)
		.option(
			"--env-file <path>",
			"read environment variables from a file",
			collectOption,
			[],
		)
		.option(
			"-v, --volume <spec>",
			"bind mount a volume (host:guest[:ro|rw])",
			collectOption,
			[],
		)
		.option(
			"--mount <spec>",
			"bind mount using Docker syntax (type=bind,src=...,target=...,readonly)",
			collectOption,
			[],
		)
		.option("-w, --workdir <path>", "working directory inside the VM", "/")
		.option(
			"--name <name>",
			"container-style name label (accepted for Docker CLI parity)",
		)
		.option("--rm", "remove VM after exit (always true for this CLI)", false);

	try {
		program.parse(["node", "agentos-shell", ...argv]);
	} catch (error) {
		if (
			error &&
			typeof error === "object" &&
			"code" in error &&
			error.code === "commander.helpDisplayed"
		) {
			process.exit(0);
		}
		throw error;
	}

	const opts = program.opts<{
		interactive: boolean;
		tty: boolean;
		actor: boolean;
		workdir: string;
		env: string[];
		envFile: string[];
		volume: string[];
		mount: string[];
		rm: boolean;
		name?: string;
	}>();
	const [command = "bash", ...args] = program.args;

	return {
		interactive: opts.interactive,
		tty: opts.tty,
		actor: opts.actor,
		workdir: opts.workdir,
		env: opts.env,
		envFile: opts.envFile,
		volume: opts.volume,
		mount: opts.mount,
		rm: opts.rm,
		name: opts.name,
		command,
		args,
	};
}

function parseEnvLine(line: string): [string, string] | null {
	const trimmed = line.trim();
	if (!trimmed || trimmed.startsWith("#")) {
		return null;
	}
	const equalsIndex = trimmed.indexOf("=");
	if (equalsIndex < 0) {
		const hostValue = process.env[trimmed];
		return hostValue === undefined ? null : [trimmed, hostValue];
	}
	return [trimmed.slice(0, equalsIndex), trimmed.slice(equalsIndex + 1)];
}

function buildEnv(options: CliOptions): Record<string, string> {
	const env: Record<string, string> = {};
	for (const envFilePath of options.envFile) {
		const content = readFileSync(resolve(envFilePath), "utf8");
		for (const line of content.split(/\r?\n/)) {
			const entry = parseEnvLine(line);
			if (entry) {
				env[entry[0]] = entry[1];
			}
		}
	}
	for (const value of options.env) {
		const entry = parseEnvLine(value);
		if (entry) {
			env[entry[0]] = entry[1];
		}
	}
	return env;
}

function hostDirMount(
	hostPath: string,
	guestPath: string,
	readOnly: boolean,
): MountConfig {
	return {
		path: guestPath,
		readOnly,
		plugin: {
			id: "host_dir",
			config: {
				hostPath: resolve(hostPath),
				readOnly,
			},
		},
	};
}

function parseVolumeSpec(spec: string): MountConfig {
	const [hostPath, guestPath, mode] = spec.split(":");
	if (!hostPath || !guestPath) {
		throw new Error(
			`Invalid volume spec "${spec}"; expected host:guest[:ro|rw]`,
		);
	}
	if (mode && mode !== "ro" && mode !== "rw") {
		throw new Error(`Invalid volume mode "${mode}" in "${spec}"`);
	}
	return hostDirMount(hostPath, guestPath, mode === "ro");
}

function parseMountSpec(spec: string): MountConfig {
	const fields = new Map<string, string | true>();
	for (const rawPart of spec.split(",")) {
		const part = rawPart.trim();
		if (!part) {
			continue;
		}
		const equalsIndex = part.indexOf("=");
		if (equalsIndex < 0) {
			fields.set(part, true);
		} else {
			fields.set(part.slice(0, equalsIndex), part.slice(equalsIndex + 1));
		}
	}

	if (fields.get("type") !== "bind") {
		throw new Error(`Only bind mounts are supported: --mount ${spec}`);
	}
	const source = fields.get("source") ?? fields.get("src");
	const target =
		fields.get("target") ?? fields.get("dst") ?? fields.get("destination");
	if (typeof source !== "string" || typeof target !== "string") {
		throw new Error(
			`Invalid mount spec "${spec}"; expected type=bind,source=...,target=...`,
		);
	}
	const readOnly = fields.has("readonly") || fields.get("ro") === "true";
	return hostDirMount(source, target, readOnly);
}

function buildMounts(options: CliOptions): MountConfig[] {
	return [
		...options.volume.map(parseVolumeSpec),
		...options.mount.map(parseMountSpec),
	];
}

function isBrushShellCommand(command: string): boolean {
	return BRUSH_SHELL_COMMANDS.has(basename(command));
}

function hasBrushInputBackend(args: string[]): boolean {
	return args.some(
		(arg) => arg === "--input-backend" || arg.startsWith("--input-backend="),
	);
}

function hasInteractiveShellFlag(args: string[]): boolean {
	return args.some((arg) => arg === "-i" || arg === "--interactive");
}

function shellArgsRequestCommandOrScript(args: string[]): boolean {
	for (let i = 0; i < args.length; i++) {
		const arg = args[i];
		if (arg === "--") {
			return i + 1 < args.length;
		}
		if (arg.startsWith("--") && arg.includes("=")) {
			if (arg.startsWith("--command=")) {
				return true;
			}
			continue;
		}
		if (SHELL_OPTIONS_WITH_VALUES.has(arg)) {
			if (arg === "-c" || arg === "--command") {
				return true;
			}
			i++;
			continue;
		}
		if (arg.startsWith("-")) {
			continue;
		}
		return true;
	}
	return false;
}

// The error brush prints when the requested input backend is not compiled
// into the wasm build (e.g. reedline missing from the shipped package). Used
// to auto-fall back to the always-available `minimal` backend.
const BRUSH_BACKEND_UNSUPPORTED_MARKER =
	"requested input backend type not supported";

/**
 * Terminal command candidates, tried in order. Prefer `reedline` (history,
 * arrows, reverse search — requires a brush wasm build with the reedline
 * feature) and fall back to `minimal` when the shipped build rejects it. An
 * explicit user-provided backend is used verbatim with no fallback.
 */
function buildTerminalCommandAttempts(options: CliOptions): {
	command: string;
	args: string[];
}[] {
	const baseArgs = [...options.args];
	if (!isBrushShellCommand(options.command)) {
		return [{ command: options.command, args: baseArgs }];
	}
	if (
		!hasInteractiveShellFlag(baseArgs) &&
		!shellArgsRequestCommandOrScript(baseArgs)
	) {
		baseArgs.push("-i");
	}
	if (hasBrushInputBackend(baseArgs)) {
		// Explicit backend: use it verbatim; the brush error surfaces as-is.
		return [{ command: options.command, args: baseArgs }];
	}
	return ["reedline", "minimal"].map((backend) => ({
		command: options.command,
		args: ["--input-backend", backend, ...baseArgs],
	}));
}

async function runSpawnedCommand(
	vm: ShellVmHandle,
	options: CliOptions,
	env: Record<string, string>,
): Promise<number> {
	const child = await vm.spawn(options.command, options.args, {
		cwd: options.workdir,
		env,
		streamStdin: options.interactive,
		onStdout: (data) => {
			process.stdout.write(data);
		},
		onStderr: (data: Uint8Array) => {
			process.stderr.write(data);
		},
	});
	let stdinQueue = Promise.resolve();
	const queueStdin = (operation: () => Promise<void>) => {
		stdinQueue = stdinQueue.then(operation);
		void stdinQueue.catch((error) => {
			const message = error instanceof Error ? error.message : String(error);
			process.stderr.write(`${message}\n`);
		});
	};
	const closeChildStdin = () => {
		queueStdin(async () => {
			try {
				await vm.closeProcessStdin(child.pid);
			} catch {
				// The process may have already exited before host stdin reports EOF.
			}
		});
	};
	const onStdinData = (data: Uint8Array | string) => {
		queueStdin(() => vm.writeProcessStdin(child.pid, data));
	};

	if (!options.interactive) {
		closeChildStdin();
		return vm.waitProcess(child.pid);
	}

	try {
		process.stdin.on("data", onStdinData);
		process.stdin.once("end", closeChildStdin);
		process.stdin.once("error", closeChildStdin);
		process.stdin.resume();
		return await vm.waitProcess(child.pid);
	} finally {
		process.stdin.removeListener("data", onStdinData);
		process.stdin.removeListener("end", closeChildStdin);
		process.stdin.removeListener("error", closeChildStdin);
		process.stdin.pause();
	}
}

async function runTerminalCommand(
	vm: ShellVmHandle,
	options: CliOptions,
	env: Record<string, string>,
): Promise<number> {
	const attempts = buildTerminalCommandAttempts(options);
	for (let index = 0; index < attempts.length; index++) {
		const canFallback = index + 1 < attempts.length;
		const result = await runTerminalAttempt(
			vm,
			options,
			env,
			attempts[index],
			canFallback,
		);
		if (result.backendUnsupported && canFallback) {
			process.stderr.write(
				"agentos-shell: shell build does not support the requested input backend; retrying with --input-backend minimal\n",
			);
			continue;
		}
		return result.exitCode;
	}
	return 1;
}

async function runTerminalAttempt(
	vm: ShellVmHandle,
	options: CliOptions,
	env: Record<string, string>,
	terminalCommand: { command: string; args: string[] },
	canFallback: boolean,
): Promise<{ exitCode: number; backendUnsupported: boolean }> {
	const stripDiagnostics = createShellDiagnosticStripper();
	let backendUnsupported = false;
	const decoder = new TextDecoder();
	const detectBackendError = (data: Uint8Array) => {
		if (decoder.decode(data).includes(BRUSH_BACKEND_UNSUPPORTED_MARKER)) {
			backendUnsupported = true;
		}
	};
	// Suppress the backend error output only when a fallback attempt will run;
	// without one the error must reach the user.
	const suppress = () => backendUnsupported && canFallback;
	const shellOptions = {
		cwd: options.workdir,
		env,
		cols: process.stdout.columns,
		rows: process.stdout.rows,
		onStderr: (data: Uint8Array) => {
			detectBackendError(data);
			if (suppress()) return;
			const sanitized = stripDiagnostics(data);
			if (sanitized) process.stderr.write(sanitized);
		},
	};
	const { shellId } = await vm.openShell({
		...shellOptions,
		...terminalCommand,
	});
	let stdinQueue = Promise.resolve();
	const queueShellInput = (data: Uint8Array | string) => {
		stdinQueue = stdinQueue.then(() => vm.writeShell(shellId, data));
		void stdinQueue.catch((error) => {
			const message = error instanceof Error ? error.message : String(error);
			process.stderr.write(`${message}\n`);
		});
	};
	const onStdinData = (data: Uint8Array | string) => {
		queueShellInput(data);
	};
	const onStdinEnd = () => {
		queueShellInput("\u0004");
	};
	const onResize = () => {
		vm.resizeShell(shellId, process.stdout.columns, process.stdout.rows);
	};
	const unsubscribeOutput = vm.onShellData(shellId, (data) => {
		detectBackendError(data);
		if (suppress()) return;
		const sanitized = stripDiagnostics(data);
		if (sanitized) process.stdout.write(sanitized);
	});
	const canUseRawMode =
		options.interactive &&
		process.stdin.isTTY &&
		typeof process.stdin.setRawMode === "function";
	let rawModeEnabled = false;

	try {
		if (options.interactive) {
			if (canUseRawMode) {
				process.stdin.setRawMode(true);
				rawModeEnabled = true;
			}
			process.stdin.on("data", onStdinData);
			process.stdin.once("end", onStdinEnd);
			process.stdin.once("error", onStdinEnd);
			process.stdin.resume();
		}
		if (process.stdout.isTTY) {
			process.stdout.on("resize", onResize);
			onResize();
		}

		const exitCode = await vm.waitShell(shellId);
		// Give trailing output events a moment to flush before unsubscribing:
		// a fast-failing shell otherwise exits with its error output silently
		// dropped, leaving nothing but a bare exit code to debug from.
		await new Promise((r) => setTimeout(r, 250));
		return { exitCode, backendUnsupported };
	} finally {
		unsubscribeOutput();
		process.stdin.removeListener("data", onStdinData);
		process.stdin.removeListener("end", onStdinEnd);
		process.stdin.removeListener("error", onStdinEnd);
		process.stdin.pause();
		if (rawModeEnabled) {
			process.stdin.setRawMode(false);
		}
		if (process.stdout.isTTY) {
			process.stdout.removeListener("resize", onResize);
		}
	}
}

const cli = parseCli(process.argv.slice(2));
const env = buildEnv(cli);
if (cli.tty && env.AGENTOS_V8_CPU_TIME_LIMIT_MS === undefined) {
	// Interactive sessions idle for hours; the wasm runner's input polling
	// accrues active-CPU against the default 30s watchdog and kills the guest
	// (vim/reedline die mid-session). Match the 24h interactive fuel budget.
	env.AGENTOS_V8_CPU_TIME_LIMIT_MS = String(INTERACTIVE_SHELL_WASM_FUEL_MS);
}
const mounts = buildMounts(cli);
const limits = cli.tty
	? {
			resources: {
				maxWasmFuel: INTERACTIVE_SHELL_WASM_FUEL_MS,
			},
		}
	: undefined;

const vm: ShellVmHandle = cli.actor
	? await createActorShellVm({ software, mounts, limits })
	: await AgentOs.create({
			mounts,
			permissions: allowAll,
			software,
			limits,
		});

let exitCode = 1;
try {
	const useTerminal = cli.tty && process.stdin.isTTY && process.stdout.isTTY;
	exitCode = useTerminal
		? await runTerminalCommand(vm, cli, env)
		: await runSpawnedCommand(vm, cli, env);
} finally {
	await vm.dispose();
}
process.exit(exitCode);
