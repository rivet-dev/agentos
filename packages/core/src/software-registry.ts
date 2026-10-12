import { readFileSync } from "node:fs";

/**
 * Registry software resolution.
 *
 * Each agentOS release publishes its software catalog as digest-addressed
 * `.aospkg` files next to `<registry>/<version>/software/manifest.json`. A
 * registry entry resolves to that release's URL and exact digest, then installs
 * through the ordinary URL source, so the sidecar still verifies the bytes.
 *
 * The manifest is fetched by this host process, not by the guest, so VM
 * network permissions do not apply to it.
 */

/** Root that holds `<version>/software/manifest.json` for each release. */
export const DEFAULT_SOFTWARE_REGISTRY_URL =
	"https://releases.rivet.dev/agent-os/";
/** Environment variable that overrides the default registry root. */
export const SOFTWARE_REGISTRY_URL_ENV = "AGENTOS_SOFTWARE_REGISTRY_URL";

const MANIFEST_FETCH_TIMEOUT_MS = 30_000;
const MANIFEST_MAX_BYTES = 8 * 1024 * 1024;
const MANIFEST_CACHE_LIMIT = 32;
// `latest` moves with every release; exact versions are immutable.
const LATEST_MANIFEST_TTL_MS = 5 * 60_000;

const PACKAGE_NAME_PATTERN = /^[a-z0-9][a-z0-9-]*$/;
const VERSION_PATTERN = /^(?:latest|\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$/;
const DIGEST_PATTERN = /^sha256:[0-9a-f]{64}$/;

/** Registry location and default release for name-based software. */
export interface SoftwareRegistryOptions {
	/**
	 * Root URL that holds `<version>/software/manifest.json`. Defaults to the
	 * `AGENTOS_SOFTWARE_REGISTRY_URL` environment variable, then
	 * `https://releases.rivet.dev/agent-os/`. Point it at a mirror or a
	 * self-hosted copy of the release layout.
	 */
	url?: string;
	/**
	 * Release to resolve entries that name no version. Defaults to the version
	 * of this `@rivet-dev/agentos-core` package, or `latest` for unreleased
	 * workspace and preview builds.
	 */
	version?: string;
}

/** A registry package resolved to its exact artifact. */
export interface ResolvedRegistrySoftware {
	name: string;
	/** Release whose manifest listed the package, such as `0.4.2` or `latest`. */
	version: string;
	/** Absolute `.aospkg` URL. */
	url: string;
	/** Exact `sha256:<64 lowercase hex>` content digest. */
	digest: string;
	sizeBytes: number;
	manifestUrl: string;
}

export interface ResolveRegistrySoftwareOptions {
	/** Release to read. Overrides `registry.version`. */
	version?: string;
	registry?: SoftwareRegistryOptions;
}

interface ManifestArtifact {
	name: string;
	digest: string;
	size: number;
	path: string;
}

interface CachedManifest {
	artifacts: Promise<ManifestArtifact[]>;
	expiresAt: number;
}

// Insertion-ordered so the oldest entry is evicted first. Bounded by
// MANIFEST_CACHE_LIMIT distinct registry/version pairs per process.
const manifestCache = new Map<string, CachedManifest>();

let cachedCoreVersion: string | undefined;

/** The version of this `@rivet-dev/agentos-core` package. */
export function coreVersion(): string {
	if (cachedCoreVersion === undefined) {
		// `package.json` sits one level above both `src/` and `dist/`, and npm
		// always ships it, so this resolves in the workspace and when installed.
		const raw = JSON.parse(
			readFileSync(new URL("../package.json", import.meta.url), "utf8"),
		) as { version?: unknown };
		if (typeof raw.version !== "string" || raw.version.length === 0) {
			throw new Error(
				"@rivet-dev/agentos-core package.json has no version; reinstall the package",
			);
		}
		cachedCoreVersion = raw.version;
	}
	return cachedCoreVersion;
}

/**
 * The release a registry entry resolves against when it names no version.
 *
 * A published client matches its own release so a given agentOS version gets
 * the software built with it. The committed workspace version `0.0.1` and
 * `0.0.0-*` preview builds never publish software, so they use `latest`.
 */
export function defaultRegistryVersion(
	version: string = coreVersion(),
): string {
	if (version === "0.0.1" || /^0\.0\.0(?:-|$)/.test(version)) {
		return "latest";
	}
	return version;
}

function registryRoot(options?: SoftwareRegistryOptions): URL {
	const raw =
		options?.url ??
		process.env[SOFTWARE_REGISTRY_URL_ENV] ??
		DEFAULT_SOFTWARE_REGISTRY_URL;
	let root: URL;
	try {
		root = new URL(raw.endsWith("/") ? raw : `${raw}/`);
	} catch {
		throw new Error(
			`invalid agentOS software registry URL ${JSON.stringify(raw)}; pass an absolute http(s) URL in softwareRegistry.url or ${SOFTWARE_REGISTRY_URL_ENV}`,
		);
	}
	assertSecureRegistryUrl(root, "registry URL");
	return root;
}

// The manifest supplies the digest every package is verified against, so it
// needs the same transport integrity as the packages: HTTPS, or plain HTTP only
// to a loopback mirror.
function isLoopbackHost(hostname: string): boolean {
	return (
		hostname === "localhost" ||
		hostname === "[::1]" ||
		/^127(?:\.\d{1,3}){3}$/.test(hostname)
	);
}

function assertSecureRegistryUrl(url: URL, label: string): void {
	if (url.protocol === "https:") return;
	if (url.protocol === "http:" && isLoopbackHost(url.hostname)) return;
	throw new Error(
		`agentOS software ${label} must use https, or plain http only on a loopback address, got ${url.href}`,
	);
}

function validateName(name: unknown): string {
	if (typeof name !== "string" || !PACKAGE_NAME_PATTERN.test(name)) {
		throw new Error(
			`invalid agentOS registry package name ${JSON.stringify(name)}; names are lowercase letters, digits, and hyphens, such as "git"`,
		);
	}
	return name;
}

function validateVersion(version: unknown): string {
	if (typeof version !== "string" || !VERSION_PATTERN.test(version)) {
		throw new Error(
			`invalid agentOS registry version ${JSON.stringify(version)}; pass a release version such as "0.4.2", or "latest"`,
		);
	}
	return version;
}

async function readBoundedBody(
	response: Response,
	manifestUrl: string,
): Promise<string> {
	const declared = Number(response.headers.get("content-length"));
	if (Number.isFinite(declared) && declared > MANIFEST_MAX_BYTES) {
		throw new Error(
			`agentOS software manifest ${manifestUrl} is ${declared} bytes, over the ${MANIFEST_MAX_BYTES}-byte limit`,
		);
	}
	if (!response.body) return "";
	const reader = response.body.getReader();
	const chunks: Uint8Array[] = [];
	let total = 0;
	for (;;) {
		const { done, value } = await reader.read();
		if (done) break;
		total += value.byteLength;
		if (total > MANIFEST_MAX_BYTES) {
			await reader.cancel();
			throw new Error(
				`agentOS software manifest ${manifestUrl} exceeds the ${MANIFEST_MAX_BYTES}-byte limit`,
			);
		}
		chunks.push(value);
	}
	return Buffer.concat(chunks).toString("utf8");
}

function parseManifest(text: string, manifestUrl: string): ManifestArtifact[] {
	let raw: unknown;
	try {
		raw = JSON.parse(text);
	} catch (error) {
		throw new Error(
			`agentOS software manifest ${manifestUrl} is not valid JSON: ${(error as Error).message}`,
		);
	}
	const record = raw as { schemaVersion?: unknown; artifacts?: unknown };
	if (record?.schemaVersion !== 1 || !Array.isArray(record.artifacts)) {
		throw new Error(
			`agentOS software manifest ${manifestUrl} is not a schemaVersion 1 manifest with an artifacts list`,
		);
	}
	return record.artifacts.map((entry, index) => {
		const artifact = entry as Partial<ManifestArtifact>;
		if (
			typeof artifact?.name !== "string" ||
			typeof artifact.path !== "string" ||
			typeof artifact.size !== "number" ||
			typeof artifact.digest !== "string" ||
			!DIGEST_PATTERN.test(artifact.digest)
		) {
			throw new Error(
				`agentOS software manifest ${manifestUrl} has a malformed artifact at index ${index}`,
			);
		}
		return {
			name: artifact.name,
			digest: artifact.digest,
			size: artifact.size,
			path: artifact.path,
		};
	});
}

async function fetchManifest(
	manifestUrl: string,
	version: string,
): Promise<ManifestArtifact[]> {
	let response: Response;
	try {
		response = await fetch(manifestUrl, {
			signal: AbortSignal.timeout(MANIFEST_FETCH_TIMEOUT_MS),
		});
	} catch (error) {
		throw new Error(
			`failed to fetch agentOS software manifest ${manifestUrl}: ${(error as Error).message}; check host network access or set softwareRegistry.url / ${SOFTWARE_REGISTRY_URL_ENV} to a reachable mirror`,
			{ cause: error },
		);
	}
	if (response.status === 404) {
		await response.body?.cancel();
		throw new Error(
			`agentOS software registry has no release "${version}": GET ${manifestUrl} returned 404; pass a published agentOS version or "latest"`,
		);
	}
	if (!response.ok) {
		await response.body?.cancel();
		throw new Error(
			`failed to fetch agentOS software manifest ${manifestUrl}: HTTP ${response.status} ${response.statusText}`.trimEnd(),
		);
	}
	if (response.url) {
		try {
			assertSecureRegistryUrl(new URL(response.url), "manifest redirect");
		} catch (error) {
			await response.body?.cancel();
			throw error;
		}
	}
	return parseManifest(
		await readBoundedBody(response, manifestUrl),
		manifestUrl,
	);
}

function loadManifest(
	manifestUrl: string,
	version: string,
): Promise<ManifestArtifact[]> {
	const now = Date.now();
	const cached = manifestCache.get(manifestUrl);
	if (cached && cached.expiresAt > now) return cached.artifacts;
	if (cached) manifestCache.delete(manifestUrl);

	const artifacts = fetchManifest(manifestUrl, version);
	const entry: CachedManifest = {
		artifacts,
		expiresAt:
			version === "latest"
				? now + LATEST_MANIFEST_TTL_MS
				: Number.POSITIVE_INFINITY,
	};
	while (manifestCache.size >= MANIFEST_CACHE_LIMIT) {
		const oldest = manifestCache.keys().next().value as string;
		manifestCache.delete(oldest);
	}
	manifestCache.set(manifestUrl, entry);
	// A failed fetch must not pin the error; the next call retries.
	artifacts.catch(() => {
		if (manifestCache.get(manifestUrl) === entry) {
			manifestCache.delete(manifestUrl);
		}
	});
	return artifacts;
}

function editDistance(left: string, right: string): number {
	let previous = Array.from({ length: right.length + 1 }, (_, index) => index);
	for (let i = 1; i <= left.length; i++) {
		const current = [i];
		for (let j = 1; j <= right.length; j++) {
			current[j] = Math.min(
				previous[j] + 1,
				current[j - 1] + 1,
				previous[j - 1] + (left[i - 1] === right[j - 1] ? 0 : 1),
			);
		}
		previous = current;
	}
	return previous[right.length];
}

function closeMatches(name: string, names: string[]): string[] {
	return names
		.map((candidate) => ({
			candidate,
			distance:
				candidate.includes(name) || name.includes(candidate)
					? 0
					: editDistance(name, candidate),
		}))
		.filter(({ distance }) => distance <= 2)
		.sort(
			(a, b) =>
				a.distance - b.distance || a.candidate.localeCompare(b.candidate),
		)
		.slice(0, 5)
		.map(({ candidate }) => candidate);
}

/**
 * Resolve a registry package name to the exact `.aospkg` URL and digest listed
 * in a release manifest. Manifests are cached per registry and version for the
 * life of the process; `latest` is refreshed every five minutes.
 */
export async function resolveRegistrySoftware(
	name: string,
	options: ResolveRegistrySoftwareOptions = {},
): Promise<ResolvedRegistrySoftware> {
	const packageName = validateName(name);
	const version = validateVersion(
		options.version ?? options.registry?.version ?? defaultRegistryVersion(),
	);
	const manifestUrl = new URL(
		`${version}/software/manifest.json`,
		registryRoot(options.registry),
	).href;
	const artifacts = await loadManifest(manifestUrl, version);
	const artifact = artifacts.find(
		(candidate) => candidate.name === packageName,
	);
	if (!artifact) {
		const suggestions = closeMatches(
			packageName,
			artifacts.map((candidate) => candidate.name),
		);
		throw new Error(
			`agentOS registry release "${version}" has no package "${packageName}" (${manifestUrl})` +
				(suggestions.length > 0
					? `; did you mean ${suggestions.map((s) => `"${s}"`).join(", ")}?`
					: "; browse https://agentos-sdk.dev/agentos/docs/software for available packages"),
		);
	}
	const url = new URL(artifact.path, manifestUrl);
	if (url.protocol !== "https:" && url.protocol !== "http:") {
		throw new Error(
			`agentOS software manifest ${manifestUrl} lists ${packageName} at a non-http(s) URL: ${url.href}`,
		);
	}
	return {
		name: packageName,
		version,
		url: url.href,
		digest: artifact.digest,
		sizeBytes: artifact.size,
		manifestUrl,
	};
}

/** @internal Test hook: forget cached manifests. */
export function clearSoftwareRegistryCache(): void {
	manifestCache.clear();
}

/** @internal Test hook: number of cached manifests. */
export function softwareRegistryCacheSize(): number {
	return manifestCache.size;
}
