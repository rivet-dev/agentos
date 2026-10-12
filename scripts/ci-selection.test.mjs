import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const workflow = readFileSync(
	new URL("../.github/workflows/ci.yml", import.meta.url),
	"utf8",
);
const defaultSoftware = JSON.parse(
	readFileSync(new URL("../software/default-software.json", import.meta.url), "utf8"),
);
function job(name) {
	const body = workflow.split(`\n  ${name}:\n`)[1];
	assert.ok(body, `missing ${name} job`);
	return body.split(/\n  [\w-]+:\n/)[0];
}
function condition(source, key = "if") {
	const match = source.match(new RegExp(`^\\s*${key}: (.+)$`, "m"));
	assert.ok(match, `missing ${key} condition`);
	return match[1];
}
function evaluate(expression, { event, base = "main", labels = [] }) {
	// These selectors use only the small Boolean subset below. Reject unknown
	// expressions instead of accidentally treating unsupported syntax as true.
	const resolved = expression
		.replace(/^\$\{\{\s*|\s*\}\}$/g, "")
		.replaceAll(
			"contains(github.event.pull_request.labels.*.name, 'aggregate-ci')",
			String(labels.includes("aggregate-ci")),
		)
		.replaceAll("always()", "true")
		.replaceAll("github.event_name", JSON.stringify(event))
		.replaceAll("github.base_ref", JSON.stringify(base));
	assert.match(resolved, /^(?:true|false|"[\w-]+"|'[\w-]+'|[\s!=&|()])+$/);
	return Function(`"use strict"; return (${resolved});`)();
}
const commandSteps = job("wasm-commands")
	.split(/\n      - /)
	.slice(1);
function step(identifier) {
	const result = commandSteps.find((source) => source.includes(identifier));
	assert.ok(result, `missing command step ${identifier}`);
	return result;
}
const fullSteps = [
	"Build the complete command corpus",
	"Build required heavy integration commands",
	"Build mandatory threaded-WASM conformance fixtures",
	"Stage native/WASM C parity fixtures",
	"Stage the complete command corpus",
	"name: wasm-thread-fixtures",
	"name: wasm-c-parity-fixtures",
];
for (const [name, context, full] of [
	["ordinary PR", { event: "pull_request" }, false],
	[
		"unrelated PR label",
		{ event: "pull_request", labels: ["documentation"] },
		false,
	],
	[
		"aggregate PR to main",
		{ event: "pull_request", labels: ["aggregate-ci"] },
		true,
	],
	[
		"aggregate stacked PR",
		{ event: "pull_request", base: "stack-parent", labels: ["aggregate-ci"] },
		true,
	],
	["main push", { event: "push" }, true],
	["manual dispatch", { event: "workflow_dispatch" }, true],
]) {
	test(`${name} consistently selects its corpus, artifacts, and backend gate`, () => {
		for (const id of [
			"checks",
			"wasm-commands",
			"rust",
			"executor-feature-isolation",
			"core-pr",
			"core-runtime-pr",
			"required",
		]) {
			assert.equal(
				evaluate(condition(job(id)), context),
				true,
				`${id} admitted`,
			);
		}
		for (const id of fullSteps) {
			assert.equal(evaluate(condition(step(id)), context), full, id);
		}
		for (const id of [
			"Build the default software PR command corpus",
			"Stage the default software PR command corpus",
		]) {
			assert.equal(evaluate(condition(step(id)), context), !full, id);
		}
		assert.equal(
			evaluate(condition(job("wasm-backend-matrix")), context),
			full,
			"backend matrix admitted",
		);
		assert.equal(
			evaluate(
				condition(job("required"), "EXPECT_WASM_BACKEND_MATRIX"),
				context,
			),
			full,
			"required gate expects matrix success",
		);
	});
}

test("CI builds code before one default-software producer and restores artifacts for runtime tests", () => {
	const checks = job("checks");
	const commands = job("wasm-commands");
	const transport = job("core-pr");
	const runtime = job("core-runtime-pr");
	assert.match(checks, /AGENTOS_CI_CODE_ONLY_BUILD: '1'/);
	assert.match(commands, /runs-on: depot-ubuntu-24\.04-8/);
	assert.match(checks, /npx turbo build \\\n\s+--only/);
	assert.match(commands, /name: Package default software once/);
	assert.match(commands, /name: default-software/);
	assert.match(commands, /copy-wasm-commands\.mjs --require-default/);
	for (const name of defaultSoftware) {
		assert.ok(
			commands.includes(`--filter @agentos-software/${name}`),
			`default package ${name} is built by the WASM producer`,
		);
	}
	const makefile = readFileSync(new URL("../toolchain/Makefile", import.meta.url), "utf8");
	const prPackages = makefile.match(/^DEFAULT_SOFTWARE := (.+)$/m)?.[1].split(" ");
	assert.deepEqual(prPackages, defaultSoftware);
	assert.match(makefile, /PR_C_COMMANDS := .*\bgrep\b/);
	assert.match(transport, /needs: \[checks\]/);
	assert.doesNotMatch(transport, /name: default-software/);
	assert.match(runtime, /needs: \[checks, rust, wasm-commands\]/);
	assert.match(runtime, /name: default-software/);
	assert.doesNotMatch(runtime, /name: wasm-commands/);
	assert.ok(
		runtime.indexOf("tar -xzf ci-artifacts/default-software/") <
			runtime.indexOf("pnpm --dir packages/core test:pr"),
	);
	assert.ok(
		runtime.indexOf("stage-default-software.mjs") <
			runtime.indexOf("pnpm --dir packages/core test:pr"),
	);
});
