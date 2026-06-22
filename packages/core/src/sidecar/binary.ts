import { existsSync } from "node:fs";
import { createRequire } from "node:module";

interface SidecarBinaryModule {
	getSidecarPath(): string;
}

/**
 * Resolve the prebuilt sidecar binary for a published (non-repo) install.
 *
 * Honors `AGENTOS_SIDECAR_BIN` as an absolute-path override, otherwise
 * resolves the platform-specific binary shipped by the
 * `@rivet-dev/agentos-sidecar` package. In-repo developer builds use the local
 * cargo build path instead and never reach this function.
 */
export function resolvePublishedSidecarBinary(): string {
	const override = process.env.AGENTOS_SIDECAR_BIN;
	if (override) {
		if (!existsSync(override)) {
			throw new Error(
				`AGENTOS_SIDECAR_BIN is set to ${override} but the file does not exist`,
			);
		}
		return override;
	}

	const require = createRequire(import.meta.url);
	let mod: SidecarBinaryModule;
	try {
		mod = require("@rivet-dev/agentos-sidecar") as SidecarBinaryModule;
	} catch (error) {
		throw new Error(
			"failed to resolve the Agent OS sidecar binary: the @rivet-dev/agentos-sidecar " +
				"package is not installed. Install it, or set AGENTOS_SIDECAR_BIN to a local " +
				`agentos-sidecar binary. (${(error as Error).message})`,
		);
	}
	return mod.getSidecarPath();
}
