// Internal build boundary: stop the actual service and await its ChildProcess close.
import assert from "node:assert/strict";
import childProcess from "node:child_process";
import { createRequire, syncBuiltinESMExports } from "node:module";
import { existsSync, realpathSync } from "node:fs";
import path from "node:path";

export async function withOwnedEsbuild(esbuildEntry, body) {
  const require = createRequire(esbuildEntry);
  const esbuild = require(esbuildEntry);
  const packageJson = require("esbuild/package.json");
  const binaries = new Set();
  function addBinary(candidate) {
    binaries.add(path.resolve(candidate));
    if (existsSync(candidate)) binaries.add(realpathSync(candidate));
  }
  // Match esbuild's ignored default/bad override; it emits its own warning.
  const override = process.env.ESBUILD_BINARY_PATH;
  if (override && override !== "/usr/bin/esbuild" && existsSync(override)) addBinary(override);
  for (const name of Object.keys(packageJson.optionalDependencies ?? {})) {
    for (const suffix of ["bin/esbuild", "esbuild.exe"]) {
      // esbuild's postinstall fallback when the optional native package is absent.
      addBinary(path.join(path.dirname(esbuildEntry), `downloaded-${name.replace("/", "-")}-${path.basename(suffix)}`));
      let resolved;
      try { resolved = require.resolve(`${name}/${suffix}`); }
      catch (error) { if (error.code === "MODULE_NOT_FOUND" || error.code === "ERR_PACKAGE_PATH_NOT_EXPORTED") continue; throw error; }
      addBinary(resolved);
      if (/\.zip\//.test(resolved)) {
        // generateBinPath materializes only this exact PnP cache target.
        let pnpapi;
        try { pnpapi = require("pnpapi"); }
        catch (error) { if (error.code !== "MODULE_NOT_FOUND") throw error; }
        if (pnpapi) {
          const root = realpathSync(pnpapi.getPackageInformation(pnpapi.topLevel).packageLocation);
          addBinary(path.join(root, "node_modules", ".cache", "esbuild",
            `pnpapi-${name.replace("/", "-")}-${esbuild.version}-${path.basename(suffix)}`));
        }
      }
    }
  }
  const originalSpawn = childProcess.spawn;
  const errors = [];
  let owned = null, result;
  childProcess.spawn = function(command, args, options) {
    // Leave unrelated build children with their existing owner.
    const service = `--service=${esbuild.version}`;
    if (!Array.isArray(args) || (args[0] !== service && args[1] !== service))
      return originalSpawn.call(this, command, args, options);
    let binary = command;
    if (args[1] === service) {
      // esbuild's supported WASM fallback runs its package binary through Node.
      assert.equal(command, "node");
      assert.deepEqual(args.slice(1), [service, "--ping"]);
      binary = args[0];
    } else assert.deepEqual(args, [service, "--ping"]);
    assert(binaries.has(realpathSync(binary)), "unexpected esbuild service binary");
    assert.equal(owned, null, "unexpected second esbuild service in one build boundary");
    const child = originalSpawn.call(this, command, args, options);
    const row = { child, errors: [], close: null, code: null, signal: null };
    row.close = new Promise(resolve => {
      child.on("error", error => row.errors.push(error));
      child.once("close", (code, signal) => { row.code = code; row.signal = signal; resolve(); });
    });
    owned = row;
    return child;
  };
  syncBuiltinESMExports();
  try { result = await body(); }
  catch (error) { errors.push(error); }
  finally {
    try { if (owned) owned.child.ref(); } catch (error) { errors.push(error); }
    try { await esbuild.stop(); } catch (error) { errors.push(error); }
    if (owned) {
      await owned.close;
      errors.push(...owned.errors);
      if (!((owned.code === 0 && owned.signal === null) || (owned.code === null && owned.signal === "SIGTERM")))
        errors.push(new Error(`esbuild service closed unexpectedly: code=${owned.code} signal=${owned.signal}`));
    }
    childProcess.spawn = originalSpawn;
    syncBuiltinESMExports();
  }
  if (errors.length === 1) throw errors[0];
  if (errors.length > 1) throw new AggregateError(errors, "build and esbuild service cleanup failed");
  return result;
}
