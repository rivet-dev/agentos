import { defineSoftware } from "@rivet-dev/agentos";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");

export default defineSoftware({
	name: "my-tool",
	type: "tool",
	packageDir,
	requires: ["@my-org/my-cli"],
	bins: { "my-cli": "@my-org/my-cli" },
});
