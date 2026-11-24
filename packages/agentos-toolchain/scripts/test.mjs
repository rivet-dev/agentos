import { createRequire } from "node:module";
import { withOwnedEsbuild } from "../../build-tools/scripts/with-owned-esbuild.mjs";

const require = createRequire(import.meta.url);
const vitestRequire = createRequire(require.resolve("vitest/node"));
const viteRequire = createRequire(vitestRequire.resolve("vite"));
let ctx;
await withOwnedEsbuild(viteRequire.resolve("esbuild"), async () => {
  const { parseCLI, startVitest } = await import("vitest/node");
  const { normalize } = vitestRequire("pathe");
  const { filter, options } = parseCLI(["vitest", "run", ...process.argv.slice(2)]);
  // Same normalization as Vitest's run CLI; preserve config and reporter choices.
  if (options.exclude) {
    options.cliExclude = Array.isArray(options.exclude) ? options.exclude : [options.exclude];
    delete options.exclude;
  }
  try {
    ctx = await startVitest("test", filter.map(normalize), options);
  } finally {
    // startVitest sets normal failure/no-tests exit codes and closes run contexts.
    // Await close before the shared owner stops and reaps the Vite build service.
    await ctx?.close();
  }
});
// Preserve the CLI's final exit handling only after service ownership is settled.
if (!ctx?.shouldKeepServer()) await ctx?.exit();
