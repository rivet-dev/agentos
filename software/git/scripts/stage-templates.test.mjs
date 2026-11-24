import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

function cleanCheckout(t) {
	const root = mkdtempSync(join(tmpdir(), "agentos-git-staging-"));
	t.after(() => rmSync(root, { recursive: true, force: true }));
	const scripts = join(root, "software", "git", "scripts");
	const toolchain = join(root, "toolchain", "c");
	mkdirSync(scripts, { recursive: true });
	mkdirSync(toolchain, { recursive: true });
	const script = join(scripts, "stage-templates.mjs");
	copyFileSync(new URL("./stage-templates.mjs", import.meta.url), script);
	copyFileSync(
		new URL("../../../toolchain/c/Makefile", import.meta.url),
		join(toolchain, "Makefile"),
	);
	return { root, script };
}

function runStaging(fixture, codeOnly) {
	const env = { ...process.env };
	delete env.AGENTOS_CI_CODE_ONLY_BUILD;
	if (codeOnly !== undefined) env.AGENTOS_CI_CODE_ONLY_BUILD = codeOnly;
	return spawnSync(process.execPath, [fixture.script], {
		cwd: fixture.root,
		env,
		encoding: "utf8",
		timeout: 10_000,
		maxBuffer: 1024 * 1024,
	});
}

test("code-only CI can build Git TypeScript before upstream sources exist", (t) => {
	const result = runStaging(cleanCheckout(t), "1");
	assert.ifError(result.error);
	assert.equal(result.signal, null);
	assert.equal(result.status, 0, result.stderr);
	assert.match(result.stdout, /skipped.*Git.*code-only/);
});

test("package staging requires upstream Git templates outside code-only CI", (t) => {
	const fixture = cleanCheckout(t);
	for (const flag of [undefined, "0", "true"]) {
		const result = runStaging(fixture, flag);
		assert.ifError(result.error);
		assert.equal(result.signal, null);
		assert.notEqual(result.status, 0);
		assert.match(
			result.stderr,
			/Git source templates are absent.*build toolchain cmd\/git first/,
		);
	}
});
