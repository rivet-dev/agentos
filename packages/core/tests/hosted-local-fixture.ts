import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";
import type { TokenGrant } from "rivetkit/client";
import {
	type AgentOsClient,
	createAgentOsClient,
} from "../src/hosted-client.js";

export function required(name: string): string {
	const value = process.env[name];
	assert(value, `owned hosted fixture requires ${name}`);
	return value;
}

export function localClient(
	token = required("AGENTOS_HOSTED_TEST_TOKEN"),
): AgentOsClient {
	const endpoint = new URL(required("AGENTOS_HOSTED_TEST_ENDPOINT"));
	assert(endpoint.protocol === "http:" && endpoint.hostname === "127.0.0.1");
	return createAgentOsClient({
		endpoint: endpoint.href,
		namespace: required("AGENTOS_HOSTED_TEST_NAMESPACE"),
		poolName: required("AGENTOS_HOSTED_TEST_POOL"),
		token,
		encoding: "bare",
		disableMetadataLookup: true,
	});
}

export function softwareSources(): { url: string; digest: string }[] {
	const value = required("AGENTOS_HOSTED_TEST_SOFTWARE_JSON");
	assert(
		Buffer.byteLength(value) <= 64 * 1024,
		"fixture software JSON exceeds 64KiB",
	);
	const sources: unknown = JSON.parse(value);
	assert(Array.isArray(sources) && sources.length > 0 && sources.length <= 16);
	return sources.map((source: unknown) => {
		assert(
			source !== null &&
				typeof source === "object" &&
				"url" in source &&
				"digest" in source,
		);
		assert(typeof source.url === "string" && typeof source.digest === "string");
		assert.match(source.digest, /^sha256:[0-9a-f]{64}$/);
		const url = new URL(source.url);
		assert(url.protocol === "http:" && url.hostname === "127.0.0.1");
		return { url: url.href, digest: source.digest };
	});
}

export function newVm(client: AgentOsClient) {
	return client.agentOS.getOrCreate(["owned-local", randomUUID()], {
		createWithInput: {
			config: {
				software: softwareSources(),
				filesystem: {
					mounts: [
						{
							path: "/workspace",
							backend: { type: "actor-sqlite" },
							readOnly: false,
						},
					],
				},
			},
		},
	});
}

export async function eventually<T>(
	read: () => Promise<T>,
	ready: (value: T) => boolean,
	timeoutMs = 10_000,
): Promise<T> {
	const end = performance.now() + timeoutMs;
	let latest: T;
	for (let attempt = 0; attempt < 200 && performance.now() < end; attempt++) {
		latest = await read();
		if (ready(latest)) return latest;
		await delay(50);
	}
	throw Error(
		`ERR_HOSTED_FIXTURE_DEADLINE: predicate did not settle in ${timeoutMs}ms`,
	);
}

export async function engineRequest(
	path: string,
	method = "GET",
	body?: unknown,
	token = required("AGENTOS_HOSTED_TEST_TOKEN"),
) {
	const endpoint = new URL(required("AGENTOS_HOSTED_TEST_ENDPOINT"));
	assert(endpoint.hostname === "127.0.0.1" && endpoint.protocol === "http:");
	const result = await fetch(new URL(path, endpoint), {
		method,
		redirect: "error",
		signal: AbortSignal.timeout(5_000),
		headers: {
			authorization: `Bearer ${token}`,
			"content-type": "application/json",
		},
		body: body === undefined ? undefined : JSON.stringify(body),
	});
	const length = Number(result.headers.get("content-length") ?? "0");
	assert(length <= 1024 * 1024, "engine fixture response exceeds 1MiB");
	const reader = result.body?.getReader();
	const chunks: Uint8Array[] = [];
	let size = 0;
	if (reader)
		while (true) {
			const next = await reader.read();
			if (next.done) break;
			size += next.value.length;
			if (size > 1024 * 1024) {
				await reader.cancel();
				throw Error("engine fixture response exceeds 1MiB");
			}
			chunks.push(next.value);
		}
	const text = Buffer.concat(chunks).toString("utf8");
	return {
		status: result.status,
		ok: result.ok,
		body: text ? JSON.parse(text) : null,
	};
}

export function actorPath(id: string, suffix = ""): string {
	return `/actors/${encodeURIComponent(id)}${suffix}?namespace=${encodeURIComponent(required("AGENTOS_HOSTED_TEST_NAMESPACE"))}`;
}

export async function destroyActor(id: string): Promise<void> {
	const result = await engineRequest(actorPath(id), "DELETE");
	assert(result.ok, `owned actor destroy failed: ${result.status}`);
}

export async function issueToken(grants: TokenGrant[], duration = 300) {
	const result = await engineRequest(
		"/auth/tokens",
		"POST",
		{
			namespace: required("AGENTOS_HOSTED_TEST_NAMESPACE"),
			duration,
			subject: "owned-hosted-control",
			grants,
		},
		required("AGENTOS_HOSTED_TEST_ADMIN_TOKEN"),
	);
	assert(result.ok, `owned JWT issuance failed: ${result.status}`);
	assert(
		typeof result.body.token === "string" &&
			Number.isSafeInteger(result.body.expires_ts),
	);
	return {
		token: result.body.token as string,
		expiresTs: result.body.expires_ts as number,
	};
}

export function invalidSignature(token: string): string {
	const parts = token.split(".");
	assert(parts.length >= 3, "owned fixture expected a signed Rivet JWT");
	const signature = Buffer.from(parts.at(-1)!, "base64url");
	assert(signature.length > 0);
	signature[0] ^= 1;
	parts[parts.length - 1] = signature.toString("base64url");
	return parts.join(".");
}

export async function waitForExpiry(expiresTs: number): Promise<void> {
	// Rivet3.0.7 validates authorization until exp + PROTOCOL_CLOCK_SKEW(30s).
	// Wait beyond that real signed deadline, without modifying token claims.
	const milliseconds = Math.max(0, expiresTs + 30_000 - Date.now() + 1_100);
	assert(
		milliseconds <= 35_000,
		"expiry control must use a one-second real token plus published30s clock skew",
	);
	await delay(milliseconds);
}

export async function rejectedNodeConnect(token: string) {
	const url = new URL(
		"/envoys/connect",
		required("AGENTOS_HOSTED_TEST_ENDPOINT"),
	);
	assert(url.hostname === "127.0.0.1" && url.protocol === "http:");
	url.protocol = "ws:";
	// Exact published3.0.7 envoy schema is v8. Authentication is checked
	// before any connection record or protocol session is allocated.
	url.search = new URLSearchParams({
		protocol_version: "8",
		envoy_key: randomUUID(),
		version: "1",
		namespace: required("AGENTOS_HOSTED_TEST_NAMESPACE"),
		pool_name: required("AGENTOS_HOSTED_TEST_POOL"),
	}).toString();
	const socket = new WebSocket(url, [
		"rivet-owned-auth-control",
		`rivet_token.${token}`,
	]);
	try {
		return await new Promise<{ code: number; reason: string }>(
			(accept, reject) => {
				const timer = setTimeout(
					() =>
						reject(
							Error("ERR_HOSTED_AUTH_DEADLINE: node rejection exceeded5s"),
						),
					5_000,
				);
				socket.addEventListener(
					"message",
					() => {
						clearTimeout(timer);
						reject(
							Error(
								"ERR_HOSTED_ACCESS_ACCEPTED: rejected node received protocol data",
							),
						);
					},
					{ once: true },
				);
				socket.addEventListener(
					"close",
					(event) => {
						clearTimeout(timer);
						accept({ code: event.code, reason: event.reason });
					},
					{ once: true },
				);
				socket.addEventListener(
					"error",
					() => {
						clearTimeout(timer);
						reject(
							Error(
								"owned node connection transport failed before typed rejection",
							),
						);
					},
					{ once: true },
				);
			},
		);
	} finally {
		socket.close();
	}
}
