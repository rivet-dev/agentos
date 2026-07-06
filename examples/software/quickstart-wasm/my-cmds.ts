import { defineSoftware } from "@rivet-dev/agentos";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// WASM output is already a package - no `pack` step.
const packagePath = resolve(dirname(fileURLToPath(import.meta.url)), "my-cmds");

export default defineSoftware({ packagePath });
