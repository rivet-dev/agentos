import {
	persistentTunnelPaths,
	persistentTunnelStatus,
	stopPersistentTunnel,
} from "./tunnel.js";

const action = process.argv[2] ?? "status";

if (action === "stop") {
	const stopped = await stopPersistentTunnel();
	process.stdout.write(
		stopped
			? "stopped persistent cloudflared tunnel\n"
			: "no persistent cloudflared tunnel is running\n",
	);
} else if (action === "status") {
	const state = persistentTunnelStatus();
	process.stdout.write(
		state
			? `${JSON.stringify({ ...state, logPath: persistentTunnelPaths.logPath }, null, 2)}\n`
			: "no persistent cloudflared tunnel is running\n",
	);
} else {
	throw new Error(`unknown tunnel action: ${action}`);
}
