import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const workflow = readFileSync(
	new URL("../.github/workflows/ci.yml", import.meta.url),
	"utf8",
);
const defaultSoftware = JSON.parse(
	readFileSync(
		new URL("../software/default-software.json", import.meta.url),
		"utf8",
	),
);
function job(name, source = workflow) {
	const body = source.split(`\n  ${name}:\n`)[1];
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
	"name: wasm-command-licenses",
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
	const makefile = readFileSync(
		new URL("../toolchain/Makefile", import.meta.url),
		"utf8",
	);
	const prPackages = makefile
		.match(/^DEFAULT_SOFTWARE := (.+)$/m)?.[1]
		.split(" ");
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

test("backend packaging restores the verified GNU license from the command producer", () => {
	const producer = step("name: wasm-command-licenses");
	assert.match(producer, /uses: actions\/upload-artifact@v4/);
	assert.match(producer, /path: toolchain\/c\/build\/awk\.COPYING/);
	assert.match(producer, /if-no-files-found: error/);
	const backend = job("wasm-backend-matrix");
	const steps = backend.split(/\n      - /).slice(1);
	const restoreIndex = steps.findIndex((source) =>
		source.includes("name: wasm-command-licenses"),
	);
	assert.ok(restoreIndex >= 0, "backend downloads the license artifact");
	assert.match(steps[restoreIndex], /uses: actions\/download-artifact@v4/);
	assert.match(steps[restoreIndex], /path: toolchain\/c\/build\s*$/);
	const packageIndex = steps.findIndex((source) =>
		source.includes("pnpm --filter '@agentos-software/*' build"),
	);
	assert.ok(
		packageIndex > restoreIndex,
		"license is restored before packaging",
	);
	const licenseScript = readFileSync(
		new URL("../software/gawk/scripts/stage-license.mjs", import.meta.url),
		"utf8",
	);
	assert.match(licenseScript, /toolchain\/c\/build\/awk\.COPYING/);
	assert.match(licenseScript, /digest !== source\.licenseSha256/);
});

test("publish restores the GNU license before building software packages", () => {
	const publish = readFileSync(
		new URL("../.github/workflows/publish.yaml", import.meta.url),
		"utf8",
	);
	const producer = job("wasm-commands", publish)
		.split(/\n      - /)
		.find((source) => source.includes("name: wasm-command-licenses"));
	assert.ok(producer, "publish uploads the license artifact");
	assert.match(producer, /uses: actions\/upload-artifact@v4/);
	assert.match(producer, /path: toolchain\/c\/build\/awk\.COPYING/);
	assert.match(producer, /if-no-files-found: error/);
	const consumer = job("publish-npm", publish)
		.split(/\n      - /)
		.slice(1);
	const restoreIndex = consumer.findIndex((source) =>
		source.includes("name: wasm-command-licenses"),
	);
	assert.ok(restoreIndex >= 0, "publish downloads the license artifact");
	assert.match(consumer[restoreIndex], /uses: actions\/download-artifact@v4/);
	assert.match(consumer[restoreIndex], /path: toolchain\/c\/build\s*$/);
	const buildIndex = consumer.findIndex((source) =>
		source.includes("npx turbo build"),
	);
	assert.ok(
		buildIndex > restoreIndex,
		"license is restored before package builds",
	);
});

test("native ownership acceptance stays opt-in inside the expensive V8 job", () => {
	const backend = job("wasm-backend-matrix");
	assert.match(backend, /backend: \[v8\]/);
	assert.doesNotMatch(backend.split("    steps:")[0], /AGENTOS_STAB_HOSTED_LIFECYCLE_NATIVE/);
	const steps = backend.split(/\n      - /).slice(1);
	const core = steps.find(source => source.includes("name: Run native sidecar ownership acceptance"));
	const actor = steps.find(source => source.includes("name: Run actor lifecycle cancellation ownership"));
	assert.ok(core && actor, "both native ownership gates must be selected");
	for (const source of [core, actor]) {
		assert.match(source, /AGENTOS_STAB_HOSTED_LIFECYCLE_NATIVE: '1'/);
		assert.doesNotMatch(source, /--all-features|--features\s+hosted|wasmtime|BINARYEN/);
	}
	assert.match(core, /timeout-minutes: 2/);
	assert.match(core, /tests\/sidecar-forced-exit\.native\.test\.ts/);
	assert.match(core, /tests\/pending-creation\.native\.test\.ts/);
	assert.match(core, /--reporter=json/);
	assert.match(core, /assert\.equal\(result\.numTotalTests, 2\)/);
	assert.match(core, /assert\.equal\(result\.numPassedTests, 2\)/);
	assert.match(core, /assert\.equal\(result\.numPendingTests, 0/);
	assert.doesNotMatch(core, /--passWithNoTests/);
	assert.match(actor, /timeout-minutes: 15/);
	assert.equal((actor.match(/cargo test --release --locked -p agentos-actor --lib runtime::tests::/g) ?? []).length, 2);
	assert.match(actor, /-- --ignored --list/);
	assert.match(actor, /diff -u scripts\/ci\/actor-native-lifecycle-tests\.txt/);
	assert.match(actor, /-- --ignored --nocapture --test-threads=1/);
	assert.ok(actor.indexOf("diff -u") < actor.indexOf("-- --ignored --nocapture"), "verify exact nonempty selection before execution");
	for (const name of ["checks", "rust", "core-pr", "core-runtime-pr"]) {
		assert.doesNotMatch(job(name), /AGENTOS_STAB_HOSTED_LIFECYCLE_NATIVE/);
	}
});

test("the actor native gate inventories four actual ignored lifecycle controls", () => {
	const inventory = readFileSync(new URL("./ci/actor-native-lifecycle-tests.txt", import.meta.url), "utf8");
	const selected = inventory.split("\n").filter(line => line.endsWith(": test"));
	assert.equal(selected.length, 4);
	assert.equal(new Set(selected).size, 4);
	assert.match(inventory, /\n4 tests, 0 benchmarks\n$/);
	const runtime = readFileSync(new URL("../crates/actor/src/runtime.rs", import.meta.url), "utf8");
	for (const line of selected) {
		const name = line.replace(/^runtime::tests::/, "").replace(/: test$/, "");
		assert.ok(runtime.includes(`async fn ${name}(`), `missing selected actor test ${name}`);
	}
	assert.match(runtime, /AGENTOS_STAB_HOSTED_LIFECYCLE_NATIVE/);
});

test("the expensive job builds Core once through its dependency-inclusive selector", () => {
	const backend = job("wasm-backend-matrix");
	const build = backend.split(/\n      - /).find(source => source.includes("name: Build the release sidecar and public test clients"));
	assert.ok(build);
	assert.match(build, /pnpm --filter '@rivet-dev\/agentos\.\.\.' build/);
	assert.match(build, /pnpm --filter @rivet-dev\/agentos-test-harness build/);
	assert.doesNotMatch(build, /pnpm --filter @rivet-dev\/agentos build/);
});
