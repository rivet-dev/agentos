// Standalone script (as a user would write): create a VM on the default
// (shared) sidecar, do one op, dispose, and DO NOT call process.exit().
//
// A correct dispose() must let node exit on its own — the shared sidecar's
// child process + stdio handles must not keep the event loop alive after the
// last VM lease is released. Imports the built package entry, like a consumer.
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const entry = pathToFileURL(
	resolve(import.meta.dirname, "../../dist/index.js"),
).href;
const { AgentOs } = await import(entry);

const vm = await AgentOs.create();
await vm.writeFile("/clean-exit.txt", "ok");
await vm.dispose();

console.log("SCRIPT_DONE");
// Intentionally NO process.exit(): the process must terminate on its own.
