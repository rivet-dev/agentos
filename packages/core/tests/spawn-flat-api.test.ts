import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

describe("flat spawn API", () => {
	let vm: AgentOs;

	beforeEach(async () => {
		vm = await AgentOs.create();
	});

	afterEach(async () => {
		await vm.dispose();
	});

	test("onProcessOutput captures stderr, onProcessExit fires with exit code", async () => {
		await vm.writeFile(
			"/tmp/stderr-exit.mjs",
			'process.stderr.write("err-data\\n"); process.exit(42);',
		);

		const { pid } = vm.spawn("node", ["/tmp/stderr-exit.mjs"], {
			env: { HOME: "/home/agentos" },
		});

		const stderrChunks: string[] = [];
		vm.onProcessOutput(pid, (event) => {
			if (event.stream === "stderr") {
				stderrChunks.push(new TextDecoder().decode(event.data));
			}
		});

		const exitCodePromise = new Promise<number>((resolve) => {
			vm.onProcessExit(pid, (event) => resolve(event.exitCode));
		});

		const exitCode = await exitCodePromise;
		expect(exitCode).toBe(42);
		expect(stderrChunks.join("")).toContain("err-data");
	}, 30_000);

	test("spawn returns { pid }, writeProcessStdin sends data, onProcessOutput receives it", async () => {
		await vm.writeFile(
			"/tmp/echo-stdin.mjs",
			`process.stdin.on("data", (chunk) => process.stdout.write(chunk));`,
		);

		const { pid } = vm.spawn("node", ["/tmp/echo-stdin.mjs"], {
			streamStdin: true,
			env: { HOME: "/home/agentos" },
		});

		const chunks: string[] = [];
		const expectedOutput = "hello from flat api";
		const stdoutReceived = new Promise<void>((resolve, reject) => {
			const timeout = setTimeout(() => {
				reject(new Error("Timed out waiting for spawned stdout"));
			}, 5_000);

			vm.onProcessOutput(pid, (event) => {
				if (event.stream !== "stdout") return;
				chunks.push(new TextDecoder().decode(event.data));
				if (chunks.join("").includes(expectedOutput)) {
					clearTimeout(timeout);
					resolve();
				}
			});
		});

		vm.writeProcessStdin(pid, "hello from flat api\n");

		await stdoutReceived;

		vm.killProcess(pid);
		await vm.waitProcess(pid);

		expect(chunks.join("")).toContain(expectedOutput);
	}, 30_000);
});
