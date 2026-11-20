import { expect, test, vi } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import type { SidecarProcess } from "../src/sidecar/process-client.js";

test("every byte written before exit reaches onStdout when the host learns the exit early", async () => {
	const vm = await AgentOs.create();
	const { _sidecarClient: client, _sidecarVm } = vm as unknown as {
		_sidecarClient: SidecarProcess;
		_sidecarVm: { vmId: string };
	};
	const ready = Promise.withResolvers<void>();
	const releaseOutput = Promise.withResolvers<void>();
	const expected = Buffer.alloc(32 * 1024, 120);
	const chunks: Uint8Array[] = [];
	let holdOutput = false;
	let readyOutput = "";
	const nextEvent = client.waitForEvent.bind(client);
	const waitForEvent = vi
		.spyOn(client, "waitForEvent")
		.mockImplementation(async (...args) => {
			const event = await nextEvent(...args);
			if (
				holdOutput &&
				event.ownership.scope === "vm" &&
				event.ownership.vm_id === _sidecarVm.vmId &&
				event.payload.type === "process_output"
			) {
				await releaseOutput.promise;
			}
			return event;
		});
	try {
		const { pid } = await vm.process.spawn(
			"node",
			[
				"-e",
				`process.stdin.once('end', () => process.stdout.write(Buffer.alloc(${expected.length}, 120)));
process.stdin.resume();
process.stdout.write('ready');`,
			],
			{
				streamStdin: true,
				output: { retainEvents: true },
				onStdout: (bytes) => {
					if (holdOutput) {
						chunks.push(bytes);
					} else {
						readyOutput += Buffer.from(bytes).toString();
						if (readyOutput === "ready") ready.resolve();
					}
				},
			},
		);
		await ready.promise;
		// Pause only the application event pump. The response lane must still
		// report the exit even though the output callback has not run.
		holdOutput = true;
		await vm.process.closeStdin(pid);
		await vi.waitFor(
			async () => {
				const page = await vm.process.readOutput(pid, { maxEvents: 1 });
				expect(page.exitCode).toBe(0);
			},
			{ timeout: 5_000 },
		);
		expect(chunks).toHaveLength(0);
		expect(await vm.process.wait(pid)).toMatchObject({
			outcome: "exited",
			exitCode: 0,
		});

		releaseOutput.resolve();
		await vi.waitFor(() => expect(Buffer.concat(chunks)).toEqual(expected));
	} finally {
		releaseOutput.resolve();
		waitForEvent.mockRestore();
		await vm.dispose();
	}
}, 20_000);
