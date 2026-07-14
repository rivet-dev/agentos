import { describe, expect, test } from "vitest";
import type { Permissions } from "../src/runtime.js";
import { serializePermissionsForSidecar } from "../src/sidecar/permissions.js";

describe("serializePermissionsForSidecar", () => {
	test("preserves omission so the sidecar owns the default policy", () => {
		expect(serializePermissionsForSidecar()).toBeUndefined();
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

	test("preserves partial policies so the sidecar can apply domain defaults", () => {
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

	test("preserves omitted rule fields for sidecar wildcard defaults", () => {
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
			childProcess: undefined,
			process: undefined,
			env: undefined,
			binding: undefined,
		});
	});
});
