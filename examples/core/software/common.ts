/**
 * Local stand-in for the `@agentos-software/common` software bundle.
 *
 * In a real app you would `import common from "@agentos-software/common"`; this
 * fixture is self-contained, so it provides an equivalently-shaped default
 * export (a wasm-commands `SoftwareDescriptor`) to exercise the
 * `software: [...]` config field.
 */
import { defineSoftware } from "@rivet-dev/agentos";

const common = defineSoftware({
	name: "common",
	type: "wasm-commands",
	commandDir: "/path/to/@agentos-software/common/wasm",
});

export default common;
