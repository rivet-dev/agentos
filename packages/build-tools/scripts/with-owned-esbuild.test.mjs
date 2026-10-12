import assert from "node:assert/strict";
import childProcess from "node:child_process";
import { createRequire, syncBuiltinESMExports } from "node:module";
import test from "node:test";
import { withOwnedEsbuild } from "./with-owned-esbuild.mjs";
const require = createRequire(import.meta.url);
const esbuildEntry = require.resolve("esbuild");
const esbuild = require(esbuildEntry);

async function observeBuild(body) {
  const original = childProcess.spawn;
  const children = [];
  childProcess.spawn = function(command, args, options) {
    const child = original.call(this, command, args, options);
    if (args?.[0] === `--service=${esbuild.version}` || args?.[1] === `--service=${esbuild.version}`) {
      const row = { closed: false };
      child.once("close", () => { row.closed = true; });
      children.push(row);
    }
    return child;
  };
  syncBuiltinESMExports();
  const observingSpawn = childProcess.spawn;
  try { await body(); }
  finally {
    const restored = childProcess.spawn === observingSpawn;
    childProcess.spawn = original; syncBuiltinESMExports();
    assert(restored, "build helper must restore its prior spawn hook");
    assert.equal(children.length, 1);
    assert(children.every(child => child.closed), "helper settled before its service closed");
  }
}
test("successful real build closes its service before returning", async () => {
  await observeBuild(async () => {
    const result = await withOwnedEsbuild(esbuildEntry, () => esbuild.build({
      stdin: { contents: "export const answer = 42", sourcefile: "success.js" },
      write: false, format: "esm", logLevel: "silent",
    }));
    assert.match(result.outputFiles[0].text, /answer = 42/);
  });
});
test("real build failure is preserved after the service closes", async () => {
  await observeBuild(async () => {
    await assert.rejects(withOwnedEsbuild(esbuildEntry, () => esbuild.build({
      stdin: { contents: "export const =", sourcefile: "failure.js" },
      write: false, logLevel: "silent",
    })), /Build failed/);
  });
});

test("postinstall-downloaded service and ignored overrides retain real esbuild behavior", async () => {
  const { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } = await import("node:fs/promises");
  const { tmpdir } = await import("node:os");
  const path = await import("node:path");
  const packageJson = JSON.parse(await readFile(require.resolve("esbuild/package.json"), "utf8"));
  const nativePackage = `@esbuild/${process.platform}-${process.arch}`;
  const subpath = process.platform === "win32" ? "esbuild.exe" : "bin/esbuild";
  const binary = createRequire(esbuildEntry).resolve(`${nativePackage}/${subpath}`);
  const temporary = await mkdtemp(path.join(tmpdir(), "agentos-esbuild-fallback-"));
  const previousOverride = process.env.ESBUILD_BINARY_PATH;
  try {
    for (const [index, override] of [undefined, path.join(temporary, "missing-binary"), "/usr/bin/esbuild"].entries()) {
      const packageRoot = path.join(temporary, String(index), "node_modules", "esbuild");
      const lib = path.join(packageRoot, "lib");
      await mkdir(lib, { recursive: true });
      await writeFile(path.join(packageRoot, "package.json"), JSON.stringify(packageJson));
      const entry = path.join(lib, "main.js");
      await copyFile(esbuildEntry, entry);
      await copyFile(binary, path.join(lib, `downloaded-${nativePackage.replace("/", "-")}-${path.basename(subpath)}`));
      const fixtureRequire = createRequire(entry);
      assert.throws(() => fixtureRequire.resolve(`${nativePackage}/${subpath}`), { code: "MODULE_NOT_FOUND" });
      if (override === undefined) delete process.env.ESBUILD_BINARY_PATH;
      else process.env.ESBUILD_BINARY_PATH = override;
      await observeBuild(async () => {
        const result = await withOwnedEsbuild(entry, () => fixtureRequire(entry).build({
          stdin: { contents: "export const fallback = true" }, write: false, format: "esm", logLevel: "silent",
        }));
        assert.match(result.outputFiles[0].text, /fallback = true/);
      });
    }
  } finally {
    if (previousOverride === undefined) delete process.env.ESBUILD_BINARY_PATH;
    else process.env.ESBUILD_BINARY_PATH = previousOverride;
    await rm(temporary, { recursive: true, force: true });
  }
});
