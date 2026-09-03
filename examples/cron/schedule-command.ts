import { vm } from "./client.js";

// Schedule a cleanup script every hour
const job = await vm.cron.schedule({
	name: "cleanup",
	expression: "0 * * * *",
	command: "rm",
	args: ["-rf", "/tmp/cache"],
	options: { env: {} },
});
console.log("Cron job name:", job.name);
