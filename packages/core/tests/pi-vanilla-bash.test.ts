import { resolve } from "node:path";
import type { Fixture, ToolCall } from "@copilotkit/llmock";
import common from "@rivet-dev/agent-os-common";
import pi from "@rivet-dev/agent-os-pi";
import { describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import {
	createAnthropicFixture,
	startLlmock,
	stopLlmock,
} from "./helpers/llmock-helper.js";
import {
	hasRegistryCommands,
	registrySkipReason,
} from "./helpers/registry-commands.js";

const MODULE_ACCESS_CWD = resolve(import.meta.dirname, "..");

function getRequestBody(req: unknown): Record<string, unknown> {
	const direct = req as Record<string, unknown>;
	const body = direct.body;
	return body && typeof body === "object"
		? (body as Record<string, unknown>)
		: direct;
}

/**
 * Two-turn fixture: the first model turn (no tool result in the request) emits
 * the bash tool call; the second turn (the request now carries the tool result)
 * returns the final assistant text.
 */
function createBashFixtures(toolCall: ToolCall, finalText: string): Fixture[] {
	return [
		createAnthropicFixture(
			{
				predicate: (req) =>
					!JSON.stringify(getRequestBody(req)).includes('"role":"tool"'),
			},
			{ toolCalls: [toolCall] },
		),
		createAnthropicFixture(
			{
				predicate: (req) =>
					JSON.stringify(getRequestBody(req)).includes('"role":"tool"'),
			},
			{ content: finalText },
		),
	];
}

function bashToolCall(args: Record<string, unknown>): ToolCall {
	return {
		name: "bash",
		arguments: JSON.stringify(args),
	};
}

async function createPiVm(mockUrl: string): Promise<AgentOs> {
	return AgentOs.create({
		loopbackExemptPorts: [Number(new URL(mockUrl).port)],
		moduleAccessCwd: MODULE_ACCESS_CWD,
		software: [common, pi],
	});
}

async function createVmPiHome(vm: AgentOs, mockUrl: string): Promise<string> {
	const homeDir = "/home/user";
	await vm.mkdir(`${homeDir}/.pi/agent`, { recursive: true });
	await vm.writeFile(
		`${homeDir}/.pi/agent/models.json`,
		JSON.stringify(
			{
				providers: {
					anthropic: {
						baseUrl: mockUrl,
						apiKey: "mock-key",
					},
				},
			},
			null,
			2,
		),
	);
	return homeDir;
}

async function createVmWorkspace(vm: AgentOs): Promise<string> {
	const workspaceDir = "/home/user/workspace";
	await vm.mkdir(workspaceDir, { recursive: true });
	return workspaceDir;
}

function sessionEventText(vm: AgentOs, sessionId: string): string {
	return vm
		.getSessionEvents(sessionId)
		.map((event) => JSON.stringify(event.notification.params))
		.join("\n");
}

/**
 * Vanilla Pi bash coverage: these tests use the unmodified Pi SDK bash backend
 * (`createLocalBashOperations()` spawning the shell directly with
 * `detached: true` and streaming stdout/stderr), with no custom `operations`
 * override in the adapter. Everything stays inside the VM.
 *
 * The file-write, timeout, and abort cases depend on runtime behavior that is
 * still outstanding below the adapter layer (shell `>` redirect visibility
 * through `vm.readFile`, and a blocking guest `sleep`). They are tracked in
 * `~/.agents/todo/agent-os-runtime-fixes.md` and registered as skipped
 * placeholders here so the file documents the full vanilla contract without
 * asserting behavior the runtime cannot yet deliver.
 */
describe("vanilla Pi bash tool inside the VM", () => {
	if (!hasRegistryCommands) {
		test.skip(`skipped: ${registrySkipReason}`, () => {});
		return;
	}

	test("runs the vanilla bash backend in the session working directory", async () => {
		const fixtures = createBashFixtures(
			bashToolCall({ command: "pwd", timeout: 10 }),
			"reported the directory.",
		);
		const { mock, url } = await startLlmock(fixtures);
		const vm = await createPiVm(url);

		let sessionId: string | undefined;
		try {
			const homeDir = await createVmPiHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			sessionId = (
				await vm.createSession("pi", {
					cwd: workspaceDir,
					env: {
						HOME: homeDir,
						ANTHROPIC_API_KEY: "mock-key",
						ANTHROPIC_BASE_URL: url,
					},
				})
			).sessionId;

			const { response } = await vm.prompt(sessionId, "Run pwd.");
			expect(response.error).toBeUndefined();
			expect(sessionEventText(vm, sessionId)).toContain(workspaceDir);
		} finally {
			if (sessionId) {
				vm.closeSession(sessionId);
			}
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 120_000);

	test("inherits session env in the spawned shell", async () => {
		const fixtures = createBashFixtures(
			bashToolCall({ command: "echo $AGENTOS_TEST_FLAG", timeout: 10 }),
			"reported the flag.",
		);
		const { mock, url } = await startLlmock(fixtures);
		const vm = await createPiVm(url);

		let sessionId: string | undefined;
		try {
			const homeDir = await createVmPiHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			sessionId = (
				await vm.createSession("pi", {
					cwd: workspaceDir,
					env: {
						HOME: homeDir,
						ANTHROPIC_API_KEY: "mock-key",
						ANTHROPIC_BASE_URL: url,
						AGENTOS_TEST_FLAG: "vanilla",
					},
				})
			).sessionId;

			const { response } = await vm.prompt(
				sessionId,
				"Echo the AGENTOS_TEST_FLAG variable.",
			);
			expect(response.error).toBeUndefined();
			expect(sessionEventText(vm, sessionId)).toContain("vanilla");
		} finally {
			if (sessionId) {
				vm.closeSession(sessionId);
			}
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 120_000);

	test("captures stdout, stderr, and the nonzero exit code", async () => {
		const fixtures = createBashFixtures(
			bashToolCall({
				command: "printf 'out-line\\n'; printf 'err-line\\n' 1>&2; exit 3",
				timeout: 10,
			}),
			"the command failed.",
		);
		const { mock, url } = await startLlmock(fixtures);
		const vm = await createPiVm(url);

		let sessionId: string | undefined;
		try {
			const homeDir = await createVmPiHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			sessionId = (
				await vm.createSession("pi", {
					cwd: workspaceDir,
					env: {
						HOME: homeDir,
						ANTHROPIC_API_KEY: "mock-key",
						ANTHROPIC_BASE_URL: url,
					},
				})
			).sessionId;

			const { response } = await vm.prompt(
				sessionId,
				"Run a command that writes to stdout and stderr and exits nonzero.",
			);
			expect(response.error).toBeUndefined();
			const events = sessionEventText(vm, sessionId);
			expect(events).toContain("out-line");
			expect(events).toContain("err-line");
			expect(events).toContain("3");
		} finally {
			if (sessionId) {
				vm.closeSession(sessionId);
			}
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 120_000);

	// Blocked on shell `>` redirect output being visible to `vm.readFile()`.
	// The redirect runs inside the guest shell but the written bytes do not
	// reconcile to the host read path yet. Tracked in
	// ~/.agents/todo/agent-os-runtime-fixes.md (shell-exec redirect visibility).
	test.skip("writes a file through the default bash backend", async () => {
		const fixtures = createBashFixtures(
			bashToolCall({ command: "printf 'ok' > out.txt", timeout: 10 }),
			"out.txt was written.",
		);
		const { mock, url } = await startLlmock(fixtures);
		const vm = await createPiVm(url);

		let sessionId: string | undefined;
		try {
			const homeDir = await createVmPiHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			sessionId = (
				await vm.createSession("pi", {
					cwd: workspaceDir,
					env: {
						HOME: homeDir,
						ANTHROPIC_API_KEY: "mock-key",
						ANTHROPIC_BASE_URL: url,
					},
				})
			).sessionId;

			const { response } = await vm.prompt(
				sessionId,
				"Use bash to write ok into out.txt.",
			);
			expect(response.error).toBeUndefined();
			expect(
				new TextDecoder().decode(
					await vm.readFile(`${workspaceDir}/out.txt`),
				),
			).toBe("ok");
		} finally {
			if (sessionId) {
				vm.closeSession(sessionId);
			}
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 120_000);

	// Blocked on a blocking guest `sleep`. The WASM `sleep` command currently
	// fails to spawn ("operation not supported on this platform") because the
	// host `sleep_ms` WASI import is unimplemented, so the timeout/kill path
	// cannot be exercised. Tracked in ~/.agents/todo/agent-os-runtime-fixes.md.
	test.skip("enforces the bash timeout by killing the process tree", async () => {
		const fixtures = createBashFixtures(
			bashToolCall({ command: "sleep 30", timeout: 1 }),
			"the command timed out.",
		);
		const { mock, url } = await startLlmock(fixtures);
		const vm = await createPiVm(url);

		let sessionId: string | undefined;
		const startedAt = Date.now();
		try {
			const homeDir = await createVmPiHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			sessionId = (
				await vm.createSession("pi", {
					cwd: workspaceDir,
					env: {
						HOME: homeDir,
						ANTHROPIC_API_KEY: "mock-key",
						ANTHROPIC_BASE_URL: url,
					},
				})
			).sessionId;

			const { response } = await vm.prompt(
				sessionId,
				"Run sleep 30 with a 1 second timeout.",
			);
			expect(response.error).toBeUndefined();
			// The kill must actually fire: completing in seconds (not ~30s) proves
			// the timeout killed the sleep instead of waiting for it to finish.
			expect(Date.now() - startedAt).toBeLessThan(20_000);
			expect(sessionEventText(vm, sessionId).toLowerCase()).toContain(
				"timed out",
			);
		} finally {
			if (sessionId) {
				vm.closeSession(sessionId);
			}
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 60_000);

	// Blocked on the same blocking-guest-`sleep` gap as the timeout case: the
	// in-flight bash command exits immediately instead of staying running, so
	// the cancel-while-in-progress path cannot be observed. Tracked in
	// ~/.agents/todo/agent-os-runtime-fixes.md.
	test.skip("aborts an in-flight bash command on session cancel", async () => {
		const fixtures: Fixture[] = [
			createAnthropicFixture(
				{
					predicate: (req) =>
						!JSON.stringify(getRequestBody(req)).includes('"role":"tool"'),
				},
				{ toolCalls: [bashToolCall({ command: "sleep 60", timeout: 120 })] },
			),
		];
		const { mock, url } = await startLlmock(fixtures);
		const vm = await createPiVm(url);

		let sessionId: string | undefined;
		try {
			const homeDir = await createVmPiHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			sessionId = (
				await vm.createSession("pi", {
					cwd: workspaceDir,
					env: {
						HOME: homeDir,
						ANTHROPIC_API_KEY: "mock-key",
						ANTHROPIC_BASE_URL: url,
					},
				})
			).sessionId;

			const activeSessionId = sessionId;
			const sawInProgress = new Promise<void>((resolveInProgress) => {
				const unsubscribe = vm.onSessionEvent(activeSessionId, (event) => {
					const serialized = JSON.stringify(event.notification.params);
					if (
						serialized.includes('"in_progress"') &&
						serialized.includes("bash")
					) {
						unsubscribe();
						resolveInProgress();
					}
				});
			});

			const promptPromise = vm.prompt(activeSessionId, "Run sleep 60 in bash.");

			await sawInProgress;
			await vm.cancelSession(activeSessionId);

			const { response } = await promptPromise;
			const stopReason = (response.result as { stopReason?: string })
				?.stopReason;
			expect(stopReason).toBe("cancelled");

			const lingering = vm
				.allProcesses()
				.filter(
					(proc) =>
						proc.status === "running" &&
						(proc.command.includes("sleep") ||
							proc.args.some((arg) => arg.includes("sleep"))),
				);
			expect(lingering).toEqual([]);
		} finally {
			if (sessionId) {
				vm.closeSession(sessionId);
			}
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 60_000);
});
