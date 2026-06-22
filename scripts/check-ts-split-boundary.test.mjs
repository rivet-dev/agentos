import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { auditTsSplitBoundary } from "./check-ts-split-boundary.mjs";

function withFixture(fn) {
	const root = mkdtempSync(join(tmpdir(), "ts-split-boundary-"));
	try {
		return fn(root);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
}

function write(root, rel, contents) {
	const path = join(root, rel);
	mkdirSync(join(path, ".."), { recursive: true });
	writeFileSync(path, contents);
}

function writeJson(root, rel, value) {
	write(root, rel, `${JSON.stringify(value, null, "\t")}\n`);
}

function seedReadyFixture(root) {
	const agentOsRoot = join(root, "agent-os");
	const secureExecRoot = join(root, "secure-exec");
	writeJson(secureExecRoot, "packages/core/package.json", {
		name: "@secure-exec/core",
		exports: {
			"./protocol": "./dist/generated-protocol.js",
			"./native-client": "./dist/native-client.js",
			"./sidecar-client": "./dist/sidecar-client.js",
			"./protocol-frames": "./dist/protocol-frames.js",
		},
		dependencies: {
			"@secure-exec/sidecar": "workspace:*",
			"@rivetkit/bare-ts": "^0.6.2",
		},
	});
	write(
		secureExecRoot,
		"packages/core/src/index.ts",
		[
			'export * from "./native-client.js";',
			'export * as protocol from "./generated-protocol.js";',
			'export * from "./generated-protocol.js";',
			"",
		].join("\n"),
	);
	write(
		secureExecRoot,
		"packages/core/src/generated-protocol.ts",
		[
			"export type ProtocolFrame = {};",
			"export function readProtocolFrame() {}",
			"export function writeProtocolFrame() {}",
			"",
		].join("\n"),
	);
	write(secureExecRoot, "packages/core/src/native-client.ts", "export const native = true;\n");

	writeJson(agentOsRoot, "packages/core/package.json", {
		name: "@rivet-dev/agentos-core",
		dependencies: {
			"@secure-exec/core": "link:../../../secure-exec/packages/core",
		},
	});
	write(
		agentOsRoot,
		"packages/core/src/index.ts",
		[
			'export { AgentOs, AgentOsSidecar } from "./agent-os.js";',
			'export { CronManager } from "./cron/index.js";',
			'export { hostTool, toolKit } from "./host-tools.js";',
			'export { defineSoftware } from "./packages.js";',
			"",
		].join("\n"),
	);
	write(
		agentOsRoot,
		"packages/core/src/agent-os.ts",
		[
			'import { convert } from "@secure-exec/core/descriptors";',
			'import { makeAcp } from "./sidecar/agentos-protocol.js";',
			'import { createAgentOsSidecarClient } from "./sidecar/rpc-client.js";',
			"export class AgentOs {",
			"static async create() { return new AgentOs(); }",
			"static async createSidecar() {}",
			...[
				"exec",
				"readFile",
				"writeFile",
				"writeFiles",
				"readFiles",
				"mkdir",
				"readdir",
				"stat",
				"exists",
				"fetch",
				"connectTerminal",
				"createSession",
				"destroySession",
				"prompt",
				"cancelSession",
				"respondPermission",
				"setSessionMode",
				"rawSessionSend",
				"rawSend",
				"dispose",
			].map((method) => `async ${method}() {}`),
			...[
				"spawn",
				"openShell",
				"closeSession",
				"getSessionModes",
				"onSessionEvent",
				"scheduleCron",
				"listCronJobs",
				"cancelCronJob",
				"onCronEvent",
			].map((method) => `${method}() {}`),
			"extensionRequest() {}",
			"}",
			"String(convert); String(makeAcp); String(createAgentOsSidecarClient);",
			"",
		].join("\n"),
	);
	write(
		agentOsRoot,
		"packages/core/src/sidecar/native-process-client.ts",
		'import { NativeSidecarProcessClient } from "@secure-exec/core/sidecar-client";\n',
	);
	for (const file of [
		"host-tools.ts",
		"host-tools-zod.ts",
		"packages.ts",
		"cron/index.ts",
		"cron/cron-manager.ts",
		"sidecar/agentos-protocol.ts",
	]) {
		write(agentOsRoot, `packages/core/src/${file}`, "export {};\n");
	}
	return { agentOsRoot, secureExecRoot };
}

test("reports ready for the TS split boundary", () => {
	withFixture((root) => {
		const { agentOsRoot, secureExecRoot } = seedReadyFixture(root);
		const result = auditTsSplitBoundary({ agentOsRoot, secureExecRoot });
		assert.equal(
			result.ready,
			true,
			result.checks
				.filter((item) => !item.ok)
				.map((item) => `${item.name}: ${item.details}`)
				.join("\n"),
		);
	});
});

test("reports Agent OS facade regressions in secure-exec core", () => {
	withFixture((root) => {
		const { agentOsRoot, secureExecRoot } = seedReadyFixture(root);
		write(
			secureExecRoot,
			"packages/core/src/index.ts",
			[
				'export * as protocol from "./generated-protocol.js";',
				'export * from "./generated-protocol.js";',
				"export class AgentOs {}",
				"export function hostTool() {}",
				"",
			].join("\n"),
		);
		const result = auditTsSplitBoundary({ agentOsRoot, secureExecRoot });
		assert.equal(result.ready, false);
		const boundaryCheck = result.checks.find(
			(item) => item.name === "@secure-exec/core has no Agent OS facade or TS-only sugar",
		);
		assert(boundaryCheck);
		assert.equal(boundaryCheck.ok, false);
		assert.match(boundaryCheck.details, /AgentOs facade|Agent OS host-tools sugar/);
	});
});
