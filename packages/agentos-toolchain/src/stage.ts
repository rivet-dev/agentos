import {
	copyFileSync,
	existsSync,
	mkdirSync,
	readdirSync,
	rmSync,
	statSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { readManifest } from "./manifest.js";

export interface StageOptions {
	/** Package root holding `agentos-package.json`; commands are staged under `bin/`. */
	packageDir: string;
	/** Directory of compiled command binaries (e.g. the native wasm build output). */
	commandsDir: string;
	/**
	 * What to do when the commands dir or a declared binary is absent.
	 * `"error"` (default) fails the stage; `"skip"` warns and preserves declared
	 * binaries already present in `bin/` while overlaying available artifacts.
	 * Skip mode is used by in-repo builds so `pnpm build` works on checkouts that
	 * have not run the complete native build.
	 */
	ifMissing?: "error" | "skip";
}

export interface StageResult {
	staged: string[];
	missing: string[];
}

/**
 * Populate a package's `bin/` from a commands directory, per the `commands` /
 * `aliases` / `stubs` lists declared in its `agentos-package.json`.
 *
 * Error mode wipes and rebuilds `bin/`. Skip mode overlays available artifacts,
 * preserves declared commands that are missing from a partial native target,
 * and prunes undeclared entries. Sources are copied dereferenced (`copyFileSync`
 * follows symlinks), so alias/stub symlink farms in the commands dir land as
 * real files that survive npm packing.
 */
export function stage(options: StageOptions): StageResult {
	const packageDir = resolve(options.packageDir);
	const commandsDir = resolve(options.commandsDir);
	const ifMissing = options.ifMissing ?? "error";

	const manifest = readManifest(packageDir);
	const commands = manifest?.commands ?? [];
	const aliases = manifest?.aliases ?? {};
	const stubs = manifest?.stubs ?? [];
	if (
		commands.length === 0 &&
		stubs.length === 0 &&
		Object.keys(aliases).length === 0
	) {
		process.stdout.write(
			`stage: no commands declared in ${join(packageDir, "agentos-package.json")} — nothing to stage\n`,
		);
		return { staged: [], missing: [] };
	}

	if (!existsSync(commandsDir)) {
		if (ifMissing === "skip") {
			process.stdout.write(
				`stage: commands dir not found (${commandsDir}) — leaving bin/ unstaged (placeholder package)\n`,
			);
			return { staged: [], missing: [...commands, ...stubs, ...Object.keys(aliases)] };
		}
		throw new Error(`stage: commands dir not found: ${commandsDir}`);
	}

	const binDir = join(packageDir, "bin");
	if (ifMissing === "error") {
		rmSync(binDir, { recursive: true, force: true });
		mkdirSync(binDir, { recursive: true });
	} else {
		// In-repo builds commonly have a partial native target after rebuilding a
		// single command. Overlay those fresh artifacts on the checked-in command
		// set instead of turning a complete package into a partial one. Still prune
		// entries removed from the manifest so skip mode cannot retain undeclared
		// commands forever.
		mkdirSync(binDir, { recursive: true });
		const declared = new Set([
			...commands,
			...stubs,
			...Object.keys(aliases),
		]);
		for (const entry of readdirSync(binDir)) {
			if (!declared.has(entry)) {
				rmSync(join(binDir, entry), { recursive: true, force: true });
			}
		}
	}

	const staged: string[] = [];
	const missing: string[] = [];

	for (const command of commands) {
		const source = join(commandsDir, command);
		if (!existsSync(source) || !statSync(source).isFile()) {
			missing.push(command);
			continue;
		}
		copyFileSync(source, join(binDir, command));
		staged.push(command);
	}

	const stubsSource = join(commandsDir, "_stubs");
	const hasStubsBinary = existsSync(stubsSource);
	for (const stub of stubs) {
		if (!hasStubsBinary) {
			missing.push(stub);
			continue;
		}
		copyFileSync(stubsSource, join(binDir, stub));
		staged.push(stub);
	}

	// Aliases copy from bin/ (not the commands dir) so they always match the
	// staged command, and so a target may itself be a stub.
	for (const [alias, target] of Object.entries(aliases)) {
		const source = join(binDir, target);
		if (!existsSync(source)) {
			missing.push(alias);
			continue;
		}
		copyFileSync(source, join(binDir, alias));
		staged.push(alias);
	}

	if (missing.length > 0) {
		const detail = `missing from ${commandsDir}: ${missing.join(", ")}`;
		if (ifMissing === "error") {
			throw new Error(`stage: ${detail}`);
		}
		process.stdout.write(`stage: WARN ${detail}\n`);
	}
	process.stdout.write(
		`staged ${staged.length} command(s) into ${binDir}\n`,
	);
	return { staged, missing };
}
