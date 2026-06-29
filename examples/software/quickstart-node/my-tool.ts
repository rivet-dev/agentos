import { defineSoftware } from "@rivet-dev/agentos";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Point at the self-contained directory produced by `agentos-toolchain pack`.
const packageDir = resolve(
	dirname(fileURLToPath(import.meta.url)),
	"my-tool-package",
);

export default defineSoftware({ packageDir });
