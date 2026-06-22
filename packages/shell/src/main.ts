#!/usr/bin/env node

import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import codex from "@agentos-software/codex";
import common from "@agentos-software/common";
import { AgentOs } from "@rivet-dev/agentos-core";
import type { SoftwareInput } from "@rivet-dev/agentos-core";
import fd from "@agentos-software/fd";
import file from "@agentos-software/file";
import jq from "@agentos-software/jq";
import ripgrep from "@agentos-software/ripgrep";
import tree from "@agentos-software/tree";
import unzip from "@agentos-software/unzip";
import yq from "@agentos-software/yq";
import zip from "@agentos-software/zip";

const __dirname = dirname(fileURLToPath(import.meta.url));
const COMMAND_SUBPATH = "registry/native/target/wasm32-wasip1/release/commands";
// Published packages ship package-local wasm/ dirs. Workspace packages use the
// native build output: agent-os's own registry when present, or the sibling
// secure-exec checkout under `just secure-exec-local` (where the WASM is built).
const fallbackCommandDir = [
	resolve(__dirname, "../../..", COMMAND_SUBPATH),
	resolve(__dirname, "../../../../secure-exec", COMMAND_SUBPATH),
].find((dir) => existsSync(dir));

function withLocalCommandFallback(software: SoftwareInput): SoftwareInput {
	if (Array.isArray(software)) {
		return software.map(withLocalCommandFallback) as SoftwareInput;
	}

	if (
		fallbackCommandDir !== undefined &&
		"commandDir" in software &&
		typeof software.commandDir === "string" &&
		!existsSync(software.commandDir)
	) {
		const dir = fallbackCommandDir;
		return {
			...software,
			get commandDir() {
				return dir;
			},
		};
	}

	return software;
}

const software = [common, jq, ripgrep, fd, tree, file, zip, unzip, yq, codex].map(
	withLocalCommandFallback,
);

function printUsage(): void {
	console.error(
		[
			"Usage:",
			"  agentos-shell [--work-dir <path>] [--] [command] [args...]",
			"",
			"Options:",
			"  --work-dir <path>   Set the working directory inside the VM (default: /home/user)",
			"  --help, -h          Show this help",
			"",
			"Examples:",
			"  pnpm shell",
			"  pnpm shell --work-dir /tmp/demo",
			"  pnpm shell -- node -e 'console.log(42)'",
		].join("\n"),
	);
}

interface CliOptions {
	workDir?: string;
	command: string;
	args: string[];
}

function parseArgs(argv: string[]): CliOptions {
	const options: CliOptions = {
		command: "bash",
		args: [],
	};

	for (let i = 0; i < argv.length; i++) {
		const arg = argv[i];
		if (arg === "--") {
			const trailing = argv.slice(i + 1);
			if (trailing.length > 0) {
				options.command = trailing[0];
				options.args = trailing.slice(1);
			}
			break;
		}

		if (!arg.startsWith("-")) {
			options.command = arg;
			options.args = argv.slice(i + 1);
			break;
		}

		switch (arg) {
			case "--work-dir":
				if (!argv[i + 1]) {
					throw new Error("--work-dir requires a path");
				}
				options.workDir = argv[++i];
				break;
			case "--help":
			case "-h":
				printUsage();
				process.exit(0);
				return options;
			default:
				throw new Error(`Unknown argument: ${arg}`);
		}
	}

	return options;
}

async function runCommand(
	vm: AgentOs,
	cli: CliOptions,
	cwd: string,
): Promise<number> {
	const args =
		(cli.command === "bash" || cli.command === "sh") && cli.args.length === 0
			? ["-i"]
			: cli.args;
	const child = vm.spawn(cli.command, args, {
		cwd,
		onStdout: (data) => {
			process.stdout.write(data);
		},
		onStderr: (data) => {
			process.stderr.write(data);
		},
	});
	const restoreRawMode =
		process.stdin.isTTY && typeof process.stdin.setRawMode === "function";
	const onStdinData = (data: Uint8Array | string) => {
		vm.writeProcessStdin(child.pid, data);
	};

	try {
		if (restoreRawMode) {
			process.stdin.setRawMode(true);
		}
		process.stdin.on("data", onStdinData);
		process.stdin.resume();
		return await vm.waitProcess(child.pid);
	} finally {
		process.stdin.removeListener("data", onStdinData);
		process.stdin.pause();
		if (restoreRawMode) {
			process.stdin.setRawMode(false);
		}
	}
}

const cli = parseArgs(process.argv.slice(2));

const vm = await AgentOs.create({
	software,
});

const cwd = cli.workDir ?? "/home/user";

console.error("agent-os shell");
console.error(`cwd: ${cwd}`);

let exitCode = 1;
try {
	exitCode = await runCommand(vm, cli, cwd);
} finally {
	await vm.dispose();
}
process.exit(exitCode);
