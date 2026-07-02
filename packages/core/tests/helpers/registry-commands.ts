/**
 * Registry software packages for tests — STRICT, no silent skips.
 *
 * Every `@agentos-software/*` package exports `{ packageDir }` pointing at its
 * registry-built runtime dir (`dist/package/`). Importing this helper THROWS
 * with build instructions when a standard package is not built, instead of
 * letting suites silently skip: with the committed file-linked deps, "not
 * built" always means the sibling secure-exec registry needs building.
 *
 * The only sanctioned exception is the C-sysroot package set (duckdb,
 * http-get, sqlite3, wget, zip, unzip): those need the patched wasi C sysroot
 * that most checkouts don't have, so `cSysrootPackageSkipReason` reports a
 * skip reason instead of throwing. Everything else is load-or-throw.
 */

import {
	copyFileSync,
	existsSync,
	mkdirSync,
	readFileSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import codex from "@agentos-software/codex-cli";
import coreutils from "@agentos-software/coreutils";
import curl from "@agentos-software/curl";
import diffutils from "@agentos-software/diffutils";
import fd from "@agentos-software/fd";
import file from "@agentos-software/file";
import findutils from "@agentos-software/findutils";
import gawk from "@agentos-software/gawk";
import grep from "@agentos-software/grep";
import gzip from "@agentos-software/gzip";
import jq from "@agentos-software/jq";
import ripgrep from "@agentos-software/ripgrep";
import sed from "@agentos-software/sed";
import tar from "@agentos-software/tar";
import tree from "@agentos-software/tree";
import yq from "@agentos-software/yq";

export interface RegistryPackageRef {
	packageDir: string;
}

const BUILD_INSTRUCTIONS =
	"Build the registry in the sibling secure-exec checkout:\n" +
	"  just registry-native   # native wasm binaries, once per checkout (slow)\n" +
	"  just registry-build    # stage bin/ + assemble every dist/package\n" +
	"See secure-exec registry/README.md.";

/** Read a built package's `dist/package/package.json` bin map, or null. */
function readBinMap(dir: string): Record<string, string> | null {
	const manifestPath = join(dir, "package.json");
	if (!existsSync(manifestPath)) return null;
	try {
		const manifest = JSON.parse(readFileSync(manifestPath, "utf8")) as {
			bin?: Record<string, string>;
		};
		return manifest.bin ?? {};
	} catch {
		return null;
	}
}

function builtState(pkg: RegistryPackageRef): {
	bin: Record<string, string> | null;
	missing: string[];
} {
	const bin = readBinMap(pkg.packageDir);
	if (bin === null) return { bin, missing: [] };
	const missing = Object.entries(bin)
		.filter(([, rel]) => !existsSync(join(pkg.packageDir, rel)))
		.map(([cmd]) => cmd);
	return { bin, missing };
}

/**
 * Assert a registry package is built (assembled `dist/package` with a
 * non-empty, fully-present command set) and return it. Throws with build
 * instructions otherwise.
 */
export function requireBuilt<T extends RegistryPackageRef>(
	pkg: T,
	name: string,
): T {
	const { bin, missing } = builtState(pkg);
	if (bin === null) {
		throw new Error(
			`registry package ${name} is NOT BUILT (no ${pkg.packageDir}/package.json).\n${BUILD_INSTRUCTIONS}`,
		);
	}
	if (Object.keys(bin).length === 0) {
		throw new Error(
			`registry package ${name} is an EMPTY placeholder (no commands staged into bin/).\n${BUILD_INSTRUCTIONS}`,
		);
	}
	if (missing.length > 0) {
		throw new Error(
			`registry package ${name} is missing built commands: ${missing.join(", ")}.\n${BUILD_INSTRUCTIONS}`,
		);
	}
	return pkg;
}

/**
 * Skip reason for the C-sysroot package set ONLY (duckdb, http-get, sqlite3,
 * wget, zip, unzip). These need the patched wasi C sysroot
 * (`make -C registry/native/c` in secure-exec), which most checkouts don't
 * build — a missing artifact is an environment limitation, not a forgotten
 * build, so suites may skip with this reason instead of throwing.
 */
export function cSysrootPackageSkipReason(
	...packages: Array<{ pkg: RegistryPackageRef; name: string }>
): string | false {
	const unbuilt = packages.filter(({ pkg }) => {
		const { bin, missing } = builtState(pkg);
		return bin === null || Object.keys(bin).length === 0 || missing.length > 0;
	});
	if (unbuilt.length === 0) return false;
	return (
		`C-sysroot registry packages not built: ${unbuilt.map(({ name }) => name).join(", ")} ` +
		"(needs the patched wasi C sysroot: `make -C registry/native/c` in secure-exec, then `just registry-build`)"
	);
}

/** All standard registry software packages — throws at import if any is unbuilt. */
export const REGISTRY_SOFTWARE = (
	[
		[coreutils, "coreutils"],
		[sed, "sed"],
		[grep, "grep"],
		[gawk, "gawk"],
		[findutils, "findutils"],
		[diffutils, "diffutils"],
		[tar, "tar"],
		[gzip, "gzip"],
		[jq, "jq"],
		[ripgrep, "ripgrep"],
		[fd, "fd"],
		[tree, "tree"],
		[file, "file"],
		[yq, "yq"],
		[codex, "codex-cli"],
		[curl, "curl"],
	] as Array<[RegistryPackageRef, string]>
).map(([pkg, name]) => requireBuilt(pkg, name));

/**
 * Test-only commands (e.g. `xu`, a registry VM-test binary) ship in NO
 * software package — they exist only in the native build output of the linked
 * secure-exec checkout. Synthesize a minimal package around them so suites can
 * project them like any other software. Throws when the native build output is
 * absent (same build instructions as everything else).
 */
export function testOnlyCommandSoftware(
	commands: string[] = ["xu"],
): RegistryPackageRef {
	// registry/software/<pkg>/dist/package -> registry/native/.../commands, so
	// this follows whichever secure-exec checkout the deps are linked to.
	const nativeCommandsDir = join(
		coreutils.packageDir,
		"../../../..",
		"native/target/wasm32-wasip1/release/commands",
	);
	const dir = join(tmpdir(), `agentos-test-cmds-${process.pid}`);
	const binDir = join(dir, "bin");
	mkdirSync(binDir, { recursive: true });
	const bin: Record<string, string> = {};
	for (const command of commands) {
		const src = join(nativeCommandsDir, command);
		if (!existsSync(src)) {
			throw new Error(
				`test-only command "${command}" is missing from the native build output ` +
					`(${nativeCommandsDir}).\n${BUILD_INSTRUCTIONS}`,
			);
		}
		copyFileSync(src, join(binDir, command));
		bin[command] = `bin/${command}`;
	}
	writeFileSync(
		join(dir, "package.json"),
		`${JSON.stringify({ name: "agentos-test-commands", version: "0.0.0", bin }, null, 2)}\n`,
	);
	writeFileSync(
		join(dir, "agentos-package.json"),
		`${JSON.stringify({ name: "agentos-test-commands" }, null, 2)}\n`,
	);
	return { packageDir: dir };
}
