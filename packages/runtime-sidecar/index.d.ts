/**
 * Resolve the absolute path to the prebuilt `agentos-native-sidecar` binary for
 * the current platform.
 *
 * Resolution priority:
 *   1. `AGENTOS_SIDECAR_BIN` env var.
 *   2. A `agentos-native-sidecar` binary placed next to this package.
 *   3. The platform-specific `@rivet-dev/agentos-runtime-sidecar-<platform>` package.
 *
 * @throws if the platform is unsupported or no binary can be found.
 */
export function getSidecarPath(): string;
