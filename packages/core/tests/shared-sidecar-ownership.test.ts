import common from "@agentos-software/common";
import { afterEach, describe, expect, it, vi } from "vitest";
import { z } from "zod";
import { AgentOs, hostTool, toolKit } from "../src/index.js";
import { NativeSidecarProcessClient } from "../src/sidecar/rpc-client.js";

function ownerToolKit(owner: string) {
	return toolKit({
		name: "owner",
		description: "Reports the VM-specific host owner",
		tools: {
			who: hostTool({
				description: "Return the owner",
				inputSchema: z.object({}),
				execute: () => ({ owner }),
			}),
		},
	});
}

describe("shared sidecar VM ownership", () => {
	let vmA: AgentOs | null = null;
	let vmB: AgentOs | null = null;
	let sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>> | null = null;

	afterEach(async () => {
		await vmB?.dispose();
		await vmA?.dispose();
		await sidecar?.dispose();
		vmA = null;
		vmB = null;
		sidecar = null;
		vi.restoreAllMocks();
	});

	it("keeps host tools and cron callbacks scoped to their VM", async () => {
		const registrations: Array<{
			ownership: any;
			handler: (request: any) => Promise<any> | any;
		}> = [];
		const original =
			NativeSidecarProcessClient.prototype.registerSidecarRequestHandler;
		vi.spyOn(
			NativeSidecarProcessClient.prototype,
			"registerSidecarRequestHandler",
		).mockImplementation(function (
			this: NativeSidecarProcessClient,
			ownership: any,
			handler: any,
		) {
			registrations.push({ ownership, handler });
			return original.call(this, ownership, handler);
		});

		sidecar = await AgentOs.createSidecar();
		vmA = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			software: [common],
			toolKits: [ownerToolKit("a")],
		});
		vmB = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			software: [common],
			toolKits: [ownerToolKit("b")],
		});

		expect(registrations).toHaveLength(2);
		const invoke = async (registration: (typeof registrations)[number]) =>
			registration.handler({
				frame_type: "sidecar_request",
				schema: { name: "agentos-sidecar", version: 1 },
				request_id: 1,
				ownership: registration.ownership,
				payload: {
					type: "host_callback",
					invocation_id: "same-invocation-id",
					callback_key: "owner:who",
					input: {},
					timeout_ms: 1_000,
				},
			});
		await expect(invoke(registrations[0])).resolves.toMatchObject({
			result: { owner: "a" },
		});
		await expect(invoke(registrations[1])).resolves.toMatchObject({
			result: { owner: "b" },
		});

		const callbackA = vi.fn();
		const callbackB = vi.fn();
		const fireAt = new Date(Date.now() + 500).toISOString();
		await Promise.all([
			vmA.scheduleCron({
				id: "owner-a",
				schedule: fireAt,
				action: { type: "callback", fn: callbackA },
			}),
			vmB.scheduleCron({
				id: "owner-b",
				schedule: fireAt,
				action: { type: "callback", fn: callbackB },
			}),
		]);
		await vi.waitFor(
			() => {
				expect(callbackA).toHaveBeenCalledOnce();
				expect(callbackB).toHaveBeenCalledOnce();
			},
			{ timeout: 5_000 },
		);

		await vmB.dispose();
		vmB = null;
		const callbackAfterDispose = vi.fn();
		await vmA.scheduleCron({
			id: "owner-a-after-b-dispose",
			schedule: new Date(Date.now() + 250).toISOString(),
			action: { type: "callback", fn: callbackAfterDispose },
		});
		await vi.waitFor(
			() => expect(callbackAfterDispose).toHaveBeenCalledOnce(),
			{ timeout: 5_000 },
		);
	});
});
