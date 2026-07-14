import { afterEach, describe, expect, it, vi } from "vitest";
import { type LimitWarning, AgentOs } from "../src/agent-os.js";

type LimitWarningAgent = AgentOs & {
	_limitWarningHandler?: (warning: LimitWarning) => void;
	_handleLimitWarning(detail: Record<string, string>): void;
};

function createAgent(
	handler: (warning: LimitWarning) => void,
): LimitWarningAgent {
	const agent = Object.create(AgentOs.prototype) as LimitWarningAgent;
	agent._limitWarningHandler = handler;
	return agent;
}

describe("AgentOs limit warning dispatch", () => {
	afterEach(() => vi.restoreAllMocks());

	it("forwards complete sidecar warning fields without client defaults", () => {
		const handler = vi.fn();
		const agent = createAgent(handler);

		agent._handleLimitWarning({
			limit: "vm_open_fds",
			category: "resource",
			observed: "82",
			capacity: "100",
			fillPercent: "82.5",
		});

		expect(handler).toHaveBeenCalledWith({
			limit: "vm_open_fds",
			category: "resource",
			observed: 82,
			capacity: 100,
			fillPercent: 82.5,
		});
	});

	it("reports malformed sidecar warnings instead of inventing zero values", () => {
		const handler = vi.fn();
		const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
		const agent = createAgent(handler);

		agent._handleLimitWarning({
			limit: "vm_open_fds",
			category: "resource",
			observed: "not-a-number",
			capacity: "100",
		});

		expect(handler).not.toHaveBeenCalled();
		expect(warn).toHaveBeenCalledWith(
			"invalid limit warning from sidecar",
			expect.any(Error),
		);
	});
});
