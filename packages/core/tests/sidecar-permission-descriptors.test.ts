import { describe, expect, test } from "vitest";
import type { Permissions } from "../src/runtime-compat.js";
import { serializePermissionsForSidecar } from "../src/sidecar/permissions.js";

describe("serializePermissionsForSidecar", () => {
	test("omits the policy when permissions are omitted, so the sidecar default applies", () => {
		expect(serializePermissionsForSidecar()).toBeUndefined();
	});

	test("sends only the scopes the caller set", () => {
		expect(serializePermissionsForSidecar({ network: "allow" })).toEqual({
			fs: undefined,
			network: "allow",
			childProcess: undefined,
			process: undefined,
			env: undefined,
			binding: undefined,
		});
	});

	test("passes structured declarative policies through unchanged", () => {
		const permissions: Permissions = {
			fs: {
				default: "deny",
				rules: [
					{
						mode: "allow",
						operations: ["read"],
						paths: ["/workspace/**"],
					},
				],
			},
			network: {
				default: "deny",
				rules: [
					{
						mode: "allow",
						operations: ["dns"],
						patterns: ["dns://*.example.test"],
					},
				],
			},
			childProcess: "deny",
			process: {
				default: "deny",
				rules: [
					{
						mode: "allow",
						operations: ["inspect"],
						patterns: ["**"],
					},
				],
			},
			binding: {
				default: "deny",
				rules: [
					{
						mode: "allow",
						operations: ["invoke"],
						patterns: ["math:*"],
					},
				],
			},
		};

		expect(serializePermissionsForSidecar(permissions)).toEqual({
			fs: {
				default: "deny",
				rules: [
					{
						mode: "allow",
						operations: ["read"],
						paths: ["/workspace/**"],
					},
				],
			},
			network: {
				default: "deny",
				rules: [
					{
						mode: "allow",
						operations: ["dns"],
						patterns: ["dns://*.example.test"],
					},
				],
			},
			childProcess: "deny",
			process: {
				default: "deny",
				rules: [
					{
						mode: "allow",
						operations: ["inspect"],
						patterns: ["**"],
					},
				],
			},
			env: undefined,
			binding: {
				default: "deny",
				rules: [
					{
						mode: "allow",
						operations: ["invoke"],
						patterns: ["math:*"],
					},
				],
			},
		});
	});

	test("preserves partial policies so unspecified domains can be denied in Rust", () => {
		const permissions: Permissions = {
			env: {
				rules: [
					{
						mode: "allow",
						operations: ["*"],
						patterns: ["OPENAI_*", "PATH"],
					},
				],
			},
		};

		expect(serializePermissionsForSidecar(permissions)).toEqual({
			fs: undefined,
			network: undefined,
			childProcess: undefined,
			process: undefined,
			env: {
				rules: [
					{
						mode: "allow",
						operations: ["*"],
						patterns: ["OPENAI_*", "PATH"],
					},
				],
			},
			binding: undefined,
		});
	});

	test("expands omitted rule operations and resources to explicit wildcards", () => {
		const permissions: Permissions = {
			fs: {
				default: "deny",
				rules: [
					{
						mode: "allow",
						operations: ["read"],
					},
				],
			},
			network: {
				default: "deny",
				rules: [
					{
						mode: "allow",
						patterns: ["tcp://localhost:443"],
					},
				],
			},
		};

		expect(serializePermissionsForSidecar(permissions)).toEqual({
			fs: {
				default: "deny",
				rules: [
					{
						mode: "allow",
						operations: ["read"],
						paths: ["**"],
					},
				],
			},
			network: {
				default: "deny",
				rules: [
					{
						mode: "allow",
						operations: ["*"],
						patterns: ["tcp://localhost:443"],
					},
				],
			},
			childProcess: undefined,
			process: undefined,
			env: undefined,
			binding: undefined,
		});
	});
});
