/**
 * Resolve the absolute path to the prebuilt `agentos-sidecar` binary for the
 * current platform.
 *
 * Resolution priority:
 *   1. `AGENTOS_SIDECAR_BIN` env var (absolute path override).
 *   2. The platform-specific `@rivet-dev/agentos-sidecar-<platform>` package.
 *
 * @throws if the platform is unsupported or no binary can be found.
 */
export function getSidecarPath(): string;
