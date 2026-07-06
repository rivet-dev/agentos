/**
 * Agent metadata for an agent package descriptor.
 */
export interface PackageAgentDescriptor {
	/** `bin/` command that speaks ACP over stdio. */
	acpEntrypoint: string;
	/** Static environment variables for the agent process. */
	env?: Record<string, string>;
	/** Optional extra launch arguments. */
	launchArgs?: string[];
	/** Optional snapshot flag. */
	snapshot?: boolean;
}

/**
 * Read-only file content contributed by a package.
 *
 * `source` is absolute or relative to the package directory. `target` is the
 * guest path where the sidecar mounts it read-only.
 */
export interface PackageProvidesFileDescriptor {
	source: string;
	target: string;
}

/**
 * Defaults contributed by a package when it is configured in a VM.
 */
export interface PackageProvidesDescriptor {
	/** VM environment defaults. Existing caller/base env values win. */
	env?: Record<string, string>;
	/** Host directories to expose as read-only guest file layers. */
	files?: PackageProvidesFileDescriptor[];
}

/**
 * Pure JSON manifest read by the sidecar from `<package dir>/agentos-package.json`.
 *
 * Commands come from `bin/`; version lives in this manifest.
 */
export interface AgentosPackageManifest {
	/** Short package name (e.g., "jq", "git", "claude"). */
	name: string;
	/** Package version. */
	version: string;
	/** Present only for agent packages. */
	agent?: PackageAgentDescriptor;
	/** Optional VM environment defaults and read-only file layers. */
	provides?: PackageProvidesDescriptor;
}

/** Runtime package reference passed by registry packages during the JSON manifest migration. */
export type PackageRef = string;

/** Client-facing software reference: points at a self-contained package tar.
 *  Extensible — future fields can be added without breaking callers. */
export interface SoftwarePackageRef {
	/** Absolute path to the self-contained package tar. */
	packageTar: string;
	/** @deprecated Directory refs are accepted only for local transition fixtures. */
	packageDir?: string;
}

/**
 * Descriptor for a registry package (software or agent).
 *
 * Each @agentos-software/* package default-exports a plain object literal
 * satisfying this type. Commands are derived by the sidecar from the package's
 * package's `bin/` directory, so no per-command metadata lives here.
 */
export interface PackageDescriptor {
	/** Short package name (e.g., "jq", "git", "claude"). */
	name: string;
	/** Absolute path to the self-contained package directory. */
	dir: string;
	/** Present only for agent packages. */
	agent?: PackageAgentDescriptor;
	/** Optional VM environment defaults and read-only file layers. */
	provides?: PackageProvidesDescriptor;
}

/**
 * Permission tier for WASM command execution.
 * Shared runtime permission tiers for registry command metadata.
 *
 * - full: spawn processes, network I/O, file read/write
 * - read-write: file read/write, no network or process spawning
 * - read-only: file read-only, no writes, no spawn, no network
 * - isolated: restricted to cwd subtree reads only
 */
export type PermissionTier = "full" | "read-write" | "read-only" | "isolated";

/**
 * Descriptor for a single command within a WASM command package.
 */
export interface WasmCommandEntry {
	/** Command name as invoked (e.g., "grep", "egrep"). */
	name: string;
	/** Default permission tier for this command. */
	permissionTier: PermissionTier;
	/** If set, this command is an alias for another command in the same package. */
	aliasOf?: string;
}

/**
 * Descriptor for a WASM command package.
 * Each @agentos-software/* software package exports a default value satisfying this type.
 */
export interface WasmCommandPackage {
	/** Package name without scope (e.g., "coreutils", "grep"). */
	name: string;
	/** Apt/Debian equivalent package name. */
	aptName: string;
	/** Human-readable description. */
	description: string;
	/** Build source: "rust" or "c". */
	source: "rust" | "c";
	/** Commands provided by this package. */
	commands: WasmCommandEntry[];
	/** Absolute path to the directory containing WASM command binaries. */
	readonly commandDir: string;
}

/**
 * Descriptor for a meta-package that aggregates other WASM command packages.
 */
export interface WasmMetaPackage {
	/** Package name without scope. */
	name: string;
	/** Human-readable description. */
	description: string;
	/** Package names (without scope) included in this meta-package. */
	includes: string[];
}
