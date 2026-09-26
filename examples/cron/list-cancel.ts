import { vm } from "./client.js";

// List all cron jobs
const jobs = await vm.cron.list({});
for (const job of jobs) {
	console.log(job.name, job.expression);
}

// Cancel a specific job
const first = jobs[0];
if (first) await vm.cron.cancel({ name: first.name });
