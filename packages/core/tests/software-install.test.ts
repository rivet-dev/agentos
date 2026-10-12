import { agentOsOptionFieldSchemas } from "../src/options-schema.js";
import { describe, expect, it, vi } from "vitest";
import { AgentOs } from "../src/agent-os.js";

function createAgent() {
	const metadata = {
		package_id: "pkg-sha256:demo",
		digest: "sha256:demo",
		size: 123n,
		package_name: "demo",
		version: "1.0.0",
		commands: ["demo"],
	};
	const client = {
		onEvent: () => () => {},
		onClose: () => () => {},
		installPackage: vi.fn(async () => ({
			package: metadata,
			projected_commands: [],
		})),
		unlinkPackage: vi.fn(async () => ["demo"]),
	};
	// Exercise the real namespace/methods without booting a native VM. The
	// constructor only wires trusted host handles; these tests need no kernel IO.
	const agent = Reflect.construct(AgentOs, [
		{},
		{},
		[],
		[],
		{},
		{},
		client,
		{ connectionId: "conn", sessionId: "session" },
		{ vmId: "vm" },
	]) as AgentOs;
	return { agent, client, metadata };
}

describe("sidecar-owned software install", () => {
	it("forwards a closed source and returns an isolated installed snapshot", async () => {
		const { agent, client, metadata } = createAgent();
		const installed = await agent.software.install({
			type: "path",
			path: "/tmp/demo.aospkg",
		});
		expect(client.installPackage).toHaveBeenCalledWith(
			{ connectionId: "conn", sessionId: "session" },
			{ vmId: "vm" },
			{
				source: {
					type: "path",
					path: "/tmp/demo.aospkg",
					expected_digest: undefined,
				},
			},
		);
		installed.packageName = "mutated";
		installed.commands.push("injected");
		metadata.commands.push("transport-mutated");
		const snapshot = agent.software.installed();
		expect(snapshot).toEqual([
			expect.objectContaining({
				packageName: "demo",
				commands: ["demo"],
				sizeBytes: 123n,
			}),
		]);
		snapshot[0].commands.length = 0;
		expect(agent.software.installed()[0].commands).toEqual(["demo"]);
	});

	it("retains installed identity when sidecar unlink fails", async () => {
		const { agent, client } = createAgent();
		const installed = await agent.software.install({
			type: "url",
			url: "https://packages.gameinc.io/demo.aospkg",
		});
		client.unlinkPackage.mockRejectedValueOnce(
			new Error("mount removal failed"),
		);
		await expect(agent.software.uninstall(installed.packageId)).rejects.toThrow(
			"mount removal failed",
		);
		expect(agent.software.installed()).toHaveLength(1);
		await agent.software.uninstall(installed.packageId);
		expect(agent.software.installed()).toEqual([]);
	});

	it("rejects overlapping VM mutations instead of queueing them", async () => {
		const { agent, client, metadata } = createAgent();
		let release!: () => void;
		client.installPackage.mockImplementationOnce(
			() =>
				new Promise((resolve) => {
					release = () =>
						resolve({ package: metadata, projected_commands: [] });
				}),
		);

		const first = agent.software.install({
			type: "path",
			path: "/tmp/first.aospkg",
		});
		await expect(
			agent.software.install({
				type: "path",
				path: "/tmp/second.aospkg",
			}),
		).rejects.toThrow(
			"another VM mount or software mutation is already in progress",
		);
		release();
		await expect(first).resolves.toEqual(
			expect.objectContaining({ packageId: metadata.package_id }),
		);
	});
});

describe("create-time software entries", () => {
	it("rejects an entry that is not a package ref", async () => {
		await expect(
			AgentOs.create({
				software: [{ name: "tool" } as unknown as { packagePath: string }],
			}),
		).rejects.toThrow("not a package ref");
	});
});

it("projects the trusted source URL option into the actual install request and preserves omission", async () => {
    const client = {
        onEvent: () => () => {}, onClose: () => () => {},
        installPackage: vi.fn(async () => ({ package: {
            package_id: "fixture", digest: "sha256:fixture", size: 1n,
            package_name: "fixture", version: "1.0.0", commands: [],
        }, projected_commands: [] })),
    };
    const agent = Reflect.construct(AgentOs,[{}, {}, [], [], {}, {}, client,
        {connectionId:"conn",sessionId:"session"},{vmId:"vm"}]) as AgentOs;
    const source = {type:"url" as const,url:"https://packages.gameinc.io/fixture.aospkg"};
    await agent.software.install(source);
    expect(client.installPackage).toHaveBeenNthCalledWith(1,
        {connectionId:"conn",sessionId:"session"},{vmId:"vm"},
        expect.not.objectContaining({max_source_url_bytes:expect.anything()}));
    (agent as unknown as {_packageResolver: {maxSourceUrlBytes:number}})._packageResolver = {maxSourceUrlBytes:8192};
    await agent.software.install(source);
    expect(client.installPackage).toHaveBeenNthCalledWith(2,
        {connectionId:"conn",sessionId:"session"},{vmId:"vm"},
        expect.objectContaining({max_source_url_bytes:8192n}));
});

it("accepts only finite positive safe source URL capacities", () => {
    const schema = agentOsOptionFieldSchemas.packageResolver;
    expect(schema.parse({maxSourceUrlBytes:8192})).toEqual({maxSourceUrlBytes:8192});
    expect(schema.parse({})).toEqual({});
    for (const value of [0,-1,1.5,Infinity,Number.MAX_SAFE_INTEGER+1]) {
        expect(schema.safeParse({maxSourceUrlBytes:value}).success).toBe(false);
    }
});
