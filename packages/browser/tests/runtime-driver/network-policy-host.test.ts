import { describe, expect, it } from "vitest";
import {
	type NetworkAdapter,
	type Permissions,
	wrapNetworkAdapter,
} from "../../src/runtime.js";

// F-009 (sec-ts T3): the network policy callback must receive the parsed host
// and port, not just the url, so an operator's host-keyed SSRF deny rule
// (`req.host === "169.254.169.254"`) actually matches and the cloud-metadata
// host is unreachable. See:
//   /home/nathan/.agent/notes/security-review/FAILURES.md#F-009

const METADATA_HOST = "169.254.169.254";
const SECRET_BODY = "SECRET-IAM-CREDENTIALS";

function makeAdapter(): NetworkAdapter {
	// Stub host adapter that would happily return metadata secrets if reached.
	const response = {
		ok: true,
		status: 200,
		statusText: "OK",
		headers: {},
		body: SECRET_BODY,
		url: "",
		redirected: false,
	};
	return {
		async fetch() {
			return { ...response };
		},
		async dnsLookup() {
			return { address: "127.0.0.1", family: 4 };
		},
		async httpRequest() {
			const { redirected, ...rest } = response;
			return { ...rest };
		},
	};
}

describe("network policy callback receives parsed host and port not just url", () => {
	it("passes host and port from the URL to the policy callback", () => {
		const seen: Array<{ url?: string; host?: string; port?: number }> = [];
		const permissions: Permissions = {
			network: (req) => {
				seen.push(req);
				return true;
			},
		};
		const wrapped = wrapNetworkAdapter(makeAdapter(), permissions);

		return wrapped
			.fetch("https://example.com:8443/path")
			.then(() => {
				expect(seen).toHaveLength(1);
				expect(seen[0].host).toBe("example.com");
				expect(seen[0].port).toBe(8443);
				expect(seen[0].url).toBe("https://example.com:8443/path");
			});
	});

	it("derives the default port from the scheme when omitted", async () => {
		const seen: Array<{ url?: string; host?: string; port?: number }> = [];
		const permissions: Permissions = {
			network: (req) => {
				seen.push(req);
				return true;
			},
		};
		const wrapped = wrapNetworkAdapter(makeAdapter(), permissions);

		await wrapped.fetch("http://example.com/path");
		expect(seen[0].host).toBe("example.com");
		expect(seen[0].port).toBe(80);

		await wrapped.fetch("https://example.com/path");
		expect(seen[1].host).toBe("example.com");
		expect(seen[1].port).toBe(443);
	});

	it("denies a host-keyed deny rule against the cloud-metadata host (fetch)", async () => {
		const permissions: Permissions = {
			network: (req) => ({ allow: req.host !== METADATA_HOST }),
		};
		const wrapped = wrapNetworkAdapter(makeAdapter(), permissions);

		await expect(
			wrapped.fetch(`http://${METADATA_HOST}/latest/meta-data/iam/`),
		).rejects.toThrow(/EACCES/);
	});

	it("denies a host-keyed deny rule against the cloud-metadata host (httpRequest)", async () => {
		const permissions: Permissions = {
			network: (req) => ({ allow: req.host !== METADATA_HOST }),
		};
		const wrapped = wrapNetworkAdapter(makeAdapter(), permissions);

		await expect(
			wrapped.httpRequest(`http://${METADATA_HOST}/latest/meta-data/iam/`),
		).rejects.toThrow(/EACCES/);
	});
});
