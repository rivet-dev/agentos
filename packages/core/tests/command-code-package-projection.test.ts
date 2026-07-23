import commandCode from "@agentos-software/command-code";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";

describe("Command Code package projection", () => {
	let vm: AgentOs | undefined;

	beforeEach(async () => {
		vm = await AgentOs.create({
			defaultSoftware: false,
			software: [commandCode],
		});
	}, 120_000);

	afterEach(async () => {
		await vm?.dispose();
	});

	test("projects the genuine v1 CLI without registering an ACP agent", async () => {
		if (!vm) throw new Error("VM setup failed");
		for (const command of ["cmd", "cmdc", "command-code", "commandcode"]) {
			expect(await vm.exists(`/opt/agentos/bin/${command}`)).toBe(true);
		}
		expect(await vm.exists("/opt/agentos/bin/command-code-acp")).toBe(false);
		expect(await vm.listAgents()).not.toEqual(
			expect.arrayContaining([expect.objectContaining({ id: "command-code" })]),
		);

		const version = await vm.execArgv("cmd", ["--version"]);
		expect(version.exitCode, version.stderr).toBe(0);
		expect(version.stdout.trim()).toBe("1.1.0");

		const help = await vm.execArgv("cmd", ["--help"]);
		expect(help.exitCode, help.stderr).toBe(0);
		expect(help.stdout).toContain("Command Code v1.1.0");
		expect(help.stdout).toContain("--output-format <format>");
		expect(help.stdout).toContain("mcp");
		expect(help.stdout).toContain("skills");
		expect(help.stdout).not.toMatch(/(^|\n)undefined(\n|$)/);

		const processExit = await vm.execArgv("node", [
			"-e",
			"try { process.exit(23) } catch { console.log('caught') } finally { console.log('finally') }",
		]);
		expect(processExit.exitCode).toBe(23);
		expect(processExit.stdout).toBe("");

		const asyncProcessExit = await vm.execArgv("node", [
			"-e",
			"setTimeout(() => process.exit(24), 0); setTimeout(() => console.log('late'), 1000)",
		]);
		expect(asyncProcessExit.exitCode, asyncProcessExit.stderr).toBe(24);
		expect(asyncProcessExit.stdout).toBe("");
	}, 120_000);

	test("runs status, skills, MCP configuration, and headless auth handling", async () => {
		if (!vm) throw new Error("VM setup failed");
		const home = "/home/agentos-command-code";
		const workspace = `${home}/workspace`;
		await vm.mkdir(workspace, { recursive: true });
		await vm.mkdir(`${workspace}/.agents/skills/demo`, { recursive: true });
		await vm.writeFile(
			`${workspace}/.agents/skills/demo/SKILL.md`,
			"---\nname: demo\ndescription: Test skill discovery.\n---\n\n# Demo\n",
		);
		const options = {
			cwd: workspace,
			env: {
				COMMANDCODE_SKIP_UPDATES: "1",
				DO_NOT_TRACK: "1",
				HOME: home,
			},
		};

		const status = await vm.execArgv("cmd", ["status", "--json"], options);
		expect(status.exitCode, status.stderr).toBe(1);
		expect(JSON.parse(status.stdout)).toMatchObject({
			authenticated: false,
			version: "1.1.0",
		});

		const skills = await vm.execArgv(
			"cmd",
			["skills", "list", "--debug"],
			options,
		);
		expect(skills.exitCode, skills.stderr).toBe(0);
		expect(skills.stdout).toContain("demo");

		const mcpAdd = await vm.execArgv(
			"cmd",
			[
				"mcp",
				"add",
				"--transport",
				"http",
				"--scope",
				"project",
				"demo",
				"https://example.com/mcp",
			],
			options,
		);
		expect(mcpAdd.exitCode, mcpAdd.stderr).toBe(0);
		const mcpGet = await vm.execArgv("cmd", ["mcp", "get", "demo"], options);
		expect(mcpGet.exitCode, mcpGet.stderr).toBe(0);
		expect(mcpGet.stdout).toContain("https://example.com/mcp");
		expect(
			JSON.parse(
				new TextDecoder().decode(await vm.readFile(`${workspace}/.mcp.json`)),
			),
		).toMatchObject({
			mcpServers: {
				demo: {
					transport: "http",
					url: "https://example.com/mcp",
				},
			},
		});

		const headless = await vm.execArgv(
			"cmd",
			[
				"-p",
				"hello",
				"--output-format",
				"json",
				"--trust",
				"--skip-onboarding",
				"--no-auto-update",
			],
			options,
		);
		expect(headless.exitCode, headless.stderr).toBe(3);
		expect(headless.stdout).toContain("Not authenticated");
	}, 120_000);

	test.runIf(process.env.AGENTOS_RUN_COMMAND_CODE_AUTH_TEST === "1")(
		"authenticates an API key and reaches the hosted model boundary",
		async () => {
			if (!vm) throw new Error("VM setup failed");
			const apiKey = process.env.COMMAND_CODE_API_KEY;
			if (!apiKey) throw new Error("COMMAND_CODE_API_KEY is required");
			const home = "/home/agentos-command-code-authenticated";
			const workspace = `${home}/workspace`;
			await vm.mkdir(workspace, { recursive: true });
			const options = {
				cwd: workspace,
				env: {
					COMMAND_CODE_API_KEY: apiKey,
					COMMANDCODE_SKIP_UPDATES: "1",
					DO_NOT_TRACK: "1",
					HOME: home,
				},
			};
			const status = await vm.execArgv("cmd", ["status", "--json"], options);
			expect(status.exitCode, status.stderr).toBe(0);
			expect(JSON.parse(status.stdout)).toMatchObject({ authenticated: true });

			const result = await vm.execArgv(
				"cmd",
				[
					"-p",
					"Reply with exactly COMMAND_CODE_AUTH_OK and no other text. Do not use tools.",
					"--output-format",
					"json",
					"--trust",
					"--skip-onboarding",
					"--no-auto-update",
					"--max-turns",
					"1",
				],
				options,
			);
			if (result.exitCode === 0) {
				expect(result.stdout).toContain("COMMAND_CODE_AUTH_OK");
			} else {
				expect(result.exitCode).toBe(10);
				expect(result.stderr || result.stdout).toContain(
					"insufficient credits",
				);
			}
		},
		120_000,
	);
});
