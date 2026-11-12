import { randomUUID } from "node:crypto";
import { describe, expect, test } from "vitest";
import {
	issueHostedNodeCredential,
	issueHostedVmCredential,
} from "../../../examples/hosted/credentials.js";
import {
	destroyActor,
	engineRequest,
	invalidSignature,
	issueToken,
	localClient,
	newVm,
	rejectedNodeConnect,
	required,
	waitForExpiry,
} from "./hosted-local-fixture.js";

function credentialIssuer() {
	return {
		endpoint: required("AGENTOS_HOSTED_TEST_ENDPOINT"),
		namespace: required("AGENTOS_HOSTED_TEST_NAMESPACE"),
		poolName: required("AGENTOS_HOSTED_TEST_POOL"),
		adminToken: required("AGENTOS_HOSTED_TEST_ADMIN_TOKEN"),
	};
}

async function authRejection(operation: Promise<unknown>): Promise<void> {
	let timer: ReturnType<typeof setTimeout> | undefined;
	try {
		await Promise.race([
			operation,
			new Promise((_, reject) => {
				timer = setTimeout(
					() =>
						reject(
							Error(
								"ERR_HOSTED_AUTH_DEADLINE: rejection was not observed in 10s",
							),
						),
					10_000,
				);
			}),
		]);
	} catch (error) {
		// A timeout, transport failure, or missing actor is not proof of authentication.
		const details =
			error instanceof Error
				? `${error.message} ${JSON.stringify(error, Object.getOwnPropertyNames(error))}`
				: String(error);
		expect(details).toMatch(
			/forbidden|unauthoriz|invalid_token|token_expired|insufficient_permissions|permission_denied/i,
		);
		return;
	} finally {
		if (timer) clearTimeout(timer);
	}
	throw Error(
		"ERR_HOSTED_ACCESS_ACCEPTED: forbidden operation unexpectedly succeeded",
	);
}

// Mandatory acceptance gates use real control-plane-issued JWTs. REST,
// actor HTTP/WebSocket access and node registration are distinct boundaries.
describe.runIf(process.env.AGENTOS_HOSTED_TEST_REQUIRE_LOCAL === "1")(
	"native hosted authorization",
	() => {
		test("rejects a wrong admin token at the control-plane REST boundary", async () => {
			const result = await engineRequest(
				"/namespaces",
				"GET",
				undefined,
				required("AGENTOS_HOSTED_TEST_BAD_TOKEN"),
			);
			expect([401, 403]).toContain(result.status);
		});

		test("rejects wrong credentials for both HTTP and WebSocket actor actions", async () => {
			const client = localClient();
			const vm = newVm(client);
			const bad = localClient(required("AGENTOS_HOSTED_TEST_BAD_TOKEN"));
			let id: string | undefined;
			let connection: ReturnType<typeof vm.connect> | undefined;
			const errors: unknown[] = [];
			try {
				id = await vm.resolve();
				await vm.vm.status({});
				const denied = bad.getForId("agentOS", id);
				await authRejection(denied.vm.status({}));
				connection = denied.connect();
				await authRejection(connection.vm.status({}));
			} catch (error) {
				errors.push(error);
			}
			// Attempt every cleanup owner without replacing the primary failure.
			for (const cleanup of [
				async () => {
					if (connection !== undefined) await connection.dispose();
				},
				async () => {
					if (id !== undefined) await destroyActor(id);
				},
			]) {
				try {
					await cleanup();
				} catch (error) {
					errors.push(error);
				}
			}
			const clientResults = await Promise.allSettled([
				Promise.resolve().then(() => client.dispose()),
				Promise.resolve().then(() => bad.dispose()),
			]);
			for (const result of clientResults) {
				if (result.status === "rejected") errors.push(result.reason);
			}
			if (errors.length === 1) throw errors[0];
			if (errors.length > 1) {
				throw new AggregateError(
					errors,
					"hosted auth operation or cleanup failed",
				);
			}
		}, 90_000);

		test("real JWTs enforce actor scope, signature, expiry and delegation on HTTP and WebSocket", async () => {
			const owner = localClient();
			const actors = [newVm(owner), newVm(owner)];
			const ids: string[] = [];
			try {
				for (const actor of actors) {
					ids.push(await actor.resolve());
					await actor.vm.status({});
				}
				const grant = [
					{
						resource: "actor_gateway" as const,
						target: { id: ids[0] },
						operations: ["read" as const],
					},
				];
				const valid = await issueHostedVmCredential(
					credentialIssuer(),
					ids[0],
					"owned-user",
				);
				const expired = await issueToken(grant, 1);
				await waitForExpiry(expired.expiresTs);
				for (const [token, permitted] of [
					[valid.token, true],
					[invalidSignature(valid.token), false],
					[expired.token, false],
				] as const) {
					const client = localClient(token);
					try {
						const vm = client.getForId("agentOS", ids[0]);
						if (permitted) await vm.vm.status({});
						else await authRejection(vm.vm.status({}));
						const connection = vm.connect();
						try {
							if (permitted) await connection.vm.status({});
							else await authRejection(connection.vm.status({}));
						} finally {
							await connection.dispose();
						}
						if (permitted) {
							const other = client.getForId("agentOS", ids[1]);
							await authRejection(other.vm.status({}));
							const wrongScope = other.connect();
							try {
								await authRejection(wrongScope.vm.status({}));
							} finally {
								await wrongScope.dispose();
							}
							await authRejection(newVm(client).vm.status({}));
							const delegated = await engineRequest(
								"/auth/tokens",
								"POST",
								{
									namespace: required("AGENTOS_HOSTED_TEST_NAMESPACE"),
									duration: 60,
									grants: grant,
								},
								token,
							);
							expect([401, 403]).toContain(delegated.status);
						}
					} finally {
						await client.dispose();
					}
				}
			} finally {
				try {
					for (const id of ids) await destroyActor(id);
				} finally {
					await owner.dispose();
				}
			}
		}, 90_000);

		test("local bootstrap pool authority is namespace-scoped, not exact-pool-scoped", async () => {
			const nodeToken = required("AGENTOS_HOSTED_TEST_NODE_TOKEN");
			const namespace = required("AGENTOS_HOSTED_TEST_NAMESPACE");
			const pool = required("AGENTOS_HOSTED_TEST_POOL");
			const admin = required("AGENTOS_HOSTED_TEST_ADMIN_TOKEN");
			const ownPath = `/pools/${pool}?namespace=${namespace}`;
			const before = await engineRequest(ownPath, "GET", undefined, admin);
			expect(before.status).toBe(200);
			// Engine3.0.7 PUT replaces the full config, rather than patching it.
			// Prove authorization with the identical current config, including the
			// SDK's local bootstrap setting; preserve all eviction/metadata fields.
			const { name, ...unchangedConfig } = before.body.pool;
			expect(name).toBe(pool);
			const own = await engineRequest(
				ownPath,
				"PUT",
				unchangedConfig,
				nodeToken,
			);
			expect(own.status).toBe(200);
			expect(own.body.pool).toEqual(before.body.pool);
			const after = await engineRequest(ownPath, "GET", undefined, admin);
			expect(after.status).toBe(200);
			expect(after.body.pool).toEqual(before.body.pool);
			const otherNamespace = `scope-${randomUUID()}`;
			const created = await engineRequest(
				"/namespaces",
				"POST",
				{ name: otherNamespace },
				admin,
			);
			expect(created.status).toBe(200);
			const otherPoolPath = `/pools/${pool}?namespace=${otherNamespace}`;
			let poolCreated = false;
			try {
				const createdPool = await engineRequest(
					otherPoolPath,
					"PUT",
					{},
					admin,
				);
				expect(createdPool.status).toBe(200);
				poolCreated = true;
				const forbidden = await engineRequest(
					otherPoolPath,
					"PUT",
					{ drain_on_version_upgrade: true },
					nodeToken,
				);
				expect(forbidden.status).toBe(403);
				expect(forbidden.body).toMatchObject({
					group: "auth",
					code: "insufficient_permissions",
				});
			} finally {
				if (poolCreated) {
					const removed = await engineRequest(
						otherPoolPath,
						"DELETE",
						undefined,
						admin,
					);
					expect(removed.status).toBe(200);
				}
			}
		}, 30_000);

		test("node registration rejects a signed wrong-resource JWT, bad signature and real expiry", async () => {
			// Successful real node registration uses a separate JWT in the launcher.
			const node = await issueHostedNodeCredential(credentialIssuer());
			const expired = await issueToken(
				[{ resource: "node", target: "any", operations: ["create"] }],
				1,
			);
			const wrongScope = await issueToken([
				{ resource: "actor_gateway", target: "any", operations: ["read"] },
			]);
			await waitForExpiry(expired.expiresTs);
			for (const [token, expected] of [
				[invalidSignature(node.token), "auth.invalid_token"],
				[expired.token, "auth.token_expired"],
				[wrongScope.token, "auth.insufficient_permissions"],
			]) {
				const closed = await rejectedNodeConnect(token);
				expect(closed).toEqual({ code: 1008, reason: expected });
			}
		}, 90_000);
	},
);
