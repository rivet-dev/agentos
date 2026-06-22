// Platform-specific resolver for the prebuilt agent-os actor plugin cdylib
// (`libagentos_actor_plugin.{so,dylib,dll}`). Mirrors the sidecar-binary
// resolver (`@rivet-dev/agentos-sidecar`): the library ships inside one of the
// `@rivet-dev/agentos-plugin-<platform>` packages, declared as
// optionalDependencies so npm installs only the one matching the current
// `os`/`cpu`/`libc` at install time (spec phase 6).
//
// Resolution priority:
//   1. `AGENTOS_PLUGIN_BIN` env var (absolute path override).
//   2. A cargo build output under the repo `target/{release,debug}/` (dev).
//   3. The platform-specific `@rivet-dev/agentos-plugin-<platform>` package.

import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const PLUGIN_BIN_ENV = "AGENTOS_PLUGIN_BIN";

/** The cdylib filename for the current platform. */
function libraryName(): string {
	switch (process.platform) {
		case "darwin":
			return "libagentos_actor_plugin.dylib";
		case "win32":
			return "agentos_actor_plugin.dll";
		default:
			return "libagentos_actor_plugin.so";
	}
}

function getPlatformPackageName(): string | null {
	const { platform, arch } = process;
	switch (platform) {
		case "linux":
			if (arch === "x64") return "@rivet-dev/agentos-plugin-linux-x64-gnu";
			if (arch === "arm64") return "@rivet-dev/agentos-plugin-linux-arm64-gnu";
			break;
		default:
			break;
	}
	return null;
}

/**
 * Resolve the absolute path to the agent-os actor plugin cdylib. RivetKit
 * `dlopen`s this path through the generic native-plugin ABI.
 */
export function getPluginPath(): string {
	const override = process.env[PLUGIN_BIN_ENV];
	if (override) {
		if (!existsSync(override)) {
			throw new Error(
				`${PLUGIN_BIN_ENV} is set to ${override} but the file does not exist`,
			);
		}
		return override;
	}

	const lib = libraryName();

	// Dev: a cargo build output under the repo `target/`. `import.meta.url` is
	// `packages/agentos/dist/plugin-binary.js`, so the repo root is three levels up.
	const here = dirname(fileURLToPath(import.meta.url));
	for (const profile of ["release", "debug"]) {
		const candidate = join(here, "..", "..", "..", "target", profile, lib);
		if (existsSync(candidate)) {
			return candidate;
		}
	}

	// Prod: the platform-specific package.
	const platformPkg = getPlatformPackageName();
	if (!platformPkg) {
		throw new Error(
			`@rivet-dev/agentos: unsupported platform ${process.platform}/${process.arch}. ` +
				"The Agent OS actor plugin currently supports linux x64 and arm64. " +
				`Set ${PLUGIN_BIN_ENV} to a local cdylib to override.`,
		);
	}

	const require = createRequire(import.meta.url);
	let pkgJsonPath: string;
	try {
		pkgJsonPath = require.resolve(`${platformPkg}/package.json`);
	} catch {
		throw new Error(
			`@rivet-dev/agentos: platform package ${platformPkg} is not installed.\n` +
				"This usually means the platform is unsupported or optionalDependencies\n" +
				`were skipped during install. Try: npm install --include=optional ${platformPkg}\n` +
				`Or set ${PLUGIN_BIN_ENV} to a local cdylib, or build it with\n` +
				"`cargo build -p agentos-actor-plugin`.",
		);
	}

	return join(dirname(pkgJsonPath), lib);
}
