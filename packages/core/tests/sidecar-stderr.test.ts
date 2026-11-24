import { expect, test, vi } from "vitest";
import { AgentOs, type AgentOsSidecar } from "../src/index.js";

// The sidecar's stderr is forwarded to the host's stderr, so anything it prints
// lands in the user's terminal. This reaches into internals only to read the
// copy the client keeps of that stream.
function sidecarStderr(vm: AgentOs): string {
	return (
		vm as unknown as {
			_sidecarClient: {
				protocolClient: { sidecarProcess: { stderrText(): string } };
			};
		}
	)._sidecarClient.protocolClient.sidecarProcess.stderrText();
}

test("ordinary JavaScript and shell runs are quiet at warning log level", async () => {
	let sidecar: AgentOsSidecar | undefined;
	let vm: AgentOs | undefined;
	const errors: unknown[] = [];
	try {
		vi.stubEnv("AGENTOS_LOG_LEVEL", "warn");
		vi.stubEnv("AGENTOS_LOG_FILE", "");
		// A dedicated sidecar keeps output from other tests out of the captured stream.
		sidecar = await AgentOs.createSidecar();
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
		});
		// Several runs claim, refill, and evict warm V8 workers.
		for (let i = 0; i < 3; i++) {
			expect(await vm.javascript.evaluate(`${i} + 1`)).toMatchObject({
				outcome: "succeeded",
				value: i + 1,
			});
		}
		expect((await vm.exec("echo hi")).stdout).toBe("hi\n");

		expect(sidecarStderr(vm)).toBe("");
	} catch (error) {
		errors.push(error);
	} finally {
		try {
			await vm?.dispose();
		} catch (error) {
			errors.push(error);
		}
		try {
			await sidecar?.dispose();
		} catch (error) {
			errors.push(error);
		}
		try {
			vi.unstubAllEnvs();
		} catch (error) {
			errors.push(error);
		}
	}
	if (errors.length === 1) throw errors[0];
	if (errors.length > 1) {
		throw new AggregateError(errors, "warning-level sidecar control failed");
	}
}, 120_000);
