/**
 * Local stand-in for the `@agentos-software/common` software bundle.
 *
 * In a real app you would `import common from "@agentos-software/common"`; this
 * fixture is self-contained, so it provides an equivalently-shaped WASM-command
 * descriptor to exercise the `software: [...]` config field.
 */
import { resolve } from "node:path";
import { defineSoftware } from "@rivet-dev/agentos";

const common = defineSoftware({
	name: "common",
	type: "wasm-commands",
	commandDir: resolve(import.meta.dirname, "wasm"),
});

export default common;
