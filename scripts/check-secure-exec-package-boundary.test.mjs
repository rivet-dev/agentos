import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import assert from "node:assert/strict";
import test from "node:test";
import { checkSecureExecPackageBoundary } from "./check-secure-exec-package-boundary.mjs";

function withFixture(fn) {
	const root = mkdtempSync(join(tmpdir(), "secure-exec-boundary-"));
	try {
		mkdirSync(join(root, "packages"), { recursive: true });
		return fn(root);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
}

function writePackage(root, dirName, manifest, files = {}) {
	const packageDir = join(root, "packages", dirName);
	writePackageAt(root, packageDir, manifest, files);
}

function writePackageAt(root, packageDir, manifest, files = {}) {
	mkdirSync(packageDir, { recursive: true });
	writeFileSync(
		join(packageDir, "package.json"),
		`${JSON.stringify(manifest, null, "\t")}\n`,
	);

	for (const [relativePath, contents] of Object.entries(files)) {
		const path = join(packageDir, relativePath);
		mkdirSync(join(path, ".."), { recursive: true });
		writeFileSync(path, contents);
	}
}

test("accepts secure-exec packages without Agent OS edges", () => {
	withFixture((root) => {
		writePackage(
			root,
			"secure-exec-core",
			{
				name: "@secure-exec/core",
				exports: {
					".": {
						import: "./dist/index.js",
					},
				},
				dependencies: {
					"@rivetkit/bare-ts": "^0.6.2",
				},
			},
			{
				"src/index.ts": "export const ok = true;\n",
			},
		);

		assert.deepEqual(checkSecureExecPackageBoundary({ root }), []);
	});
});

test("accepts exported secure-exec source modules including aliases", () => {
	withFixture((root) => {
		writePackage(
			root,
			"secure-exec-core",
			{
				name: "@secure-exec/core",
				exports: {
					".": {
						types: "./dist/index.d.ts",
						import: "./dist/index.js",
					},
					"./protocol": {
						types: "./dist/generated-protocol.d.ts",
						import: "./dist/generated-protocol.js",
					},
					"./framing": {
						import: "./dist/framing.js",
					},
				},
			},
			{
				"src/index.ts": "export const ok = true;\n",
				"src/generated-protocol.ts": "export const protocol = true;\n",
				"src/framing.ts": "export const framing = true;\n",
			},
		);

		assert.deepEqual(checkSecureExecPackageBoundary({ root }), []);
	});
});

test("rejects unexported secure-exec source modules", () => {
	withFixture((root) => {
		writePackage(
			root,
			"secure-exec-core",
			{
				name: "@secure-exec/core",
				exports: {
					".": {
						import: "./dist/index.js",
					},
				},
			},
			{
				"src/index.ts": "export const ok = true;\n",
				"src/native-client.ts": "export const hidden = true;\n",
			},
		);

		assert.deepEqual(checkSecureExecPackageBoundary({ root }), [
			"@secure-exec/core must export packages/secure-exec-core/src/native-client.ts through package.json exports",
		]);
	});
});

test("rejects secure-exec core root sidecar-client re-exports", () => {
	withFixture((root) => {
		writePackage(
			root,
			"secure-exec-core",
			{
				name: "@secure-exec/core",
				exports: {
					".": {
						import: "./dist/index.js",
					},
					"./sidecar-client": {
						import: "./dist/sidecar-client.js",
					},
				},
			},
			{
				"src/index.ts": 'export * from "./sidecar-client.js";\n',
				"src/sidecar-client.ts": "export const client = true;\n",
			},
		);

		assert.deepEqual(checkSecureExecPackageBoundary({ root }), [
			"@secure-exec/core root export must not re-export ./sidecar-client; keep it on the explicit subpath (packages/secure-exec-core/src/index.ts)",
		]);
	});
});

test("ignores compatibility wrappers that intentionally depend on Agent OS", () => {
	withFixture((root) => {
		writePackage(root, "secure-exec", {
			name: "secure-exec",
			dependencies: {
				"@rivet-dev/agentos-core": "workspace:*",
			},
		});

		writePackage(root, "secure-exec-typescript", {
			name: "@secure-exec/typescript",
			dependencies: {
				"secure-exec": "workspace:*",
			},
		});

		assert.deepEqual(checkSecureExecPackageBoundary({ root }), []);
	});
});

test("ignores the temporary secure-exec sibling checkout", () => {
	withFixture((root) => {
		writePackageAt(root, join(root, "_secure-exec-sibling", "packages", "core"), {
			name: "@secure-exec/core",
		});

		assert.deepEqual(checkSecureExecPackageBoundary({ root }), []);
	});
});

test("does not require private secure-exec examples to export src modules", () => {
	withFixture((root) => {
		writePackage(
			root,
			"secure-exec-example",
			{
				name: "@secure-exec/example-ai-agent-type-check",
				private: true,
			},
			{
				"src/index.ts": "export const ok = true;\n",
			},
		);

		assert.deepEqual(checkSecureExecPackageBoundary({ root }), []);
	});
});

test("rejects Agent OS manifest dependencies", () => {
	withFixture((root) => {
		writePackage(root, "secure-exec-core", {
			name: "@secure-exec/core",
			dependencies: {
				"@rivet-dev/agentos-core": "workspace:*",
			},
		});

		assert.deepEqual(checkSecureExecPackageBoundary({ root }), [
			"@secure-exec/core must not depend on Agent OS package @rivet-dev/agentos-core (dependencies)",
		]);
	});
});

test("rejects Agent OS package descriptions and readmes", () => {
	withFixture((root) => {
		writePackage(
			root,
			"secure-exec-core",
			{
				name: "@secure-exec/core",
				description: "Runtime package for Agent OS",
				exports: {
					".": {
						import: "./dist/index.js",
					},
				},
			},
			{
				"src/index.ts": "export const ok = true;\n",
				"README.md": [
					"# @secure-exec/core",
					"",
					"Use this package with AgentOs from @rivet-dev/agentos-core.",
					"",
				].join("\n"),
			},
		);

		assert.deepEqual(checkSecureExecPackageBoundary({ root }), [
			"@secure-exec/core package description must not mention Agent OS surface Agent OS (packages/secure-exec-core/package.json)",
			"@secure-exec/core README must not mention Agent OS surface AgentOs (packages/secure-exec-core/README.md)",
			"@secure-exec/core README must not mention Agent OS surface @rivet-dev/agentos (packages/secure-exec-core/README.md)",
		]);
	});
});

test("audits secure-exec packages outside packages directory", () => {
	withFixture((root) => {
		writePackageAt(root, join(root, "registry/tool/sandbox"), {
			name: "@secure-exec/sandbox",
			dependencies: {
				"@rivet-dev/agentos-core": "workspace:*",
			},
		});

		assert.deepEqual(checkSecureExecPackageBoundary({ root }), [
			"@secure-exec/sandbox must not depend on Agent OS package @rivet-dev/agentos-core (dependencies)",
		]);
	});
});

test("rejects Agent OS source imports", () => {
	withFixture((root) => {
		writePackage(
			root,
			"secure-exec-core",
			{
				name: "@secure-exec/core",
				exports: {
					".": {
						import: "./dist/index.js",
					},
				},
			},
			{
				"src/index.ts":
					'import { createVm } from "@rivet-dev/agentos-core";\n',
			},
		);

		assert.deepEqual(checkSecureExecPackageBoundary({ root }), [
			"@secure-exec/core must not import Agent OS package @rivet-dev/agentos-core (packages/secure-exec-core/src/index.ts)",
		]);
	});
});

test("rejects Agent OS facade and binding symbols in secure-exec sources", () => {
	withFixture((root) => {
		writePackage(
			root,
			"secure-exec-core",
			{
				name: "@secure-exec/core",
				exports: {
					".": {
						import: "./dist/index.js",
					},
				},
			},
			{
				"src/index.ts":
					"export class AgentOs {}\nexport function registerToolkit(tool: Binding): BindingGroup { return tool; }\nexport const type = 'register_binding_group';\nexport const result = 'binding_group_registered';\n",
			},
		);

		assert.deepEqual(checkSecureExecPackageBoundary({ root }), [
			"@secure-exec/core must not expose Agent OS facade/binding symbol AgentOs (packages/secure-exec-core/src/index.ts)",
			"@secure-exec/core must not expose Agent OS facade/binding symbol Binding (packages/secure-exec-core/src/index.ts)",
			"@secure-exec/core must not expose Agent OS facade/binding symbol BindingGroup (packages/secure-exec-core/src/index.ts)",
			"@secure-exec/core must not expose Agent OS facade/binding symbol registerToolkit (packages/secure-exec-core/src/index.ts)",
			"@secure-exec/core must not expose Agent OS facade/binding symbol register_binding_group (packages/secure-exec-core/src/index.ts)",
			"@secure-exec/core must not expose Agent OS facade/binding symbol binding_group_registered (packages/secure-exec-core/src/index.ts)",
		]);
	});
});

test("rejects stale Agent OS base filesystem metadata in secure-exec core", () => {
	withFixture((root) => {
		writePackage(
			root,
			"secure-exec-core",
			{
				name: "@secure-exec/core",
				exports: {
					".": {
						import: "./dist/index.js",
					},
				},
			},
			{
				"src/index.ts": "export const ok = true;\n",
				"fixtures/base-filesystem.json": `${JSON.stringify(
					{
						source: {
							transforms: [
								"Preserve the captured user-level environment and filesystem layout as the AgentOs base layer",
							],
						},
						environment: {
							env: {
								HOSTNAME: "agent-os",
							},
						},
						filesystem: {
							entries: [
								{
									path: "/etc/hostname",
									content: "agent-os\n",
								},
							],
						},
					},
					null,
					"\t",
				)}\n`,
			},
		);

		assert.deepEqual(checkSecureExecPackageBoundary({ root }), [
			'@secure-exec/core base filesystem HOSTNAME must be secure-exec, got "agent-os"',
			"@secure-exec/core base filesystem /etc/hostname must contain secure-exec",
			"@secure-exec/core base filesystem metadata must not mention AgentOs",
		]);
	});
});

test("rejects relative imports into another package", () => {
	withFixture((root) => {
		writePackage(
			root,
			"secure-exec-core",
			{
				name: "@secure-exec/core",
				exports: {
					".": {
						import: "./dist/index.js",
					},
				},
			},
			{
				"src/index.ts": 'export { createVm } from "../../core/src/agent-os";\n',
			},
		);

		writePackage(root, "core", {
			name: "@rivet-dev/agentos-core",
		});

		assert.deepEqual(checkSecureExecPackageBoundary({ root }), [
			"@secure-exec/core must not import source from another package via ../../core/src/agent-os (packages/secure-exec-core/src/index.ts)",
		]);
	});
});
