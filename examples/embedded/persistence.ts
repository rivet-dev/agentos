import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { AgentOs, type AgentOsOptions } from "@rivet-dev/agentos";

mkdirSync(".agentos", { recursive: true });
const options = {
	// The SQLite file that stores the VM's durable state.
	database: {
		type: "sqlite_file",
		path: resolve(".agentos/agentos.sqlite"),
	},
	// Store the root filesystem in that database instead of in memory.
	rootFilesystem: {
		type: "native",
		plugin: { id: "chunked_sqlite", config: {} },
	},
} satisfies AgentOsOptions;

let vm = await AgentOs.create(options);
await vm.filesystem.writeFile("/home/agentos/result.txt", "persistent");
await vm.dispose();

vm = await AgentOs.create(options);
const result = await vm.filesystem.readFile("/home/agentos/result.txt");
console.log(new TextDecoder().decode(result));
await vm.dispose();
