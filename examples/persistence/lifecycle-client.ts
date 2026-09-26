import { vm } from "./client.js";

const conn = vm.connect();
await conn.ready;

conn.on("vm.booted", () => {
	console.log("VM is ready");
});

conn.on("vm.shutdown", (payload) => {
	console.log("VM shutdown reason:", payload.reason);
	// reason: "sleep" | "destroy" | "error"
});

// The VM starts lazily on the first agentOS action after wake, after the
// lifecycle subscriptions above are active.
await conn.filesystem.exists({ path: "/" });
