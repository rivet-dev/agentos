import { describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";

type FetchBackdoor = AgentOs & {
	_sidecarClient: {
		vmFetch(): Promise<string>;
	};
	_sidecarSession: object;
	_sidecarVm: object;
};

function agentWithFetchResponse(response: unknown): FetchBackdoor {
	const agent = Object.create(AgentOs.prototype) as FetchBackdoor;
	agent._sidecarClient = {
		async vmFetch() {
			return JSON.stringify(response);
		},
	};
	agent._sidecarSession = {};
	agent._sidecarVm = {};
	return agent;
}

describe("vm.fetch response validation", () => {
	test("rejects missing normalized fields instead of applying client defaults", async () => {
		const agent = agentWithFetchResponse({ status: 200 });

		await expect(
			agent.fetch(8080, new Request("http://vm.test/")),
		).rejects.toThrow("sidecar vm.fetch response is missing statusText");
	});

	test("builds the host response from complete sidecar data", async () => {
		const agent = agentWithFetchResponse({
			status: 201,
			statusText: "Created",
			headers: [["x-agentos", "ok"]],
			body: Buffer.from("hello").toString("base64"),
		});

		const response = await agent.fetch(
			8080,
			new Request("http://vm.test/resource"),
		);
		expect(response.status).toBe(201);
		expect(response.statusText).toBe("Created");
		expect(response.headers.get("x-agentos")).toBe("ok");
		expect(await response.text()).toBe("hello");
	});
});
