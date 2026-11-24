import { createRequire } from "node:module";
import { withOwnedEsbuild } from "../../build-tools/scripts/with-owned-esbuild.mjs";

const require = createRequire(import.meta.url);
const tsupRequire = createRequire(require.resolve("tsup"));
await withOwnedEsbuild(tsupRequire.resolve("esbuild"), async () => {
  const { build } = await import("tsup");
  // Same CLI entries, --format esm and --dts; public build retains config loading.
  await build({ entry: ["src/index.ts", "src/cli.ts"], format: ["esm"], dts: {} });
});
