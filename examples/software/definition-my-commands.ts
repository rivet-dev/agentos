import { defineSoftware } from "@rivet-dev/agentos";

export default defineSoftware({
	name: "my-commands",
	type: "wasm-commands",
	commandDir: "/abs/path/to/wasm",
	aliases: { ll: "ls" },
	permissions: { readOnly: "*" },
});
