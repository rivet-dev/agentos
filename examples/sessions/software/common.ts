import { join } from "node:path";

/**
 * Local stand-in for the `@agentos-software/common` software bundle.
 *
 * In a real app you would `import common from "@agentos-software/common"`; this
 * fixture is self-contained, so it provides an equivalently-shaped WASM command
 * descriptor to exercise the `software: [...]` config field. Any object with a
 * `commandDir` property is treated as a WASM command source.
 */
const common = {
	name: "common",
	commandDir: join(import.meta.dirname, "wasm"),
} as const;

export default common;
