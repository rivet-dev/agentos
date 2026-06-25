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

		const result = await vm.execArgv("node", [
			"-e",
			"console.log('should not run')",
			"this-argument-is-too-long",
		]).then(
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
			expect(String(result.error.message)).toMatch(/argv|argument|limit|too large/i);
		}
	});
});
