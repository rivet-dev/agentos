/**
 * Local stand-in for the `@agentos-software/common` software bundle.
 *
 * In a real app you would `import common from "@agentos-software/common"`; this
 * fixture is self-contained, so it provides an equivalently-shaped default
 * export to exercise the `software: [...]` config field. A WASM command bundle
 * is any object carrying a `commandDir` pointing at the directory of command
 * binaries on the host.
 */
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const commandDir = resolve(
	dirname(fileURLToPath(import.meta.url)),
	"commands",
);

const common = {
	name: "common",
	commandDir,
} as const;

export default common;
