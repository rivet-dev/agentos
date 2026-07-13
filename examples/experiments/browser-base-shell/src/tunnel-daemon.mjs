import { spawn } from "node:child_process";
import {
	mkdirSync,
	readFileSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { dirname } from "node:path";

const MAX_LOG_BYTES = 64 * 1024;
const [portValue, statePath, logPath] = process.argv.slice(2);
const port = Number(portValue);

if (!Number.isInteger(port) || port < 1 || port > 65_535 || !statePath || !logPath) {
	throw new Error("usage: tunnel-daemon.mjs <port> <state-path> <log-path>");
}

mkdirSync(dirname(statePath), { recursive: true });
let output = "";
writeFileSync(logPath, output);

function appendLog(chunk) {
	output = (output + String(chunk)).slice(-MAX_LOG_BYTES);
	writeFileSync(logPath, output);
}

function removeOwnedState() {
	try {
		const state = JSON.parse(readFileSync(statePath, "utf8"));
		if (state.supervisorPid === process.pid) rmSync(statePath, { force: true });
	} catch {
		// A missing or incomplete state file needs no cleanup.
	}
}

const cloudflared = spawn(
	"cloudflared",
	[
		"tunnel",
		"--url",
		`http://127.0.0.1:${port}`,
		"--no-autoupdate",
		"--metrics",
		"127.0.0.1:0",
		"--transport-loglevel",
		"warn",
	],
	{ stdio: ["ignore", "pipe", "pipe"] },
);

let stateWritten = false;
for (const stream of [cloudflared.stdout, cloudflared.stderr]) {
	stream.setEncoding("utf8");
	stream.on("data", (chunk) => {
		appendLog(chunk);
		if (stateWritten) return;
		const match = output.match(/https:\/\/[a-z0-9-]+\.trycloudflare\.com/i);
		if (!match) return;
		stateWritten = true;
		writeFileSync(
			statePath,
			JSON.stringify(
				{
					version: 1,
					supervisorPid: process.pid,
					cloudflaredPid: cloudflared.pid,
					port,
					url: match[0],
					startedAt: new Date().toISOString(),
				},
				null,
				2,
			),
		);
	});
}

cloudflared.once("error", (error) => appendLog(`${error.stack ?? error}\n`));
cloudflared.once("exit", (code, signal) => {
	appendLog(`cloudflared exited code=${String(code)} signal=${String(signal)}\n`);
	removeOwnedState();
	process.exitCode = code ?? 1;
});
