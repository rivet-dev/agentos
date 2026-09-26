import { vm } from "./client.js";

// docs:start subscribe
const conn = vm.connect();
await conn.ready;
conn.on("cron.fired", (event) => {
	console.log("Cron event:", event);
});
// docs:end subscribe

await conn.cron.schedule({
	name: "heartbeat-monitor",
	expression: "*/1 * * * *",
	command: "echo",
	args: ["heartbeat"],
	options: { env: {} },
});
