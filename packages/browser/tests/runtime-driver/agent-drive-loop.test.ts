import { describe, expect, it } from "vitest";
import {
	type AgentDriveDeps,
	AgentInteractionError,
	type AgentOutput,
	driveAgentInteraction,
} from "../../src/agent-drive-loop.js";

const text = (s: string) => new TextEncoder().encode(s);

/** A scripted deps: a queue of agent outputs, and deliver responses that go
 * pending until the final one returns the real result. */
function scriptedDeps(opts: {
	outputs: (AgentOutput | null)[];
	// For each delivered chunk, whether it is still pending; the last !pending one
	// carries the final result.
	deliver: {
		pending: boolean;
		response: string;
		timeoutMs?: number;
		timeoutPhase?: string;
	}[];
	clock?: number[];
}): {
	deps: AgentDriveDeps;
	delivered: Uint8Array[];
	stderr: Uint8Array[];
	deadlines: number[];
} {
	const outputs = [...opts.outputs];
	const deliver = [...opts.deliver];
	const delivered: Uint8Array[] = [];
	const stderr: Uint8Array[] = [];
	const deadlines: number[] = [];
	const clock = opts.clock ? [...opts.clock] : [];
	let now = 0;
	return {
		delivered,
		stderr,
		deadlines,
		deps: {
			pollAgentOutput: (_processId, deadlineMs) => {
				deadlines.push(deadlineMs);
				return outputs.shift() ?? null;
			},
			deliverAgentOutput: (_pid, chunk) => {
				delivered.push(chunk);
				const next = deliver.shift();
				if (!next) throw new Error("unexpected deliver");
				return next.pending
					? {
							pending: true,
							response: text(next.response),
							timeoutMs: next.timeoutMs ?? 30_000,
							timeoutPhase: next.timeoutPhase ?? "session/prompt",
						}
					: { pending: false, response: text(next.response) };
			},
			onAgentStderr: (_pid, chunk) => stderr.push(chunk),
			now: () => {
				const next = clock.shift();
				if (next !== undefined) now = next;
				return now;
			},
		},
	};
}

describe("driveAgentInteraction", () => {
	it("feeds stdout chunks until the handshake completes, returning the real result", () => {
		const { deps, delivered } = scriptedDeps({
			outputs: [
				{ kind: "stdout", payload: text("initialize-response") },
				{ kind: "stdout", payload: text("session-new-response") },
			],
			deliver: [
				{ pending: true, response: "acp-pending" },
				{ pending: false, response: "acp-session-created" },
			],
		});
		const result = driveAgentInteraction(
			deps,
			"proc-1",
			1_000_000,
			"session/prompt",
		);
		expect(new TextDecoder().decode(result)).toBe("acp-session-created");
		// Both stdout chunks were fed in order.
		expect(delivered.map((d) => new TextDecoder().decode(d))).toEqual([
			"initialize-response",
			"session-new-response",
		]);
	});

	it("skips stderr (it does not advance the handshake)", () => {
		const { deps, delivered, stderr } = scriptedDeps({
			outputs: [
				{ kind: "stderr", payload: text("warning: noise") },
				{ kind: "stdout", payload: text("the-response") },
			],
			deliver: [{ pending: false, response: "done" }],
		});
		const result = driveAgentInteraction(
			deps,
			"p",
			1_000_000,
			"session/prompt",
		);
		expect(new TextDecoder().decode(result)).toBe("done");
		expect(delivered).toHaveLength(1); // stderr was not delivered
		expect(stderr.map((chunk) => new TextDecoder().decode(chunk))).toEqual([
			"warning: noise",
		]);
	});

	it("throws if the agent exits before completing the interaction", () => {
		const { deps } = scriptedDeps({
			outputs: [{ kind: "exit", payload: new Uint8Array([0]) }],
			deliver: [],
		});
		expect(() =>
			driveAgentInteraction(deps, "p", 1_000_000, "session/prompt"),
		).toThrow(AgentInteractionError);
	});

	it("throws on timeout when no output arrives", () => {
		const { deps } = scriptedDeps({ outputs: [null], deliver: [] });
		expect(() =>
			driveAgentInteraction(deps, "p", 1_000_000, "session/prompt"),
		).toThrow(/no output/);
	});

	it("throws when the deadline has already passed", () => {
		const { deps } = scriptedDeps({
			outputs: [{ kind: "stdout", payload: text("x") }],
			deliver: [],
			clock: [2_000],
		});
		expect(() =>
			driveAgentInteraction(deps, "p", 1_000, "session/prompt"),
		).toThrow(/timed out/);
	});

	it("does not extend a deadline for repeated output in the same sidecar phase", () => {
		const { deps, deadlines } = scriptedDeps({
			outputs: [
				{ kind: "stdout", payload: text("noise-one") },
				{ kind: "stdout", payload: text("noise-two") },
			],
			deliver: [
				{
					pending: true,
					response: "still-pending",
					timeoutMs: 10_000,
					timeoutPhase: "session/prompt",
				},
			],
			clock: [0, 101],
		});
		expect(() =>
			driveAgentInteraction(deps, "p", 100, "session/prompt"),
		).toThrow(/timed out/);
		expect(deadlines).toEqual([100]);
	});

	it("starts a fresh deadline when the sidecar advances to a new phase", () => {
		const { deps, deadlines } = scriptedDeps({
			outputs: [
				{ kind: "stdout", payload: text("initialize-result") },
				{ kind: "stdout", payload: text("session-new-result") },
			],
			deliver: [
				{
					pending: true,
					response: "next-phase",
					timeoutMs: 1_000,
					timeoutPhase: "create.session_new",
				},
				{ pending: false, response: "done" },
			],
			clock: [0, 10, 20],
		});
		const result = driveAgentInteraction(deps, "p", 100, "create.initialize");
		expect(new TextDecoder().decode(result)).toBe("done");
		expect(deadlines).toEqual([100, 1_010]);
	});
});
