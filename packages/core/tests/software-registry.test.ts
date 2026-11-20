import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import {
	afterAll,
	afterEach,
	beforeAll,
	describe,
	expect,
	it,
	vi,
} from "vitest";
import { AgentOs } from "../src/agent-os.js";
import { resolveDefaultSoftware } from "../src/default-software.js";
import {
	clearSoftwareRegistryCache,
	coreVersion,
	DEFAULT_SOFTWARE_REGISTRY_URL,
	defaultRegistryVersion,
	resolveRegistrySoftware,
	SOFTWARE_REGISTRY_URL_ENV,
	softwareRegistryCacheSize,
} from "../src/software-registry.js";

// A local registry with the release layout `<version>/software/manifest.json`.
// Unit tests never reach the network.

const DIGEST = `sha256:${"a".repeat(64)}`;

function manifest(names: string[], digest = DIGEST) {
	return {
		schemaVersion: 1,
		artifacts: names.map((name) => ({
			name,
			digest,
			size: 1,
			path: `packages/${name}/${digest.slice("sha256:".length)}.aospkg`,
		})),
	};
}

interface Registry {
	url: string;
	requests: string[];
	releases: Map<string, unknown>;
	failNext: number;
	packages: Map<string, Buffer>;
}

async function startRegistry(): Promise<{
	registry: Registry;
	server: Server;
}> {
	const registry: Registry = {
		url: "",
		requests: [],
		releases: new Map(),
		failNext: 0,
		packages: new Map(),
	};
	const server = createServer((request, response) => {
		const path = request.url ?? "/";
		registry.requests.push(path);
		const pkg = registry.packages.get(path);
		if (pkg) {
			response.end(pkg);
			return;
		}
		const match = path.match(/^\/agent-os\/([^/]+)\/software\/manifest\.json$/);
		if (!match) {
			response.statusCode = 404;
			response.end();
			return;
		}
		if (registry.failNext > 0) {
			registry.failNext -= 1;
			response.statusCode = 503;
			response.statusMessage = "Service Unavailable";
			response.end();
			return;
		}
		const release = registry.releases.get(match[1]);
		if (release === undefined) {
			response.statusCode = 404;
			response.end();
			return;
		}
		response.setHeader("content-type", "application/json");
		response.end(
			typeof release === "string" ? release : JSON.stringify(release),
		);
	});
	await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
	registry.url = `http://127.0.0.1:${(server.address() as AddressInfo).port}/agent-os/`;
	return { registry, server };
}

let registry: Registry;
let server: Server;

beforeAll(async () => {
	({ registry, server } = await startRegistry());
});

afterAll(async () => {
	await new Promise<void>((resolve) => server.close(() => resolve()));
});

afterEach(() => {
	clearSoftwareRegistryCache();
	registry.requests.length = 0;
	registry.releases.clear();
	registry.packages.clear();
	registry.failNext = 0;
	vi.restoreAllMocks();
	vi.unstubAllEnvs();
});

// The local server serves every release under `/agent-os/`.
function serve(version: string, body: unknown) {
	registry.releases.set(version, body);
}

describe("registry version default", () => {
	it("uses the client's own release", () => {
		expect(defaultRegistryVersion("0.4.2")).toBe("0.4.2");
		expect(defaultRegistryVersion("0.4.3-rc.1")).toBe("0.4.3-rc.1");
	});

	it("uses latest for unreleased workspace and preview builds", () => {
		expect(defaultRegistryVersion("0.0.1")).toBe("latest");
		expect(defaultRegistryVersion("0.0.0-my-branch.abc1234")).toBe("latest");
		// The committed workspace pin is 0.0.1, so this package resolves latest.
		expect(coreVersion()).toBe("0.0.1");
		expect(defaultRegistryVersion()).toBe("latest");
	});

	it("defaults to releases.rivet.dev", () => {
		expect(DEFAULT_SOFTWARE_REGISTRY_URL).toBe(
			"https://releases.rivet.dev/agent-os/",
		);
	});
});

describe("resolveRegistrySoftware", () => {
	it("resolves a name to an absolute digest-pinned URL", async () => {
		serve("latest", manifest(["git", "ripgrep"]));
		const resolved = await resolveRegistrySoftware("git", {
			registry: { url: registry.url },
		});
		expect(resolved).toEqual({
			name: "git",
			version: "latest",
			url: `${registry.url}latest/software/packages/git/${"a".repeat(64)}.aospkg`,
			digest: DIGEST,
			sizeBytes: 1,
			manifestUrl: `${registry.url}latest/software/manifest.json`,
		});
	});

	it("reads an explicit version, which wins over the registry default", async () => {
		serve("0.4.2", manifest(["git"]));
		serve("0.4.1", manifest(["git"]));
		const resolved = await resolveRegistrySoftware("git", {
			version: "0.4.2",
			registry: { url: registry.url, version: "0.4.1" },
		});
		expect(resolved.version).toBe("0.4.2");
		const pinned = await resolveRegistrySoftware("git", {
			registry: { url: registry.url, version: "0.4.1" },
		});
		expect(pinned.version).toBe("0.4.1");
		expect(registry.requests).toEqual([
			"/agent-os/0.4.2/software/manifest.json",
			"/agent-os/0.4.1/software/manifest.json",
		]);
	});

	it("honors the registry URL environment variable", async () => {
		serve("latest", manifest(["git"]));
		vi.stubEnv(SOFTWARE_REGISTRY_URL_ENV, registry.url.replace(/\/$/, ""));
		const resolved = await resolveRegistrySoftware("git");
		expect(resolved.manifestUrl).toBe(
			`${registry.url}latest/software/manifest.json`,
		);
	});

	it("caches manifests per version and refreshes latest", async () => {
		serve("latest", manifest(["git", "ripgrep"]));
		serve("0.4.2", manifest(["git"]));
		const options = { registry: { url: registry.url } };
		await Promise.all([
			resolveRegistrySoftware("git", options),
			resolveRegistrySoftware("ripgrep", options),
		]);
		await resolveRegistrySoftware("git", { ...options, version: "0.4.2" });
		await resolveRegistrySoftware("git", { ...options, version: "0.4.2" });
		expect(registry.requests).toHaveLength(2);

		// `latest` moves with releases, so it expires; exact versions do not.
		const now = Date.now();
		vi.spyOn(Date, "now").mockReturnValue(now + 6 * 60_000);
		await resolveRegistrySoftware("git", options);
		await resolveRegistrySoftware("git", { ...options, version: "0.4.2" });
		expect(registry.requests).toEqual([
			"/agent-os/latest/software/manifest.json",
			"/agent-os/0.4.2/software/manifest.json",
			"/agent-os/latest/software/manifest.json",
		]);
	});

	it("bounds the manifest cache", async () => {
		for (let patch = 0; patch < 40; patch++) {
			serve(`1.0.${patch}`, manifest(["git"]));
			await resolveRegistrySoftware("git", {
				version: `1.0.${patch}`,
				registry: { url: registry.url },
			});
		}
		expect(softwareRegistryCacheSize()).toBe(32);
	});

	it("does not cache a failed fetch", async () => {
		serve("latest", manifest(["git"]));
		registry.failNext = 1;
		const options = { registry: { url: registry.url } };
		await expect(resolveRegistrySoftware("git", options)).rejects.toThrow(
			`failed to fetch agentOS software manifest ${registry.url}latest/software/manifest.json: HTTP 503 Service Unavailable`,
		);
		await expect(resolveRegistrySoftware("git", options)).resolves.toEqual(
			expect.objectContaining({ name: "git" }),
		);
	});

	it("names close matches for an unknown package", async () => {
		serve("latest", manifest(["git", "gitleaks", "ripgrep"]));
		await expect(
			resolveRegistrySoftware("gti", { registry: { url: registry.url } }),
		).rejects.toThrow(
			`agentOS registry release "latest" has no package "gti" (${registry.url}latest/software/manifest.json); did you mean "git"?`,
		);
		await expect(
			resolveRegistrySoftware("zzzzzz", { registry: { url: registry.url } }),
		).rejects.toThrow("browse https://agentos-sdk.dev/agentos/docs/software");
	});

	it("reports a release that does not exist", async () => {
		await expect(
			resolveRegistrySoftware("git", {
				version: "9.9.9",
				registry: { url: registry.url },
			}),
		).rejects.toThrow(
			`agentOS software registry has no release "9.9.9": GET ${registry.url}9.9.9/software/manifest.json returned 404`,
		);
	});

	it("reports an unreachable registry with its URL", async () => {
		const closed = await startRegistry();
		const url = closed.registry.url;
		await new Promise<void>((resolve) => closed.server.close(() => resolve()));
		await expect(
			resolveRegistrySoftware("git", { registry: { url } }),
		).rejects.toThrow(
			`failed to fetch agentOS software manifest ${url}latest/software/manifest.json`,
		);
	});

	it("rejects malformed manifests, names, and versions", async () => {
		serve("latest", "{not json");
		serve("0.4.2", { schemaVersion: 2, artifacts: [] });
		serve("0.4.3", {
			schemaVersion: 1,
			artifacts: [{ name: "git", digest: "md5:x", size: 1, path: "git" }],
		});
		const registryOptions = { url: registry.url };
		await expect(
			resolveRegistrySoftware("git", { registry: registryOptions }),
		).rejects.toThrow("is not valid JSON");
		await expect(
			resolveRegistrySoftware("git", {
				version: "0.4.2",
				registry: registryOptions,
			}),
		).rejects.toThrow("is not a schemaVersion 1 manifest");
		await expect(
			resolveRegistrySoftware("git", {
				version: "0.4.3",
				registry: registryOptions,
			}),
		).rejects.toThrow("malformed artifact at index 0");
		await expect(
			resolveRegistrySoftware("../git", { registry: registryOptions }),
		).rejects.toThrow("invalid agentOS registry package name");
		await expect(
			resolveRegistrySoftware("git", {
				version: "../latest",
				registry: registryOptions,
			}),
		).rejects.toThrow("invalid agentOS registry version");
		for (const url of ["ftp://mirror/", "http://mirror.example.com/"]) {
			await expect(
				resolveRegistrySoftware("git", { registry: { url } }),
			).rejects.toThrow(
				"must use https, or plain http only on a loopback address",
			);
		}
		expect(registry.requests).toHaveLength(3);
	});
});

function createMockAgent() {
	const metadata = {
		package_id: "pkg-sha256:git",
		digest: DIGEST,
		size: 1n,
		package_name: "git",
		version: "2.0.0",
		commands: ["git"],
	};
	const client = {
		onEvent: () => () => {},
		onClose: () => () => {},
		installPackage: vi.fn(async () => ({
			package: metadata,
			projected_commands: [],
		})),
	};
	// Exercise the real install method without booting a native VM.
	const agent = Reflect.construct(AgentOs, [
		{},
		{},
		[],
		[],
		{},
		{},
		client,
		{ connectionId: "conn", sessionId: "session" },
		{ vmId: "vm" },
	]) as AgentOs;
	return { agent, client };
}

describe("software.install registry source", () => {
	it("forwards the resolved URL and digest to the sidecar", async () => {
		serve("0.4.2", manifest(["git"]));
		const { agent, client } = createMockAgent();
		Object.assign(agent, {
			_softwareRegistry: { url: registry.url },
			_packageResolver: { allowInsecureLocalHttp: true, maxRedirects: 2 },
		});
		const installed = await agent.software.install({
			type: "registry",
			name: "git",
			version: "0.4.2",
		});
		expect(installed.packageName).toBe("git");
		expect(client.installPackage).toHaveBeenCalledWith(
			{ connectionId: "conn", sessionId: "session" },
			{ vmId: "vm" },
			{
				source: {
					type: "url",
					url: `${registry.url}0.4.2/software/packages/git/${"a".repeat(64)}.aospkg`,
					expected_digest: DIGEST,
				},
				max_redirects: 2,
				allow_insecure_local_http: true,
			},
		);
	});

	it("does not reach the sidecar when resolution fails", async () => {
		serve("latest", manifest(["git"]));
		const { agent, client } = createMockAgent();
		Object.assign(agent, { _softwareRegistry: { url: registry.url } });
		await expect(
			agent.software.install({ type: "registry", name: "gitt" }),
		).rejects.toThrow('did you mean "git"?');
		expect(client.installPackage).not.toHaveBeenCalled();
	});
});

describe("create-time registry and URL entries", () => {
	it("fails before booting a VM when a registry name is unknown", async () => {
		serve("latest", manifest(["git"]));
		await expect(
			AgentOs.create({
				software: [{ registry: "gti" }],
				softwareRegistry: { url: registry.url },
			}),
		).rejects.toThrow('did you mean "git"?');
	});

	it("rejects malformed entries instead of dropping them", async () => {
		await expect(
			AgentOs.create({
				software: [
					{ registry: "git", version: 1 } as unknown as { registry: string },
				],
			}),
		).rejects.toThrow("non-string version");
		await expect(
			AgentOs.create({
				software: [
					{ url: "https://example.com/t.aospkg", digest: 1 } as unknown as {
						url: string;
					},
				],
			}),
		).rejects.toThrow("non-string digest");
		await expect(
			AgentOs.create({
				software: [{ name: "tool" } as unknown as { packagePath: string }],
			}),
		).rejects.toThrow("not a package ref");
	});
});

// These boot a real VM. The sidecar downloads the package from the local
// registry, so loopback HTTP is allowed explicitly.
// They check installation, not execution: PR CI stages only coreutils with its
// WASM binaries, so the vendored sed package projects no runnable command.
describe("registry install in a VM", () => {
	const sedPath = resolveDefaultSoftware().find((ref) =>
		ref.packagePath.endsWith("/sed.aospkg"),
	)?.packagePath;
	let vm: AgentOs | undefined;

	afterEach(async () => {
		await vm?.dispose();
		vm = undefined;
	});

	function serveSed(digestOverride?: string) {
		if (!sedPath) throw new Error("vendored sed.aospkg is missing; build core");
		const bytes = readFileSync(sedPath);
		const hex = createHash("sha256").update(bytes).digest("hex");
		serve("latest", {
			schemaVersion: 1,
			artifacts: [
				{
					name: "sed",
					digest: digestOverride ?? `sha256:${hex}`,
					size: bytes.length,
					path: `packages/sed/${hex}.aospkg`,
				},
			],
		});
		registry.packages.set(
			`/agent-os/latest/software/packages/sed/${hex}.aospkg`,
			bytes,
		);
		return `sha256:${hex}`;
	}

	const vmOptions = () => ({
		defaultSoftware: false,
		softwareRegistry: { url: registry.url },
		packageResolver: { allowInsecureLocalHttp: true },
	});

	it("boots with a registry package installed", async () => {
		const digest = serveSed();
		vm = await AgentOs.create({
			...vmOptions(),
			software: [{ registry: "sed" }],
		});
		expect(vm.software.installed()).toEqual([
			expect.objectContaining({ packageName: "sed", digest }),
		]);
	}, 60_000);

	it("installs a URL entry at create time", async () => {
		const digest = serveSed();
		vm = await AgentOs.create({
			...vmOptions(),
			software: [
				{
					url: `${registry.url}latest/software/packages/sed/${digest.slice(7)}.aospkg`,
					digest,
				},
			],
		});
		expect(vm.software.installed()).toEqual([
			expect.objectContaining({ packageName: "sed", digest }),
		]);
	}, 60_000);

	it("installs a registry package while running", async () => {
		const digest = serveSed();
		vm = await AgentOs.create(vmOptions());
		const installed = await vm.software.install({
			type: "registry",
			name: "sed",
		});
		expect(installed).toEqual(
			expect.objectContaining({ packageName: "sed", digest }),
		);
	}, 60_000);

	it("fails create when the downloaded bytes do not match the digest", async () => {
		serveSed(`sha256:${"0".repeat(64)}`);
		await expect(
			AgentOs.create({ ...vmOptions(), software: [{ registry: "sed" }] }),
		).rejects.toThrow(/digest mismatch|expected digest/i);
	}, 60_000);
});
