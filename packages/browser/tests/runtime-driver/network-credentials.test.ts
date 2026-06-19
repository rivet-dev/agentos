import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createBrowserNetworkAdapter } from "../../src/driver.js";

// F-008 (sec-ts T1): the browser network adapter forwards guest requests to the
// host `fetch`. Without an explicit credentials policy, the request defaults to
// `same-origin` and rides the embedding page's ambient cookies / HTTP-auth,
// enabling credential exfil / CSRF from untrusted guest code. The adapter must
// set `credentials: 'omit'` on every host fetch. See:
//   /home/nathan/.agent/notes/security-review/FAILURES.md#F-008

function makeResponse(): Response {
	return {
		ok: true,
		status: 200,
		statusText: "OK",
		url: "https://example.com/",
		redirected: false,
		headers: {
			get: () => "text/plain",
			forEach: () => {},
		},
		text: async () => "body",
		arrayBuffer: async () => new ArrayBuffer(0),
	} as unknown as Response;
}

describe("browser fetch adapter omits ambient credentials and does not leak host-origin cookies", () => {
	let fetchSpy: ReturnType<typeof vi.fn>;
	const originalFetch = globalThis.fetch;

	beforeEach(() => {
		fetchSpy = vi.fn(async () => makeResponse());
		globalThis.fetch = fetchSpy as unknown as typeof fetch;
	});

	afterEach(() => {
		globalThis.fetch = originalFetch;
	});

	it("fetch() forwards credentials: 'omit' to the host fetch", async () => {
		const adapter = createBrowserNetworkAdapter();
		await adapter.fetch("https://example.com/", { method: "GET" });

		expect(fetchSpy).toHaveBeenCalledTimes(1);
		const init = fetchSpy.mock.calls[0][1] as RequestInit;
		expect(init.credentials).toBe("omit");
	});

	it("httpRequest() forwards credentials: 'omit' to the host fetch", async () => {
		const adapter = createBrowserNetworkAdapter();
		await adapter.httpRequest("https://example.com/", { method: "GET" });

		expect(fetchSpy).toHaveBeenCalledTimes(1);
		const init = fetchSpy.mock.calls[0][1] as RequestInit;
		expect(init.credentials).toBe("omit");
	});

	it("credentials policy is omit regardless of guest-supplied headers", async () => {
		const adapter = createBrowserNetworkAdapter();
		await adapter.fetch("https://example.com/", {
			method: "POST",
			headers: { cookie: "session=guest-set" },
			body: "x",
		});

		const init = fetchSpy.mock.calls[0][1] as RequestInit;
		expect(init.credentials).toBe("omit");
	});
});
