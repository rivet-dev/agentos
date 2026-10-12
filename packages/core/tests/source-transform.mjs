import { createRequire } from "node:module";

// Source tests run in separate Node processes. Own each compiler invocation
// synchronously so their exit cannot orphan an unreferenced esbuild service.
process.env.ESBUILD_WORKER_THREADS = "0";
const { transformSync } = createRequire(
	process.env.AGENTOS_BUILD_TOOLS_PACKAGE ?? import.meta.url,
)("esbuild");

export const transform = transformSync;
