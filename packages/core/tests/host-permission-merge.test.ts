import { describe, expect, test } from "vitest";
import { resolveHostPermissions } from "../src/agent-os.js";
import type { Permissions } from "../src/runtime-compat.js";
import { serializePermissionsForSidecar } from "../src/sidecar/permissions.js";

// The sidecar denies every scope the wire policy omits, so anything
// `resolveHostPermissions` leaves out is a silent over-denial rather than the
// documented baseline. See docs/content/docs/permissions.mdx.
describe("resolveHostPermissions", () => {
	test("keeps the baseline for a VM created without a policy", () => {
		expect(resolveHostPermissions(undefined)).toEqual({
			fs: "allow",
			network: "allow",
			childProcess: "allow",
			process: "allow",
			env: "allow",
			binding: "allow",
		});
	});

	test("merges a partial policy over the baseline instead of replacing it", () => {
		expect(resolveHostPermissions({ network: "allow" })).toEqual({
			fs: "allow",
			network: "allow",
			childProcess: "allow",
			process: "allow",
			env: "allow",
			binding: "allow",
		});
	});

	test("keeps the execution essentials when only a network rule set is given", () => {
		const network: Permissions["network"] = {
			default: "deny",
			rules: [
				{ mode: "allow", operations: ["fetch"], patterns: ["example.com"] },
			],
		};

		expect(resolveHostPermissions({ network })).toEqual({
			fs: "allow",
			network,
			childProcess: "allow",
			process: "allow",
			env: "allow",
			binding: "allow",
		});
	});

	test("keeps the binding auto-grant alongside an explicit policy", () => {
		expect(
			resolveHostPermissions({ fs: "allow", childProcess: "allow" }).binding,
		).toBe("allow");
	});

	test("lets an explicit scope win over the baseline", () => {
		expect(resolveHostPermissions({ fs: "deny", network: "deny" })).toEqual({
			fs: "deny",
			network: "deny",
			childProcess: "allow",
			process: "allow",
			env: "allow",
			binding: "allow",
		});
	});

	test("preserves an explicit binding deny", () => {
		expect(resolveHostPermissions({ binding: "deny" }).binding).toBe("deny");
	});

	test("preserves an explicit binding rule set", () => {
		const binding: Permissions["binding"] = {
			default: "deny",
			rules: [
				{ mode: "allow", operations: ["invoke"], patterns: ["math:add"] },
			],
		};

		expect(resolveHostPermissions({ binding }).binding).toEqual(binding);
	});

	test("serializes every scope so the sidecar never sees an absent scope", () => {
		const serialized = serializePermissionsForSidecar(
			resolveHostPermissions({ network: "allow" }),
		);

		expect(serialized).toEqual({
			fs: "allow",
			network: "allow",
			childProcess: "allow",
			process: "allow",
			env: "allow",
			binding: "allow",
		});
	});
});
