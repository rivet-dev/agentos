import { vm } from "./client.js";

// docs:start heartbeat
await vm.cron.schedule({
	name: "heartbeat",
	expression: "*/30 * * * *",
	command: "sh",
	args: ["-lc", "printf '%s\\n' heartbeat"],
	options: { env: {} },
});
// docs:end heartbeat
