import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import { toSidecarSignalName } from "../src/sidecar/rpc-client.js";

const names = JSON.parse(
	readFileSync(
		new URL(
			"../../../crates/actor/tests/fixtures/observed-signals.json",
			import.meta.url,
		),
		"utf8",
	),
) as Array<[number, string]>;

it("formats every observed signal independently of the request union", () => {
	for (const [number, name] of names)
		expect(toSidecarSignalName(number)).toBe(name);
});

async function deadline<T>(
	pending: Promise<T>,
	milliseconds: number,
): Promise<T> {
	let timer: ReturnType<typeof setTimeout> | undefined;
	try {
		return await Promise.race([
			pending,
			new Promise<never>((_, reject) => {
				timer = setTimeout(
					() => reject(new Error("process readiness exceeded5s")),
					milliseconds,
				);
			}),
		]);
	} finally {
		clearTimeout(timer);
	}
}

describe("authoritative process exit reason", () => {
	it.each([
		{ kind: "signal", code: 143, signal: 15 },
		{ kind: "normal143", code: 143, signal: undefined },
		{ kind: "caught", code: 0, signal: undefined },
	] as const)("keeps $kind identical across live wait, get, list, replay and tree", async ({
		kind,
		code,
		signal,
	}) => {
		const vm = await AgentOs.create({
			permissions: { fs: "allow", childProcess: "allow" },
		});
		try {
			const script =
				kind === "normal143"
					? "process.exit(143)"
					: `${kind === "caught" ? "process.on('SIGTERM', () => process.exit(0));" : ""}console.log('ready'); setInterval(() => {}, 1000);`;
			let ready: () => void = () => {};
			const readiness = new Promise<void>((resolve) => {
				ready = resolve;
			});
			const process = await vm.process.spawn("node", ["-e", script], {
				output: { retainEvents: true },
				onStdout: (chunk) => {
					if (new TextDecoder().decode(chunk).includes("ready")) ready();
				},
			});
			const events: unknown[] = [];
			const unsubscribe = vm.onProcessExit(process.pid, (exit) =>
				events.push(exit),
			);
			try {
				if (kind !== "normal143") {
					await deadline(readiness, 5_000);
					await vm.process.signal(process.pid, "SIGTERM");
				}
				const exit = await vm.process.wait(process.pid);
				expect(exit).toMatchObject({
					pid: process.pid,
					exitCode: code,
					outcome: signal ? "signalled" : "exited",
				});
				expect(exit.exitSignal).toBe(signal);
				expect(exit.signal).toBe(signal ? "SIGTERM" : undefined);
				expect(await vm.process.wait(process.pid)).toEqual(exit);
				expect(events).toEqual([exit]);
				const info = await vm.process.get(process.pid);
				expect(info.exitCode).toBe(code);
				expect(info.exitSignal).toBe(signal);
				expect(
					(await vm.process.list()).find((info) => info.pid === process.pid),
				).toEqual(info);
				const replay = await vm.process.readOutput(process.pid);
				expect(replay.exitCode).toBe(code);
				expect(replay.exitSignal ?? undefined).toBe(signal);
				const flatten = (
					nodes: Awaited<ReturnType<typeof vm.process.tree>>,
				): typeof nodes =>
					nodes.flatMap((node) => [node, ...flatten(node.children)]);
				const tree = flatten(await vm.process.tree()).find(
					(node) => node.pid === process.pid,
				);
				expect(
					tree,
					"tracked exited process remains queryable in topology",
				).toBeDefined();
				expect(tree?.exitCode).toBe(code);
				expect(tree?.exitSignal).toBe(signal);
			} finally {
				unsubscribe();
			}
		} finally {
			await vm.dispose();
		}
	}, 20_000);
});
