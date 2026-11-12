import { createHash } from "node:crypto";
import { readFileSync, statSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import { resolveDefaultSoftware } from "../src/default-software.js";

const MAX_FIXTURE_BYTES = 16 * 1024 * 1024;
const permissions = { fs: "allow", network: "deny", childProcess: "allow", process: "allow", env: "allow", hostFunction: "deny" } as const;

describe("public source URL package policy reaches the native installer", () => {
	for (const mode of ["create", "install"] as const) {
		test(`raised source URL policy succeeds through ${mode}; omitted limit remains 4096`, async () => {
			const failures: unknown[] = [];
			const vms: AgentOs[] = [];
			let sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>> | undefined;
			let server: Server | undefined;
			let serverFailure: unknown;
			let requests = 0;
			try {
				// Reuse the same genuine staged package fixture as software-registry.
				// No hand-written archive format or package build occurs in this test.
				const packagePath = resolveDefaultSoftware().find((ref) => ref.packagePath.endsWith("/sed.aospkg"))?.packagePath;
				if (!packagePath) throw new Error("staged sed.aospkg fixture missing; build Core software staging first");
				const size = statSync(packagePath).size;
				expect(size).toBeGreaterThan(0); expect(size).toBeLessThanOrEqual(MAX_FIXTURE_BYTES);
				const bytes = readFileSync(packagePath);
				const digest = "sha256:" + createHash("sha256").update(bytes).digest("hex");
				const target = "/fixture.aospkg?padding=" + "x".repeat(5000);
				const recordServerFailure = (error: unknown) => {
					if (serverFailure === undefined) serverFailure = error;
					else console.error("additional local package fixture failure", error);
				};
				server = createServer({ maxHeaderSize: 16 * 1024 }, (request, response) => {
					requests++;
					response.on("error", recordServerFailure);
					request.on("error", recordServerFailure);
					if (requests > 4 || request.method !== "GET" || request.url !== target) {
						recordServerFailure(new Error("local package fixture received an unexpected or excess request"));
						response.writeHead(400, { Connection: "close", "Content-Length": "0" }); response.end(); return;
					}
					response.writeHead(200, { Connection: "close", "Content-Length": String(bytes.length), "Content-Type": "application/octet-stream" });
					response.end(bytes);
				});
				server.requestTimeout = 3000; server.headersTimeout = 3000; server.keepAliveTimeout = 1;
				server.on("error", recordServerFailure);
				await new Promise<void>((resolve, reject) => { server!.once("error", reject); server!.listen(0, "127.0.0.1", resolve); });
				const address = server.address() as AddressInfo;
				const url = `http://127.0.0.1:${address.port}${target}`;
				expect(Buffer.byteLength(url)).toBeGreaterThan(4096); expect(Buffer.byteLength(url)).toBeLessThan(8192);
				sidecar = await AgentOs.createSidecar();
				const base = { defaultSoftware: false, sidecar: { kind: "explicit" as const, handle: sidecar }, permissions };
				const local = { allowInsecureLocalHttp: true, connectTimeoutMs: 1000, downloadTimeoutMs: 3000, maxPackageBytes: MAX_FIXTURE_BYTES, maxRedirects: 0 };
				const secure = await AgentOs.create({ ...base, packageResolver: { ...local, allowInsecureLocalHttp: false, maxSourceUrlBytes: 8192 } }); vms.push(secure);
				await expect(secure.software.install({ type: "url", url, expectedDigest: digest })).rejects.toThrow(/plain HTTP packages require allow_insecure_local_http/);
				expect(requests).toBe(0);
				const vm = await AgentOs.create({ ...base, packageResolver: { ...local, maxSourceUrlBytes: 8192 },
					...(mode === "create" ? { software: [{ url, digest }] } : {}),
				});
				vms.push(vm);
				const installed = mode === "install"
					? await vm.software.install({ type: "url", url, expectedDigest: digest })
					: vm.software.installed()[0];
				expect(installed).toMatchObject({ packageName: "sed", digest, sizeBytes: BigInt(bytes.length) });
				expect(await vm.exists("/opt/agentos/pkgs/sed")).toBe(true);
				expect(requests).toBe(1); // real sidecar HTTP download, not a fake RPC.

				// Keep all options explicit except the source bound itself. Validation
				// must reject the same long source before another download or cache hit.
				const defaults = await AgentOs.create({ ...base, packageResolver: local }); vms.push(defaults);
				await expect(defaults.software.install({ type: "url", url, expectedDigest: digest })).rejects.toThrow(/max_source_url_bytes.*maximum=4096/);
				let createError: unknown;
				try { vms.push(await AgentOs.create({ ...base, packageResolver: local, software: [{ url, digest }] })); }
				catch (error) { createError = error; }
				expect(String(createError)).toMatch(/max_source_url_bytes.*maximum=4096/);
				expect(requests).toBe(1);
			} catch (error) { failures.push(error); }
			finally {
				for (const vm of vms.reverse()) { try { await vm.dispose(); } catch (error) { failures.push(error); } }
				if (sidecar) { try { await sidecar.dispose(); } catch (error) { failures.push(error); } }
				if (server?.listening) {
					try {
						const closed = new Promise<void>((resolve, reject) => server!.close((error) => error ? reject(error) : resolve()));
						server.closeAllConnections(); await closed;
					} catch (error) { failures.push(error); }
				}
				if (serverFailure !== undefined) failures.push(serverFailure);
			}
			if (failures.length) throw new AggregateError(failures, "public source URL package control or owning cleanup failed");
		}, 30_000);
	}
});
