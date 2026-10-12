import { afterEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

describe("AgentOs limits", () => {
	let vm: AgentOs | undefined;

	afterEach(async () => {
		await vm?.dispose();
		vm = undefined;
	});

	test("maxProcessArgvBytes is forwarded to the VM process runtime", async () => {
		vm = await AgentOs.create({
			defaultSoftware: false,
			limits: {
				resources: {
					maxProcessArgvBytes: 16,
				},
			},
		});

		const result = await vm
			.execArgv("node", [
				"-e",
				"console.log('should not run')",
				"this-argument-is-too-long",
			])
			.then(
				(value) => ({ type: "result" as const, value }),
				(error) => ({ type: "error" as const, error }),
			);

		if (result.type === "result") {
			expect(result.value.exitCode).not.toBe(0);
			expect(`${result.value.stderr}\n${result.value.stdout}`).toMatch(
				/argv|argument|limit|too large/i,
			);
		} else {
			expect(result.error).toBeInstanceOf(Error);
			expect(String(result.error.message)).toMatch(
				/argv|argument|limit|too large/i,
			);
		}
	});

	test("reactor.operationDeadlineMs bounds an undrained socket write", async () => {
		vm = await AgentOs.create({
			defaultSoftware: false,
			limits: { reactor: { operationDeadlineMs: 500 } },
		});
		const result = await vm.javascript.execute(STALLED_REACTOR_WRITE, {
			cwd: "/",
			output: { capture: "all" },
		});
		expect(result.exitCode).toBe(0);
		const error = JSON.parse(result.stdout.trim());
		expect(error.code).toBe("ETIMEDOUT");
		expect(error.message).toContain("limits.reactor.operationDeadlineMs");
	}, 15_000);
});

// The paused reader leaves the bounded VM socket buffer full until its configured deadline.
const STALLED_REACTOR_WRITE = `
const net = require('node:net');
const server = net.createServer(socket => socket.pause());
server.listen(0, '127.0.0.1', () => {
  const client = net.connect(server.address().port, '127.0.0.1', () => {
    client.write(Buffer.alloc(6 * 1024 * 1024));
  });
  client.on('error', error => {
    console.log(JSON.stringify({code: error.code, message: error.message}));
    process.exit(0);
  });
});
`;
