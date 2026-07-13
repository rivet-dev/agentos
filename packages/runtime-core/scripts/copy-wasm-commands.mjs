#!/usr/bin/env node

/**
 * Vendor the WASM coreutils/shell command binaries into `@rivet-dev/agentos-runtime-core`'s
 * package directory so they ship inside the published tarball.
 *
 * The runtime package vendors guest `sh` and coreutils commands for callers
 * that explicitly mount them. The binaries are produced
 * by the in-repo Rust command build at
 * `registry/native/target/wasm32-wasip1/release/commands/`. That path only
 * exists in a developer checkout, so we copy the whole command set (symlinks
 * included, the way `bash -> sh` and the stub aliases are laid out) into
 * `packages/runtime-core/commands/`, which is listed in `files` for published
 * installs.
 *
 * This mirrors how the sidecar binary ships via `@rivet-dev/agentos-runtime-sidecar`: the
 * artifact is vendored into a published package and resolved from the installed
 * package at runtime, never from an in-repo build path.
 */

import {
	copyFileSync,
	existsSync,
	lstatSync,
	mkdirSync,
	readdirSync,
	realpathSync,
	rmSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const PACKAGE_ROOT = fileURLToPath(new URL("..", import.meta.url));
const REPO_ROOT = fileURLToPath(new URL("../../..", import.meta.url));

const SOURCE_DIR = path.join(
	REPO_ROOT,
	"registry/native/target/wasm32-wasip1/release/commands",
);
const DEST_DIR = path.join(PACKAGE_ROOT, "commands");

function destIsPopulated() {
	try {
		return readdirSync(DEST_DIR).some((entry) => !entry.startsWith("."));
	} catch {
		return false;
	}
}

function main() {
	if (!existsSync(SOURCE_DIR)) {
		// No in-repo build output. If `commands/` is already populated (a clean
		// checkout that vendored earlier, or CI that dropped a prebuilt commands
		// artifact straight into the package), keep it: it already ships.
		if (destIsPopulated()) {
			console.log(
				`Using already-vendored commands at ${path.relative(REPO_ROOT, DEST_DIR)}; ` +
					`in-repo build output ${path.relative(REPO_ROOT, SOURCE_DIR)} is absent.`,
			);
			return;
		}
		// Nothing to ship. Don't fail the plain TypeScript build over it — a
		// developer who hasn't built the commands can still iterate, and runtime
		// resolution falls back to the in-repo path when it later appears. The
		// release/pack path guards this with `--require` so a published tarball is
		// never built without the commands.
		const message =
			`WASM commands not found at ${SOURCE_DIR} and none vendored at ${DEST_DIR}. ` +
			"Build them with `make -C registry/native wasm` (or drop a prebuilt " +
			"commands artifact into the package) before packing so they ship in the tarball.";
		if (process.argv.includes("--require")) {
			console.error(`error: ${message}`);
			process.exit(1);
		}
		console.warn(`warning: ${message} Skipping copy.`);
		return;
	}

	rmSync(DEST_DIR, { recursive: true, force: true });
	mkdirSync(DEST_DIR, { recursive: true });

	// Copy every command as a real file, dereferencing the build's alias
	// symlinks (bash -> sh, [ -> test, the `_stubs` aliases, dir/vdir -> ls,
	// gunzip/zcat -> gzip). The command discovery at runtime
	// (`discoverWasmCommandEntries`) keys each command off its filename and reads
	// the resolved bytes, so a dereferenced copy is functionally identical to the
	// symlink. Crucially, `npm pack` drops symlinks from the tarball, so shipping
	// them as real files is what makes the full command set survive into a
	// published install. The command tree is flat (no subdirectories), so a
	// single-level walk covers it.
	let copied = 0;
	for (const entry of readdirSync(SOURCE_DIR)) {
		if (entry.startsWith(".")) {
			continue;
		}
		const sourcePath = path.join(SOURCE_DIR, entry);
		const destPath = path.join(DEST_DIR, entry);
		const stat = lstatSync(sourcePath);
		// Resolve symlinks (and any chains) to the real file before copying so
		// the destination is a standalone binary, not a dangling link.
		const realSource = stat.isSymbolicLink()
			? realpathSync(sourcePath)
			: sourcePath;
		if (!lstatSync(realSource).isFile()) {
			continue;
		}
		copyFileSync(realSource, destPath);
		copied += 1;
	}

	console.log(
		`Copied ${copied} WASM command binaries to ${path.relative(REPO_ROOT, DEST_DIR)}`,
	);
}

main();
