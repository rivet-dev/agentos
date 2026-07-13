import { describe, expect, test, vi } from "vitest";
import { AgentOs } from "../src/index.js";

describe("AgentOs facade forwarding", () => {
	test("rawSessionSend forwards method and params to the session request path", async () => {
		const sendSessionRequest = vi.fn(async () => ({
			jsonrpc: "2.0" as const,
			id: 1,
			result: { ok: true },
		}));
		const vm = Object.create(AgentOs.prototype) as AgentOs & {
			_sendSessionRequest: typeof sendSessionRequest;
		};
		vm._sendSessionRequest = sendSessionRequest;

		await expect(
			vm.rawSessionSend("session-1", "custom/method", { value: 42 }),
		).resolves.toMatchObject({
			result: { ok: true },
		});

		expect(sendSessionRequest).toHaveBeenCalledWith(
			"session-1",
			"custom/method",
			{
				value: 42,
			},
		);
	});

	test("resizeShell forwards dimensions to the tracked shell handle", async () => {
		const resize = vi.fn();
		const vm = Object.create(AgentOs.prototype) as AgentOs & {
			_shells: Map<
				string,
				{ handle: { resize(cols: number, rows: number): void } }
			>;
		};
		vm._shells = new Map([
			[
				"shell-1",
				{
					handle: {
						resize,
					},
				},
			],
		]);

		await vm.resizeShell("shell-1", 120, 40);

		expect(resize).toHaveBeenCalledWith(120, 40);
	});

	test("resizeShell rejects unknown shell ids", async () => {
		const vm = Object.create(AgentOs.prototype) as AgentOs & {
			_shells: Map<string, unknown>;
		};
		vm._shells = new Map();

		await expect(vm.resizeShell("missing", 80, 24)).rejects.toThrow(
			"Shell not found: missing",
		);
	});
});
