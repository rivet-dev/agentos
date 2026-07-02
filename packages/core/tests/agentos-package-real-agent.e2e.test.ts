import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

/**
 * REAL production-agent end-to-end proof: pack the published
 * `@agentos-software/pi` ACP adapter with the toolchain (flat + `--prune-native`),
 * project it as an `/opt/agentos` package, and launch a session via
 * `/opt/agentos/bin/pi-sdk-acp` — exercising the whole new package model on a
 * real ESM agent with dynamic imports.
 *
 * Gated behind `AGENTOS_TEST_REAL_PI=1` because it runs a real `npm install`
 * (network + ~30s) — it is not part of the default suite. Run with:
 *   AGENTOS_TEST_REAL_PI=1 pnpm --dir packages/core exec vitest run \
 *     tests/agentos-package-real-agent.e2e.test.ts
 */
const ENABLED = process.env.AGENTOS_TEST_REAL_PI === "1";
const here = dirname(fileURLToPath(import.meta.url));
// Toolchain now lives in the sibling secure-exec repo (see its package README).
const TOOLCHAIN_CLI = resolve(
	here,
	"../../../../secure-exec/packages/agentos-toolchain/dist/cli.js",
);

/** Real published agent adapters, packed flat with native addons pruned. */
const AGENTS = [
	{ pkg: "@agentos-software/pi", acpEntrypoint: "pi-sdk-acp" },
	{ pkg: "@agentos-software/claude-code", acpEntrypoint: "claude-sdk-acp" },
];

describe.skipIf(!ENABLED).each(AGENTS)(
	"real agent package end-to-end ($pkg)",
	({ pkg, acpEntrypoint }) => {
		let vm: AgentOs;
		let outRoot: string;
		let packageDir: string;

		beforeAll(async () => {
			outRoot = mkdtempSync(join(tmpdir(), "agentos-real-agent-"));
			// --prune-native drops unreachable native addons from the flat closure
			// while keeping dynamically-imported modules that --bundle would miss.
			const stdout = execFileSync(
				"node",
				[
					TOOLCHAIN_CLI,
					"pack",
					pkg,
					"--agent",
					acpEntrypoint,
					"--prune-native",
					"--out",
					outRoot,
				],
				{ encoding: "utf8" },
			);
			const match = stdout.match(/→\s+(\S+)/);
			if (!match) throw new Error(`could not parse pack output: ${stdout}`);
			packageDir = match[1];

			vm = await AgentOs.create({
				defaultSoftware: false,
				software: [packageDir],
			});
		}, 120_000);

		afterAll(async () => {
			await vm?.dispose();
			if (outRoot) rmSync(outRoot, { recursive: true, force: true });
		});

		test("lists the real agent as installed", () => {
			const entry = vm.listAgents().find((a) => a.id === pkg);
			expect(entry?.installed).toBe(true);
			expect(entry?.adapterEntrypoint).toBe(`/opt/agentos/bin/${acpEntrypoint}`);
		});

		test("createSession launches the real adapter and returns a session", async () => {
			const session = await vm.createSession(pkg);
			expect(session.sessionId).toBeTruthy();
			await vm.closeSession(session.sessionId);
		}, 60_000);
	},
);

/**
 * The built-in agents projected by DEFAULT: the @agentos-software/* agent
 * package deps (each ships its registry-built dist/package), boot with default
 * software (no explicit agent package), and launch by friendly id — no runtime
 * npm dep on the SDKs and no packing step.
 */
describe.skipIf(!ENABLED)("default built-in agents", () => {
	let vm: AgentOs;

	beforeAll(async () => {
		vm = await AgentOs.create({}); // default software — includes the built-in agent packages
	}, 240_000);

	afterAll(async () => {
		await vm?.dispose();
	});

	test("lists pi + claude by friendly id, installed", () => {
		const ids = vm.listAgents().filter((a) => a.installed).map((a) => a.id);
		expect(ids).toContain("pi");
		expect(ids).toContain("claude");
	});

	test.each(["pi", "claude"])(
		"createSession('%s') launches via the default projection",
		async (id) => {
			const session = await vm.createSession(id);
			expect(session.sessionId).toBeTruthy();
			await vm.closeSession(session.sessionId);
		},
		60_000,
	);
});
