import assert from "node:assert/strict";
import test from "node:test";
import semver from "semver";
import { resolveContext } from "./context.js";

const sha = "0123456789abcdef0123456789abcdef01234567";

test("explicit release versions require canonical SemVer", async () => {
	for (const version of ["1.2", "v1.2.3", "1.2.3 ", "01.2.3"]) {
		await assert.rejects(resolveContext({ trigger: "release", version, sha }), TypeError);
	}
	for (const [version, latest, tag, promoted] of [
		["1.2.3", true, "latest", true],
		["1.2.3+build", false, "next", false],
		["1.2.3-rc.0", true, "rc", false],
		["1.2.3-beta.0", true, "next", false],
	] as const) {
		const context = await resolveContext({ trigger: "release", version, latest, sha });
		assert.equal(context.version, version);
		assert.equal(context.npmTag, tag);
		assert.equal(context.latest, promoted);
		assert.equal(context.sha, sha);
	}
});

test("preview tags reserve release names and distinguish normalized branches", async () => {
	const names = ["latest", "NEXT", "rc", "feature/x", "feature-x", "123", "名前"];
	const contexts = await Promise.all(names.map((branch) =>
		resolveContext({ trigger: "branch", branch, sha }),
	));
	assert.equal(new Set(contexts.map((ctx) => ctx.npmTag)).size, names.length);
	for (const context of contexts) {
		assert.match(context.npmTag, /^preview-/);
		assert.equal(semver.valid(context.version), context.version);
	}
	const next = await resolveContext({ trigger: "branch", branch: names[0], sha: `${sha.slice(0, -1)}8` });
	assert.equal(next.npmTag, contexts[0].npmTag);
	assert.notEqual(next.version, contexts[0].version);
});


test("artifact staging refuses abbreviated commit IDs", async () => {
	await assert.rejects(
		resolveContext({ trigger: "release", version: "1.2.3", sha: sha.slice(0, 7) }),
		/full hexadecimal commit SHA/,
	);
});
