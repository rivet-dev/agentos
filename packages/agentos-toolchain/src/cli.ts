#!/usr/bin/env node
import { existsSync, statSync } from "node:fs";
import { basename, resolve } from "node:path";
import { build } from "./build.js";
import { packAospkgFromTar } from "./aospkg.js";
import { pack } from "./pack.js";
import { stage } from "./stage.js";

const USAGE = `agentos-toolchain — build, stage, and pack agentOS packages

Usage:
  agentos-toolchain pack <npm-pkg | ./local-dir> [options]
      Pack an npm package or local script dir into a self-contained agentOS
      package tar (JavaScript package closures).
        --out <tar>         output tar (default: ./<input-name>-package.tar)
        --prune-native      delete unreachable native .node addons
        --omit-optional     omit optional npm dependencies from the closure

  agentos-toolchain stage [<packageDir>] --commands-dir <dir> [--if-missing skip|error]
      Populate <packageDir>/bin/ from a compiled commands directory, per the
      commands/aliases/stubs lists in agentos-package.json. --if-missing skip
      leaves a valid empty placeholder when binaries are absent (default: error).

  agentos-toolchain build [<packageDir>]
      Assemble the clean runtime tar dist/package.tar (bin/ + share/ +
      agentos-package.json) from <packageDir> (default: cwd).

  -h, --help          show this help
`;

/** Default output tar: ./<input-name>-package.tar in cwd. */
function defaultOutName(source: string): string {
	if (existsSync(source) && statSync(source).isDirectory()) {
		return `./${basename(resolve(source))}-package.tar`;
	}
	// npm spec: strip a trailing @version, then the @scope/ prefix.
	const at = source.lastIndexOf("@");
	const name = at > 0 ? source.slice(0, at) : source;
	return `./${name.replace(/^@[^/]+\//, "")}-package.tar`;
}

interface ParsedArgs {
	positional: string[];
	flags: Map<string, string | true>;
}

/** Flags in this set take a value; all others are booleans. */
const VALUE_FLAGS = new Set(["--out", "--commands-dir", "--if-missing"]);

function parseArgs(argv: string[]): ParsedArgs {
	const positional: string[] = [];
	const flags = new Map<string, string | true>();
	for (let i = 0; i < argv.length; i++) {
		const a = argv[i];
		if (a === "-h" || a === "--help") {
			process.stdout.write(USAGE);
			process.exit(0);
		} else if (VALUE_FLAGS.has(a)) {
			const value = argv[++i];
			if (value === undefined) throw new Error(`${a} requires a value`);
			flags.set(a, value);
		} else if (a.startsWith("-")) {
			flags.set(a, true);
		} else {
			positional.push(a);
		}
	}
	return { positional, flags };
}

function requireKnownFlags(args: ParsedArgs, known: string[]): void {
	for (const flag of args.flags.keys()) {
		if (!known.includes(flag)) throw new Error(`unexpected argument: ${flag}`);
	}
}

function main(): void {
	const [cmd, ...rest] = process.argv.slice(2);
	if (cmd === undefined) {
		process.stdout.write(USAGE);
		process.exit(1);
	}
	if (cmd === "-h" || cmd === "--help") {
		process.stdout.write(USAGE);
		process.exit(0);
	}
	const args = parseArgs(rest);

	switch (cmd) {
		case "pack-aospkg": {
			requireKnownFlags(args, []);
			const sourceTar = args.positional[0];
			const dest = args.positional[1];
			if (!sourceTar || !dest) {
				throw new Error(
					"pack-aospkg requires <source-tar> <dest-aospkg> arguments",
				);
			}
			const summary = packAospkgFromTar(resolve(sourceTar), resolve(dest));
			process.stdout.write(
				`packed ${summary.name}@${summary.version} → ${dest}\n`,
			);
			return;
		}
		case "pack": {
			requireKnownFlags(args, ["--out", "--prune-native", "--omit-optional"]);
			const source = args.positional[0];
			if (!source) {
				throw new Error("pack requires a <npm-pkg | ./local-dir> argument");
			}
			const result = pack({
				source,
				out: resolve(
					(args.flags.get("--out") as string | undefined) ??
						defaultOutName(source),
				),
				pruneNative: args.flags.get("--prune-native") === true,
				omitOptional: args.flags.get("--omit-optional") === true,
			});
			process.stdout.write(
				`packed ${result.name}@${result.version} → ${result.packageTar}\n` +
					`  commands: ${result.commands.join(", ")}\n`,
			);
			return;
		}
		case "stage": {
			requireKnownFlags(args, ["--commands-dir", "--if-missing"]);
			const commandsDir = args.flags.get("--commands-dir") as
				| string
				| undefined;
			if (!commandsDir) throw new Error("stage requires --commands-dir <dir>");
			const ifMissing = (args.flags.get("--if-missing") ?? "error") as string;
			if (ifMissing !== "error" && ifMissing !== "skip") {
				throw new Error(
					`--if-missing must be "skip" or "error", got "${ifMissing}"`,
				);
			}
			stage({
				packageDir: args.positional[0] ?? process.cwd(),
				commandsDir,
				ifMissing,
			});
			return;
		}
		case "build": {
			requireKnownFlags(args, []);
			build(args.positional[0]);
			return;
		}
		default:
			throw new Error(
				`unknown command "${cmd}" (expected pack, pack-aospkg, stage, or build)`,
			);
	}
}

try {
	main();
} catch (error) {
	process.stderr.write(
		`error: ${error instanceof Error ? error.message : String(error)}\n`,
	);
	process.exit(1);
}
