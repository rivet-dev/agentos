import { resolve } from "node:path";
import common from "@agentos-software/common";
import pi from "@agentos-software/pi";
import type { Fixture, ToolCall } from "@copilotkit/llmock";
import { describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import {
	createAnthropicFixture,
	startLlmock,
	stopLlmock,
} from "./helpers/llmock-helper.js";
import { moduleAccessMounts } from "./helpers/node-modules-mount.js";
import { hasRegistryCommands } from "./helpers/registry-commands.js";

/**
 * REPRO / REGRESSION: "onSessionUpdate not delivered live mid-turn with Pi".
 *
 * Root cause: the secure-exec stdio loop `await`s the entire `session/prompt`
 * dispatch before flushing any frames, and `acp_extension.rs::session_request`
 * collects every `session/update` into `exchange.events`, returning them only
 * after the turn resolves. A streaming consumer (rivetkit via agentos-core)
 * therefore gets ZERO updates mid-turn and the whole batch at the end.
 *
 * Making the window observable: a zero-latency llmock collapses the whole agent
 * turn into one synchronous burst, so "live" and "batched" look identical. To
 * create a real mid-turn window we give the SECOND llmock response (the one after
 * the tool result) a `latency`. The sequence becomes:
 *
 *   t0  prompt starts
 *   ~   Pi calls the `write` tool  -> emits the `tool_call` session/update,
 *       runs the tool, then requests the final message from the LLM
 *   t1  llmock holds that 2nd response open for `RESPONSE_LATENCY_MS`
 *   t2  final message returns, turn resolves
 *
 * Between t1 and t2 the `tool_call` update already exists. With live delivery it
 * reaches `onSessionEvent` during the hold (so `resolve - firstEvent` is large,
 * and at least one update is seen before resolution). With the batching bug every
 * update arrives in a tight cluster at t2 (`resolve - firstEvent ~= 0`).
 *
 * This asserts the live contract, so it is RED on the batching bug and GREEN once
 * the live-emit path lands.
 */

const MODULE_ACCESS_CWD = resolve(import.meta.dirname, "..");
const RESPONSE_LATENCY_MS = 1500;

interface TimedEvent {
	method: string;
	params?: unknown;
	t: number;
}

function getRequestBody(req: unknown): Record<string, unknown> {
	const direct = req as Record<string, unknown>;
	const body = direct.body;
	return body && typeof body === "object"
		? (body as Record<string, unknown>)
		: direct;
}

function isPostToolResultRequest(
	req: unknown,
	expectedToolResult: string,
): boolean {
	const s = JSON.stringify(getRequestBody(req));
	return s.includes('"role":"tool"') && s.includes(expectedToolResult);
}

function isSessionUpdate(event: TimedEvent): boolean {
	return event.method === "session/update";
}

describe("REPRO: Pi session/update live delivery", () => {
	// Known-failing repro for the RivetKit native-plugin liveness bug: session/update
	// events are batched until session/prompt resolves instead of streaming live. The
	// fix lives in RivetKit and needs a republish; un-skip once that lands.
	test.skip("session/update events stream live mid-turn, not batched at prompt resolution", async () => {
		const workspacePath = "/home/agentos/workspace/tool-verify.txt";
		const expectedToolResult = "Successfully wrote";
		const finalText = "tool-verify.txt was created successfully.";

		const events: TimedEvent[] = [];
		let promptStart = 0;
		let checkpointEvents = -1; // events seen the instant the 2nd LLM req arrives

		const toolCall: ToolCall = {
			name: "write",
			arguments: JSON.stringify({
				path: workspacePath,
				content: "tool-test-ok",
			}),
		};
		const fixtures: Fixture[] = [
			createAnthropicFixture(
				{
					predicate: (req) =>
						!JSON.stringify(getRequestBody(req)).includes('"role":"tool"'),
				},
				{ toolCalls: [toolCall] },
			),
			{
				match: {
					predicate: (req) => {
						const match = isPostToolResultRequest(req, expectedToolResult);
						if (match && checkpointEvents < 0) {
							checkpointEvents = events.length;
						}
						return match;
					},
				},
				response: { content: finalText },
				// Hold the final response open so there is a real mid-turn window
				// in which the already-produced tool_call update can stream.
				latency: RESPONSE_LATENCY_MS,
			},
		];

		const { mock, url } = await startLlmock(fixtures);
		const vm = await AgentOs.create({
			loopbackExemptPorts: [Number(new URL(url).port)],
			mounts: moduleAccessMounts(MODULE_ACCESS_CWD),
			software: hasRegistryCommands ? [common, pi] : [pi],
		});

		let sessionId: string | undefined;
		try {
			const homeDir = "/home/agentos";
			await vm.mkdir(`${homeDir}/.pi/agent`, { recursive: true });
			await vm.writeFile(
				`${homeDir}/.pi/agent/models.json`,
				JSON.stringify({
					providers: { anthropic: { baseUrl: url, apiKey: "mock-key" } },
				}),
			);
			const workspaceDir = "/home/agentos/workspace";
			await vm.mkdir(workspaceDir, { recursive: true });

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

			const unsubscribe = vm.onSessionEvent(sessionId, (event) => {
				events.push({
					method: event.method,
					params: event.params,
					t: performance.now() - promptStart,
				});
			});

			promptStart = performance.now();
			const { response, text } = await vm.prompt(
				sessionId,
				"Write the text 'tool-test-ok' to tool-verify.txt. Do not explain, just do it.",
			);
			const promptResolved = performance.now() - promptStart;
			unsubscribe();

			// Sanity: the turn completed correctly and actually exercised the
			// latency hold (so the mid-turn window really existed).
			expect(response.error).toBeUndefined();
			expect(text).toContain(finalText);
			expect(mock.getRequests().length).toBeGreaterThanOrEqual(2);
			expect(
				promptResolved,
				"the turn should span the injected latency window",
			).toBeGreaterThan(RESPONSE_LATENCY_MS * 0.6);

			const updates = events.filter(isSessionUpdate);
			const firstUpdateT = updates.length ? updates[0].t : NaN;
			const updatesBeforeResolve = updates.filter(
				(e) => e.t < promptResolved - RESPONSE_LATENCY_MS * 0.4,
			).length;
			const gap = promptResolved - firstUpdateT;

			/* eslint-disable no-console */
			console.log("\n===== onSessionUpdate REPRO DIAGNOSTIC =====");
			console.log(`injected latency          : ${RESPONSE_LATENCY_MS}ms`);
			console.log(`prompt resolved at        : ${promptResolved.toFixed(1)}ms`);
			console.log(
				`session/update events      : ${updates.length} (of ${events.length} total)`,
			);
			console.log(`first update delivered at : ${firstUpdateT.toFixed(1)}ms`);
			console.log(`updates BEFORE resolution  : ${updatesBeforeResolve}`);
			console.log(`gap (resolve - firstUpdate): ${gap.toFixed(1)}ms`);
			console.log(`events at 2nd-LLM-req time : ${checkpointEvents}`);
			const verdict =
				gap > RESPONSE_LATENCY_MS * 0.5
					? "LIVE (FIXED): updates streamed during the mid-turn window"
					: "BATCHED (BUG): every update arrived only at resolution";
			console.log(`VERDICT: ${verdict}`);
			console.log("==================================================\n");
			/* eslint-enable no-console */

			// The contract: onSessionEvent is live. The tool_call update is
			// produced before the latency hold, so it must reach the subscriber
			// well before the prompt resolves.
			expect(
				updatesBeforeResolve,
				"BUG: no session/update arrived before resolution — events are batched until session/prompt resolves",
			).toBeGreaterThan(0);
			expect(
				gap,
				"BUG: first update arrived at ~the same time as resolution — events are batched, not streamed",
			).toBeGreaterThan(RESPONSE_LATENCY_MS * 0.5);
		} finally {
			if (sessionId) vm.closeSession(sessionId);
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 120_000);
});
