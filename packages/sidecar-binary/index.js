"use strict";

// Platform-specific resolver for the prebuilt `agent-os-sidecar` binary. The
// binary itself ships inside one of the `@rivet-dev/agent-os-sidecar-<platform>`
// packages, declared as optionalDependencies so npm only installs the one
// matching the current `os`/`cpu`/`libc` at install time.
//
// Resolution priority:
//   1. `AGENT_OS_SIDECAR_BINARY` env var (absolute path override).
//   2. A `agent-os-sidecar` binary placed next to this package (dev builds).
//   3. The platform-specific `@rivet-dev/agent-os-sidecar-<platform>` package.

const { existsSync } = require("node:fs");
const { join, dirname } = require("node:path");

const BINARY_NAME = "agent-os-sidecar";

function getPlatformPackageName() {
	const { platform, arch } = process;
	switch (platform) {
		case "linux":
			if (arch === "x64") return "@rivet-dev/agent-os-sidecar-linux-x64-gnu";
			if (arch === "arm64") return "@rivet-dev/agent-os-sidecar-linux-arm64-gnu";
			break;
		default:
			break;
	}
	return null;
}

function getSidecarPath() {
	const override = process.env.AGENT_OS_SIDECAR_BINARY;
	if (override) {
		if (!existsSync(override)) {
			throw new Error(
				`AGENT_OS_SIDECAR_BINARY is set to ${override} but the file does not exist`,
			);
		}
		return override;
	}

	const localBinary = join(__dirname, BINARY_NAME);
	if (existsSync(localBinary)) {
		return localBinary;
	}

	const platformPkg = getPlatformPackageName();
	if (!platformPkg) {
		throw new Error(
			`@rivet-dev/agent-os-sidecar: unsupported platform ${process.platform}/${process.arch}. ` +
				"The Agent OS sidecar currently supports linux x64 and arm64. " +
				"Set AGENT_OS_SIDECAR_BINARY to a local agent-os-sidecar binary to override.",
		);
	}

	let pkgJsonPath;
	try {
		pkgJsonPath = require.resolve(`${platformPkg}/package.json`);
	} catch {
		throw new Error(
			`@rivet-dev/agent-os-sidecar: platform package ${platformPkg} is not installed.\n` +
				"This usually means the platform is unsupported or optionalDependencies were\n" +
				`skipped during install. Try: npm install --include=optional ${platformPkg}\n` +
				"Or set AGENT_OS_SIDECAR_BINARY to a local agent-os-sidecar binary.",
		);
	}

	return join(dirname(pkgJsonPath), BINARY_NAME);
}

module.exports = { getSidecarPath };
