import type { AgentOsActorHandle } from "@rivet-dev/agentos";

// The caller supplies a unique name and retains ownership of the VM and client.
export async function observeHeartbeat(vm: AgentOsActorHandle, name: string) {
	const connection = vm.connect();
	let scheduled = false;
	try {
		connection.on("cron.fired", (event) => {
			if (event.scheduleName === name) console.log(event);
		});
		// Complete a connected action before scheduling so subscription is ready.
		await connection.vm.status({});
		await connection.cron.schedule({
			name,
			expression: "* * * * *",
			command: "echo",
			args: ["heartbeat"],
			maxHistory: 10,
		});
		scheduled = true;
		console.log(await connection.cron.list({}));
		// This bounded observation window may end before the next minute fires.
		await new Promise<void>((resolve) => setTimeout(resolve, 5_000));
	} finally {
		try {
			if (scheduled) await vm.cron.cancel({ name });
		} finally {
			await connection.dispose();
		}
	}
}
