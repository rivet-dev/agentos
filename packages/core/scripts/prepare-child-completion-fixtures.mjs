import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { readFileSync, realpathSync, writeFileSync, mkdirSync } from "node:fs";
import { resolve, dirname, join } from "node:path";
import { createHash } from "node:crypto";

const [outputArg] = process.argv.slice(2);
if (!outputArg) throw new Error("usage: prepare-child-completion-fixtures.mjs FRESH_OUTPUT");
const core = fileURLToPath(new URL("../", import.meta.url));
const workspace = realpathSync(join(core, "../.."));
const output = resolve(outputArg);

const require = createRequire(join(core, "package.json"));
const dependencyPins = {};
const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
for (const name of ["effect", "@effect/platform-node"]) {
  const root = realpathSync(join(core, "node_modules", name));
  const manifest = join(root, "package.json");
  const raw = readFileSync(manifest);
  const value = JSON.parse(raw);
  if (value.version !== "4.0.5") throw new Error(`requires installed ${name}4.0.5; found ${value.version}`);
  dependencyPins[manifest] = sha(raw);
}
// The sync service uses execFileSync and reaps its child. This build-only setting
// avoids a persistent esbuild worker/service in the later frozen runtime worker.
process.env.ESBUILD_WORKER_THREADS = "0";
const esbuild = require("esbuild");
const probe = fileURLToPath(new URL("../tests/fixtures/child-completion/effect-probe.mjs", import.meta.url));
const source = readFileSync(probe, "utf8");
const built = esbuild.buildSync({ stdin: { contents: source, resolveDir: core, sourcefile: probe }, absWorkingDir: workspace, bundle: true, platform: "node", target: "node24", format: "esm", minify: true, legalComments: "none", write: false, metafile: true });
if (built.outputFiles.length !== 1 || built.outputFiles[0].contents.length > 4 * 1024 * 1024) throw new Error("fixture output exceeds one4MiB bounded module");
const inputs = { [probe]: sha(readFileSync(probe)), ...dependencyPins };
for (const input of Object.keys(built.metafile.inputs)) {
  if (input.startsWith("<") || input === "child-completion-effect-probe.mjs") continue;
  const path = realpathSync(resolve(workspace, input));
  inputs[path] = sha(readFileSync(path));
}
inputs[require.resolve("esbuild")] = sha(readFileSync(require.resolve("esbuild")));
const esbuildRoot = dirname(require.resolve("esbuild/package.json"));
const esbuildBinary = realpathSync(process.env.ESBUILD_BINARY_PATH ?? join(esbuildRoot, "bin/esbuild"));
inputs[esbuildBinary] = sha(readFileSync(esbuildBinary));
mkdirSync(output, { recursive: false });
const artifact = join(output, "effect-probe.mjs");
writeFileSync(artifact, built.outputFiles[0].contents, { flag: "wx" });
writeFileSync(join(output, "metafile.json"), JSON.stringify(built.metafile, null, 2) + "\n", { flag: "wx" });
writeFileSync(join(output, "receipt.json"), JSON.stringify({ status: "prepared-not-executed", workspace, artifact, sha256: sha(built.outputFiles[0].contents), inputs, dependencyVersions: { effect: "4.0.5", platformNode: "4.0.5" }, limits: { bundleBytes: 4 * 1024 * 1024 }, esbuild: esbuild.version, esbuildWorkers: "disabled for synchronous preparation only" }, null, 2) + "\n", { flag: "wx" });
console.log(JSON.stringify({ prepared: output, artifact, bytes: built.outputFiles[0].contents.length }));
