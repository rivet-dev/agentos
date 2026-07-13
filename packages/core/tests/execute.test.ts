import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";
import { REGISTRY_SOFTWARE } from "./helpers/registry-commands.js";

describe("command execution", () => {
	let vm: AgentOs;

	beforeEach(async () => {
		vm = await AgentOs.create({ software: REGISTRY_SOFTWARE });
	});

	afterEach(async () => {
		await vm.dispose();
	});

	test("exec returns stdout with exit code 0", async () => {
		const result = await vm.exec("echo hello");
		expect(result.exitCode).toBe(0);
		expect(result.stdout.trim()).toBe("hello");
		expect(result.stderr).toBe("");
	});

	test("exec returns stderr and non-zero exit code", async () => {
		const result = await vm.exec("echo error >&2 && exit 1");
		expect(result.exitCode).toBe(1);
		expect(result.stderr.trim()).toBe("error");
	});

	test("exec with env vars passes them through", async () => {
		const result = await vm.exec("echo $MY_VAR", {
			env: { MY_VAR: "test-value" },
		});
		expect(result.exitCode).toBe(0);
		expect(result.stdout.trim()).toBe("test-value");
	});

	test("exec with cwd sets working directory", async () => {
		await vm.mkdir("/tmp/testdir");
		const result = await vm.exec(
			"printf found > marker.txt && cat marker.txt",
			{
				cwd: "/tmp/testdir",
			},
		);
		expect(result.exitCode).toBe(0);
		expect(result.stdout).toContain("found");
	});

	test("spawn and interact with process", async () => {
		const { pid } = await vm.spawn("cat", []);
		await vm.writeProcessStdin(pid, "hello from stdin\n");
		await vm.closeProcessStdin(pid);
		const exitCode = await vm.waitProcess(pid);
		expect(exitCode).toBe(0);
	});

	test("spawn timeout is enforced by the sidecar", async () => {
		const { pid } = await vm.spawn(
			"node",
			["-e", "setInterval(() => {}, 1000)"],
			{ timeout: 25 },
		);
		await expect(vm.waitProcess(pid)).resolves.toBe(137);
	});

	test("exec node script", async () => {
		await vm.writeFile("/tmp/test.js", 'console.log("node-output");');
		const result = await vm.exec("node /tmp/test.js");
		expect(result.exitCode).toBe(0);
		expect(result.stdout).toContain("node-output");
	});

	test("exec shell pipeline", async () => {
		for (let attempt = 0; attempt < 5; attempt += 1) {
			const result = await vm.exec("echo hello | cat");
			expect(result.exitCode, result.stderr || result.stdout).toBe(0);
			expect(result.stdout).toContain("hello");
		}
	}, 120_000);

	test("the sidecar bounds captured output without limiting raw streams", async () => {
		const limitedVm = await AgentOs.create({
			defaultSoftware: false,
			limits: { jsRuntime: { capturedOutputLimitBytes: 8 } },
		});
		try {
			await expect(
				limitedVm.execArgv("node", ["-e", "process.stdout.write('123456789')"]),
			).rejects.toMatchObject({
				code: "ERR_CAPTURED_OUTPUT_LIMIT_EXCEEDED",
				message: expect.stringContaining(
					"limits.jsRuntime.capturedOutputLimitBytes",
				),
			});

			const streamed: string[] = [];
			const uncaptured = await limitedVm.execArgv(
				"node",
				["-e", "process.stdout.write('123456789')"],
				{
					captureStdio: false,
					onStdout: (chunk) =>
						streamed.push(Buffer.from(chunk).toString("utf8")),
				},
			);
			expect(uncaptured).toEqual({ exitCode: 0, stdout: "", stderr: "" });
			expect(streamed.join("")).toBe("123456789");

			const spawned: string[] = [];
			const { pid } = await limitedVm.spawn(
				"node",
				[
					"-e",
					"process.stdin.once('data', () => process.stdout.write('123456789'))",
				],
				{ streamStdin: true },
			);
			limitedVm.onProcessStdout(pid, (chunk) => {
				spawned.push(Buffer.from(chunk).toString("utf8"));
			});
			await limitedVm.writeProcessStdin(pid, "go");
			await limitedVm.closeProcessStdin(pid);
			await expect(limitedVm.waitProcess(pid)).resolves.toBe(0);
			expect(spawned.join("")).toBe("123456789");
		} finally {
			await limitedVm.dispose();
		}
	});
});
