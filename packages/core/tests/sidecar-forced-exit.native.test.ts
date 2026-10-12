import { isAbsolute } from "node:path";
import { expect, test, vi } from "vitest";
import type { StdioSidecarProcess } from "../src/process.js";
import { SidecarProcess } from "../src/sidecar/process-client.js";
import {
	DEFAULT_SIDECAR_FORCE_EXIT_MS,
	DEFAULT_SIDECAR_GRACEFUL_EXIT_MS,
	StdioSidecarProtocolClient,
} from "../src/stdio-client.js";
import {
	bodyWithCleanup,
	bounded,
	observeOwnedClose,
	ownedProcessState,
	waitUntilStopped,
} from "./helpers/native-owner-fixture.js";

// Intentional default graceful deadline expiry; explicit expensive phase only.
// Reuse the existing lifecycle-test opt-in; this is not a runtime option.
const runNative =
	process.platform === "linux" &&
	process.env.AGENTOS_STAB_HOSTED_LIFECYCLE_NATIVE === "1";

test.skipIf(!runNative)(
	"authenticated stopped native sidecar is killed, final-waited and reaped",
	async () => {
		const command = process.env.AGENTOS_SIDECAR_BIN;
		expect(
			command && isAbsolute(command),
			"pin an absolute AGENTOS_SIDECAR_BIN",
		).toBe(true);
		const transport = StdioSidecarProtocolClient.spawn({ command });
		const child = transport.child;
		const closed = observeOwnedClose(child);
		const native = (
			transport as unknown as { sidecarProcess: StdioSidecarProcess }
		).sidecarProcess;
		const waitForExit = native.waitForExit.bind(native);
		const observations: Array<{
			timeoutMs: number;
			code: number | null;
			signal: NodeJS.Signals | null;
		}> = [];
		const waitSpy = vi
			.spyOn(native, "waitForExit")
			.mockImplementation(async (timeoutMs) => {
				const code = await waitForExit(timeoutMs);
				observations.push({ timeoutMs, code, signal: child.signalCode });
				return code;
			});
		const killSpy = vi.spyOn(child, "kill"); // Call-through, no fabricated signals/status.
		await bodyWithCleanup(
			async () => {
				const client = SidecarProcess.fromClient(transport);
				const session = await bounded(
					client.authenticateAndOpenSession(),
					10_000,
					"real native handshake",
				);
				expect(session.connectionId).not.toBe("");
				expect(child.kill("SIGSTOP")).toBe(true);
				await bounded(
					waitUntilStopped(child),
					1_500,
					"owned stopped-state observation",
				);
				await bounded(
					transport.dispose(),
					DEFAULT_SIDECAR_GRACEFUL_EXIT_MS +
						DEFAULT_SIDECAR_FORCE_EXIT_MS +
						1_000,
					"forced native disposal",
				);
				expect(killSpy.mock.calls.map(([signal]) => signal)).toEqual([
					"SIGSTOP",
					"SIGKILL",
				]);
				expect(observations).toEqual([
					{
						timeoutMs: DEFAULT_SIDECAR_GRACEFUL_EXIT_MS,
						code: null,
						signal: null,
					},
					{
						timeoutMs: DEFAULT_SIDECAR_FORCE_EXIT_MS,
						code: null,
						signal: "SIGKILL",
					},
				]);
				expect(await bounded(closed, 2_000, "native stdio close")).toEqual({
					code: null,
					signal: "SIGKILL",
				});
				expect(child.signalCode).toBe("SIGKILL");
				expect(
					await bounded(
						ownedProcessState(child),
						500,
						"owned child reap query",
					),
					"Node must reap its actual child",
				).toBeNull();
			},
			async () => {
				waitSpy.mockRestore();
				killSpy.mockRestore();
				const errors: unknown[] = [];
				if (child.exitCode === null && child.signalCode === null) {
					try {
						if (!child.kill("SIGKILL"))
							errors.push(
								new Error("owned native sidecar cleanup SIGKILL was not sent"),
							);
					} catch (error) {
						errors.push(error);
					}
				}
				// A false/throwing signal attempt must never skip real close/reap.
				try {
					await bounded(closed, 2_000, "owned native cleanup close");
				} catch (error) {
					errors.push(error);
				}
				try {
					expect(
						await bounded(
							ownedProcessState(child),
							500,
							"owned native cleanup reap query",
						),
					).toBeNull();
				} catch (error) {
					errors.push(error);
				}
				if (errors.length)
					throw new AggregateError(
						errors,
						"owned native sidecar cleanup failed",
					);
			},
		);
	},
	30_000,
);
