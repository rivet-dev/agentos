import { createHash } from "node:crypto";
import type { AgentOsActorConnection } from "@rivet-dev/agentos";
import type { VirtualStat } from "@rivet-dev/agentos-core";
import { beforeEach, describe, expect, it, vi } from "vitest";

const clientMocks = vi.hoisted(() => ({ createAgentOsClient: vi.fn() }));

vi.mock("@rivet-dev/agentos", async (importOriginal) => ({
	...(await importOriginal<typeof import("@rivet-dev/agentos")>()),
	createAgentOsClient: clientMocks.createAgentOsClient,
}));

import { AgentOSFlueConfigurationError, agentOSSandbox } from "../src/index.js";

function makeHarness() {
	const keyCalls: string[][] = [];
	const connection = {
		ready: Promise.resolve(),
		exec: vi.fn(async (_command: string, _options?: unknown) => ({
			exitCode: 0,
			stdout: "ok",
			stderr: "",
		})),
		readFile: vi.fn(async (_path: string) => new TextEncoder().encode("hello")),
		writeFile: vi.fn(
			async (_path: string, _content: string | Uint8Array) => {},
		),
		stat: vi.fn(
			async (_path: string): Promise<VirtualStat> => ({
				mode: 0o100644,
				size: 5,
				blocks: 1,
				dev: 1,
				rdev: 0,
				isDirectory: false,
				isSymbolicLink: false,
				atimeMs: 1234,
				mtimeMs: 1234,
				ctimeMs: 1234,
				birthtimeMs: 1234,
				ino: 7,
				nlink: 1,
				uid: 0,
				gid: 0,
			}),
		),
		readdir: vi.fn(async (_path: string) => ["a.txt"]),
		exists: vi.fn(async (_path: string) => true),
		mkdir: vi.fn(async (_path: string, _options?: unknown) => {}),
		remove: vi.fn(async (_path: string, _options?: unknown) => {}),
	};
	const actorConnection = {
		ready: Promise.resolve(),
		process: {
			run: vi.fn<AgentOsActorConnection["process"]["run"]>(
				async ({ command, args, options }) => {
					if (
						command !== "sh" ||
						args?.[0] !== "-c" ||
						typeof args[1] !== "string"
					) {
						throw new Error("expected shell command");
					}
					const result = await connection.exec(args[1], {
						cwd: options?.cwd,
						env: options?.env,
						timeout: options?.timeoutMs,
						captureStdio: options?.captureStdio,
					});
					return {
						status: { exitCode: result.exitCode },
						stdout: result.stdout,
						stderr: result.stderr,
					};
				},
			),
		},
		filesystem: {
			readFile: vi.fn(({ path }: any) => connection.readFile(path)),
			writeFile: vi.fn(({ path, content }: any) =>
				connection.writeFile(path, content),
			),
			stat: vi.fn<AgentOsActorConnection["filesystem"]["stat"]>(
				async ({ path }) => {
					const { size, ...stat } = await connection.stat(path);
					return { ...stat, sizeBytes: size };
				},
			),
			readdir: vi.fn(({ path }: any) => connection.readdir(path)),
			exists: vi.fn(({ path }: any) => connection.exists(path)),
			mkdir: vi.fn(({ path, recursive }: any) =>
				connection.mkdir(path, { recursive }),
			),
			remove: vi.fn(({ path, recursive }: any) =>
				connection.remove(path, { recursive }),
			),
		},
	};
	const connect = vi.fn(() => actorConnection);
	const getOrCreate = vi.fn(
		(key: string[], _options?: { params?: unknown }) => {
			keyCalls.push(key);
			return { connect };
		},
	);
	clientMocks.createAgentOsClient.mockReturnValue({ agentOS: { getOrCreate } });
	return { actorConnection, connect, connection, getOrCreate, keyCalls };
}

describe("agentOSSandbox", () => {
	beforeEach(() => clientMocks.createAgentOsClient.mockReset());

	it("reconnects to the same actor without retaining session environments", async () => {
		const harness = makeHarness();
		const sandbox = agentOSSandbox();
		const first = await sandbox.createSessionEnv({ id: "ticket-1" });
		const second = await sandbox.createSessionEnv({ id: "ticket-1" });

		expect(first).not.toBe(second);
		expect(harness.connect).toHaveBeenCalledTimes(2);
		expect(harness.keyCalls).toEqual([
			[
				"flue",
				"sandbox",
				createHash("sha256").update("ticket-1").digest("hex"),
			],
			[
				"flue",
				"sandbox",
				createHash("sha256").update("ticket-1").digest("hex"),
			],
		]);
		expect(harness.getOrCreate).toHaveBeenCalledTimes(2);
		expect(clientMocks.createAgentOsClient).toHaveBeenCalledOnce();
		expect(clientMocks.createAgentOsClient).toHaveBeenCalledWith(undefined);
	});

	it("forwards actor creation input", async () => {
		const harness = makeHarness();
		await agentOSSandbox({
			createInput: { config: { environment: { MODE: "test" } } },
		}).createSessionEnv({ id: "ticket-auth" });

		expect(harness.getOrCreate).toHaveBeenCalledWith(expect.any(Array), {
			createWithInput: { config: { environment: { MODE: "test" } } },
		});
	});

	it("maps Flue shell and filesystem operations to agentOS", async () => {
		const harness = makeHarness();
		const env = await agentOSSandbox().createSessionEnv({ id: "ticket-2" });

		await expect(env.readFile("note.txt")).resolves.toBe("hello");
		await expect(env.stat("note.txt")).resolves.toEqual({
			isFile: true,
			isDirectory: false,
			isSymbolicLink: false,
			size: 5,
			mtime: new Date(1234),
		});
		await expect(env.exec("printf ok", { timeoutMs: 500 })).resolves.toEqual({
			exitCode: 0,
			stdout: "ok",
			stderr: "",
		});
		expect(harness.connection.exec).toHaveBeenCalledWith("printf ok", {
			cwd: "/workspace",
			env: {},
			timeout: 500,
			captureStdio: true,
		});
	});

	it("maps bigint hosted metadata and nonzero structured exits", async () => {
		const harness = makeHarness();
		const env = await agentOSSandbox().createSessionEnv({
			id: "ticket-wire-types",
		});
		const { size, ...stat } = await harness.connection.stat("note.txt");
		harness.actorConnection.filesystem.stat.mockResolvedValueOnce({
			...stat,
			sizeBytes: BigInt(size),
			blocks: 1n,
			dev: 1n,
			rdev: 0n,
			ino: 7n,
			nlink: 1n,
		});
		await expect(env.stat("note.txt")).resolves.toEqual({
			isFile: true,
			isDirectory: false,
			isSymbolicLink: false,
			size: 5,
			mtime: new Date(1234),
		});
		harness.actorConnection.process.run.mockResolvedValueOnce({
			status: { exitCode: 23 },
			stdout: "partial output",
			stderr: "command failed",
		});
		await expect(env.exec("exit 23")).resolves.toEqual({
			exitCode: 23,
			stdout: "partial output",
			stderr: "command failed",
		});
	});

	it("implements force removal without hiding other failures", async () => {
		const harness = makeHarness();
		const env = await agentOSSandbox().createSessionEnv({ id: "ticket-3" });

		harness.connection.exists.mockResolvedValueOnce(false);
		await expect(env.rm("missing", { force: true })).resolves.toBeUndefined();
		expect(harness.connection.remove).not.toHaveBeenCalled();

		harness.connection.exists.mockResolvedValue(true);
		harness.connection.remove.mockRejectedValueOnce(
			new Error("permission denied"),
		);
		await expect(env.rm("protected", { force: true })).rejects.toThrow(
			"permission denied",
		);
	});

	it("reports a missing actor as a configuration error", async () => {
		clientMocks.createAgentOsClient.mockReturnValue({});
		const sandbox = agentOSSandbox();
		await expect(
			sandbox.createSessionEnv({ id: "ticket-4" }),
		).rejects.toBeInstanceOf(AgentOSFlueConfigurationError);
	});
});
