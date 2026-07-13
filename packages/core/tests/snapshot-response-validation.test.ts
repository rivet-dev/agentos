import { describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";

type SnapshotBackdoor = AgentOs & {
	_sidecarClient: {
		snapshotRootFilesystem(): Promise<unknown[]>;
	};
	_sidecarSession: object;
	_sidecarVm: object;
};

function agentWithSnapshot(entries: unknown[]): SnapshotBackdoor {
	const agent = Object.create(AgentOs.prototype) as SnapshotBackdoor;
	agent._sidecarClient = {
		async snapshotRootFilesystem() {
			return entries;
		},
	};
	agent._sidecarSession = {};
	agent._sidecarVm = {};
	return agent;
}

describe("sidecar root snapshot response validation", () => {
	test("rejects missing Linux metadata instead of inventing client defaults", async () => {
		const agent = agentWithSnapshot([
			{
				path: "/workspace/file.txt",
				kind: "file",
				content: "hello",
				encoding: "utf8",
				executable: false,
			},
		]);

		await expect(agent.snapshotRootFilesystem()).rejects.toThrow(
			"sidecar root snapshot for /workspace/file.txt is missing mode",
		);
	});

	test("preserves complete sidecar metadata verbatim", async () => {
		const agent = agentWithSnapshot([
			{
				path: "/workspace/file.txt",
				kind: "file",
				mode: 0o640,
				uid: 501,
				gid: 20,
				content: "hello",
				encoding: "utf8",
				executable: false,
			},
		]);

		await expect(agent.snapshotRootFilesystem()).resolves.toMatchObject({
			source: {
				filesystem: {
					entries: [
						{
							path: "/workspace/file.txt",
							mode: "0640",
							uid: 501,
							gid: 20,
							content: "hello",
							encoding: "utf8",
						},
					],
				},
			},
		});
	});
});
