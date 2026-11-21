#!/usr/bin/env node
// Writes the fixture config that scripts/run-hosted-local.mjs runs: the pinned
// engine from the installed @rivetkit/engine-cli package, a prebuilt hosted
// worker, the staged default software, and the hosted test commands.
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { parseArgs } from "node:util";

const ALL_LABELS = ["ts-auth", "ts-lifecycle", "ts-transport", "rust-hosted"];
const { values, positionals } = parseArgs({
	options: {
		worker: { type: "string" },
		"rust-test": { type: "string" },
		out: { type: "string" },
		evidence: { type: "string" },
	},
	allowPositionals: true,
});
if (!values.worker || !values["rust-test"] || !values.out || !values.evidence) {
	console.error(
		"Usage: node scripts/hosted-local-config.mjs --worker <agentos-sidecar built with --features hosted> --rust-test <hosted_transport test binary> --out <config.json> --evidence <dir> [label...]",
	);
	process.exit(2);
}
const labels = positionals.length > 0 ? positionals : ALL_LABELS;

const repo = resolve(import.meta.dirname, "..");
const core = join(repo, "packages/core");
const sha256 = (path) =>
	createHash("sha256").update(readFileSync(path)).digest("hex");
const pin = (path, extra = {}) => ({ path, sha256: sha256(path), ...extra });

// The engine binary is the lockfile-pinned RivetKit dependency, never a download.
let rivetkit = dirname(
	createRequire(join(core, "package.json")).resolve("rivetkit/client"),
);
while (!existsSync(join(rivetkit, "package.json"))) rivetkit = dirname(rivetkit);
rivetkit = join(rivetkit, "package.json");
const engineCli = createRequire(rivetkit)("@rivetkit/engine-cli");
const engineVersion = JSON.parse(readFileSync(rivetkit, "utf8")).version;

const vitest = join(core, "node_modules/vitest/vitest.mjs");
const vitestRun = (file) => [
	process.execPath,
	vitest,
	"run",
	file,
	"--reporter=verbose",
];
const commands = {
	"ts-auth": vitestRun("tests/hosted-auth.nightly.test.ts"),
	"ts-lifecycle": vitestRun("tests/hosted-lifecycle.nightly.test.ts"),
	"ts-transport": vitestRun("tests/hosted-transport.nightly.test.ts"),
	"rust-hosted": [
		resolve(values["rust-test"]),
		"--ignored",
		"--test-threads=1",
		"--nocapture",
	],
};
for (const label of labels) {
	if (!commands[label]) {
		console.error(`unknown label ${label}; expected ${ALL_LABELS.join(", ")}`);
		process.exit(2);
	}
}

const evidence = resolve(values.evidence);
mkdirSync(evidence, { recursive: true });
const software = JSON.parse(
	readFileSync(join(repo, "software/default-software.json"), "utf8"),
).map((name) =>
	pin(join(repo, "software", name, "dist/package.aospkg"), { name }),
);
writeFileSync(
	resolve(values.out),
	`${JSON.stringify(
		{
			repo: core,
			evidenceParent: evidence,
			wholeMs: 900_000,
			engine: pin(engineCli.getEnginePath(), {
				expectedVersion: engineVersion,
			}),
			worker: pin(resolve(values.worker)),
			software,
			inputs: [
				pin(join(repo, "pnpm-lock.yaml")),
				pin(join(repo, "Cargo.lock")),
			],
			commands: labels.map((label) => ({
				label,
				argv: commands[label],
				executableSha256: sha256(commands[label][0]),
				timeoutMs: 300_000,
			})),
		},
		null,
		2,
	)}\n`,
);
