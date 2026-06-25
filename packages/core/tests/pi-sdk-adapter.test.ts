import { resolve } from "node:path";
import pi from "@agentos-software/pi";
import { moduleAccessMounts } from "./helpers/node-modules-mount.js";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";

const MODULE_ACCESS_CWD = resolve(
	import.meta.dirname,
	"../../../examples/quickstart/hello-world",
);

describe("pi-sdk software projection", () => {
	let vm: AgentOs;

	beforeEach(async () => {
		vm = await AgentOs.create({
			mounts: moduleAccessMounts(MODULE_ACCESS_CWD),
			software: [pi],
		});
	});

	afterEach(async () => {
		await vm.dispose();
	});

	test("projects the SDK adapter package and PI agent package into the VM", async () => {
		const script = `
const fs = require("fs");
console.log("adapter:" + fs.existsSync("/root/node_modules/@agentos-software/pi/package.json"));
console.log("agent:" + fs.existsSync("/root/node_modules/@mariozechner/pi-coding-agent/package.json"));
`;
		await vm.writeFile("/tmp/pi-sdk-projection.mjs", script);

		let stdout = "";
		let stderr = "";

		const { pid } = vm.spawn("node", ["/tmp/pi-sdk-projection.mjs"], {
			onStdout: (data: Uint8Array) => {
				stdout += new TextDecoder().decode(data);
			},
			onStderr: (data: Uint8Array) => {
				stderr += new TextDecoder().decode(data);
			},
		});

		const exitCode = await vm.waitProcess(pid);

		expect(exitCode, `Projection probe failed. stderr: ${stderr}`).toBe(0);
		expect(stdout).toContain("adapter:true");
		expect(stdout).toContain("agent:true");
	});
});
