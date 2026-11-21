import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { copyPrefix, uploadDir } from "./r2.js";

test("staging and mirror publication order manifests last and prune only after success", async () => {
	const root = await mkdtemp(join(tmpdir(), "release-r2-"));
	const calls = join(root, "calls");
	const env = { ...process.env };
	try {
		await writeFile(join(root, "aws"), `#!/usr/bin/env node
const fs = require("node:fs");
const args = process.argv.slice(2);
const operation = args[1];
const arg = (name) => args[args.indexOf(name) + 1];
if (operation === "list-objects-v2") {
  const prefix = arg("--prefix");
  const names = prefix === "stage/"
    ? ["default-software.json", "manifest.json", "packages/one", "packages/two"]
    : ["default-software.json", "manifest.json", "packages/one", "stale"];
  console.log(JSON.stringify({ Contents: names.map((name) => ({ Key: prefix + name })) }));
} else {
  const key = operation === "cp"
    ? args[3].replace("s3://rivet-releases/", "")
    : arg("--key");
  fs.appendFileSync(process.env.CALLS, operation + " " + key + "\\n");
  if (process.env.FAIL_COPY === key) process.exitCode = 1;
}
`, { mode: 0o700 });
		Object.assign(process.env, {
			PATH: `${root}:${env.PATH}`,
			CALLS: calls,
			R2_RELEASES_ACCESS_KEY_ID: "test",
			R2_RELEASES_SECRET_ACCESS_KEY: "test",
			FAIL_COPY: "release/packages/two",
		});
		const recorded = async () => (await readFile(calls, "utf8")).trim().split("\n");
		await assert.rejects(copyPrefix("stage/", "release/"), /failed after copying 1 of 4 objects/);
		assert.deepEqual(await recorded(), [
			"copy-object release/packages/one",
			"copy-object release/packages/two",
		]);

		delete process.env.FAIL_COPY;
		await writeFile(calls, "");
		await copyPrefix("stage/", "release/");
		assert.deepEqual(await recorded(), [
			"copy-object release/packages/one",
			"copy-object release/packages/two",
			"copy-object release/manifest.json",
			"copy-object release/default-software.json",
			"delete-object release/stale",
		]);

		const local = join(root, "local");
		await mkdir(join(local, "packages"), { recursive: true });
		await writeFile(join(local, "packages/new"), "artifact");
		await writeFile(join(local, "default-software.json"), "{}");
		await writeFile(join(local, "manifest.json"), "{}");
		await writeFile(calls, "");
		process.env.FAIL_COPY = "stage/packages/new";
		await assert.rejects(uploadDir(local, "stage/"), /failed after uploading 0 of 3 objects/);
		assert.deepEqual(await recorded(), ["cp stage/packages/new"]);

		delete process.env.FAIL_COPY;
		await writeFile(calls, "");
		await uploadDir(local, "stage/");
		assert.deepEqual(await recorded(), [
			"cp stage/packages/new",
			"cp stage/manifest.json",
			"cp stage/default-software.json",
			"delete-object stage/packages/one",
			"delete-object stage/packages/two",
		]);
	} finally {
		for (const key of ["PATH", "CALLS", "R2_RELEASES_ACCESS_KEY_ID", "R2_RELEASES_SECRET_ACCESS_KEY", "FAIL_COPY"]) {
			if (env[key] === undefined) delete process.env[key];
			else process.env[key] = env[key];
		}
		await rm(root, { recursive: true, force: true });
	}
});
