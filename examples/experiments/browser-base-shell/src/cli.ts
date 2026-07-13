import { BrowserbaseShellSession } from "./session.js";

let session: BrowserbaseShellSession | undefined;
let closing = false;
let writeQueue = Promise.resolve();
const stdinWasRaw = process.stdin.isRaw;

async function close(exitCode = 0): Promise<void> {
	if (closing) return;
	closing = true;
	process.stdin.pause();
	if (process.stdin.isTTY) process.stdin.setRawMode(stdinWasRaw ?? false);
	await writeQueue.catch((error) => {
		console.error("terminal input failed", error);
		exitCode = 1;
	});
	await session?.close();
	process.stdout.write("\r\nBrowserbase shell closed.\r\n");
	process.exitCode = exitCode;
}

process.once("SIGTERM", () => void close(143));
process.once("SIGHUP", () => void close(129));
process.once("uncaughtException", (error) => {
	console.error(error);
	void close(1);
});
process.once("unhandledRejection", (error) => {
	console.error(error);
	void close(1);
});

try {
	process.stdout.write("Starting AgentOS browser shell...\r\n");
	session = await BrowserbaseShellSession.open({
		onStatus: (message) => process.stdout.write(`[host] ${message}\r\n`),
		onEvent: (event) => {
			if (event.kind === "pty") process.stdout.write(event.bytes);
			else if (event.kind === "status") {
				process.stdout.write(
					`\r\n[CDP ${event.mode}] ${event.bytes.toString().trim()}\r\n`,
				);
			} else {
				process.stderr.write(`\r\n[CDP error] ${event.bytes.toString()}\r\n`);
			}
		},
	});
	process.stdout.write(
		`\r\ntransport=CDP runtime=AgentOS-browser session=${session.sessionId}\r\n` +
			`Browserbase replay: ${session.sessionUrl}\r\n` +
			"Exit with Ctrl-]. Ctrl-C is forwarded to the browser PTY.\r\n",
	);
	if (process.stdin.isTTY) process.stdin.setRawMode(true);
	process.stdin.resume();
	process.stdin.on("data", (chunk: Buffer) => {
		const exitIndex = chunk.indexOf(0x1d);
		const forwarded = exitIndex === -1 ? chunk : chunk.subarray(0, exitIndex);
		const activeSession = session;
		if (forwarded.length > 0 && activeSession) {
			writeQueue = writeQueue.then(() => activeSession.write(forwarded));
			writeQueue.catch((error) => {
				console.error("terminal input failed", error);
				void close(1);
			});
		}
		if (exitIndex !== -1) void close();
	});
	process.stdin.once("end", () => void close());
} catch (error) {
	console.error(error);
	await close(1);
}
