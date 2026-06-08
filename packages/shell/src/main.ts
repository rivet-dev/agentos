#!/usr/bin/env node

import codex from "@rivet-dev/agent-os-codex";
// Software packages — uses npm-published versions which include pre-built
// WASM binaries. Workspace copies have empty wasm/ dirs since the native
// build (Rust nightly + wasi-sdk) is not run locally.
// curl, wget, sqlite3 are excluded (not yet published, need patched wasi-libc).
import common from "@rivet-dev/agent-os-common";
import { AgentOs } from "@rivet-dev/agent-os-core";
import fd from "@rivet-dev/agent-os-fd";
import file from "@rivet-dev/agent-os-file";
import jq from "@rivet-dev/agent-os-jq";
import ripgrep from "@rivet-dev/agent-os-ripgrep";
import tree from "@rivet-dev/agent-os-tree";
import unzip from "@rivet-dev/agent-os-unzip";
import yq from "@rivet-dev/agent-os-yq";
import zip from "@rivet-dev/agent-os-zip";

const software = [common, jq, ripgrep, fd, tree, file, zip, unzip, yq, codex];

function printUsage(): void {
	console.error(
		[
			"Usage:",
			"  agent-os-shell [--work-dir <path>] [--] [command] [args...]",
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
