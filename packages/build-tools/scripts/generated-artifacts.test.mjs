import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, mkdir, readFile, writeFile, copyFile, symlink, rm, stat, utimes } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const packageDir = fileURLToPath(new URL("../", import.meta.url));
for (const [script, schema, output, command] of [
	["compile-sidecar-protocol.mjs", "crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare", "packages/core/src/generated-protocol.ts", "build:protocol"],
	["compile-package-format.mjs", "crates/vfs-core/package-format/v2.bare", "packages/agentos-toolchain/src/generated-package-format.ts", "build:package-format"],
]) {
	test(`${script} verifies without rewriting and rejects drift`, async () => {
		const root = await mkdtemp(path.join(os.tmpdir(), "agentos-generated-check-"));
		try {
			const scripts = path.join(root, "packages/build-tools/scripts");
			await mkdir(scripts, { recursive: true });
			await copyFile(path.join(packageDir, "scripts", script), path.join(scripts, script));
			await symlink(path.join(packageDir, "node_modules"), path.join(root, "packages/build-tools/node_modules"), "dir");
			await mkdir(path.dirname(path.join(root, schema)), { recursive: true });
			await writeFile(path.join(root, schema), "type Fixture u8\n");
			const run = (...args) => spawnSync(process.execPath, [path.join(scripts, script), ...args], { encoding: "utf8", timeout: 5000, maxBuffer: 16384 });
			const generated = run();
			assert.equal(generated.status, 0, generated.stderr);
			const target = path.join(root, output);
			const original = await readFile(target, "utf8");
			await utimes(target, 1, 1);
			const before = await stat(target);
			const checked = run("--check");
			assert.equal(checked.status, 0, checked.stderr);
			assert.equal((await stat(target)).mtimeMs, before.mtimeMs);
			await writeFile(target, `${original}// stale\n`);
			const stale = run("--check");
			assert.equal(stale.status, 1);
			assert.ok(stale.stderr.includes(command));
			assert.equal(await readFile(target, "utf8"), `${original}// stale\n`);
			await rm(target);
			const missing = run("--check");
			assert.equal(missing.status, 1);
			await assert.rejects(stat(target), { code: "ENOENT" });
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});
}
