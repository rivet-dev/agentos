import { expect, it } from "vitest";
import { AgentOs } from "../src/agent-os.js";

it.each([
	"abandoned",
	"partial",
	"cancelled",
] as const)("VM disposal releases a %s HTTP response stream", async (mode) => {
	const vm = await AgentOs.create({
		permissions: { fs: "allow", childProcess: "allow", network: "deny" },
	});
	try {
		await vm.process.spawn("node", [
			"-e",
			"require('http').createServer((q,r)=>{r.writeHead(200);r.write('held-response');}).listen(3000,()=>require('fs').writeFileSync('/tmp/ready','1'));",
		]);
		const deadline = Date.now() + 5_000;
		while (!(await vm.filesystem.exists("/tmp/ready"))) {
			if (Date.now() > deadline)
				throw Error("guest server readiness exceeded 5s");
			await new Promise((resolve) => setTimeout(resolve, 20));
		}
		const head = await vm.fetchStreamStart(
			3000,
			new Request("http://guest.gameinc.io/"),
		);
		expect(head.status).toBe(200);
		if (mode === "partial") {
			const part = await vm.fetchStreamRead(head.streamId, 4);
			expect(Buffer.from(part.body).toString()).toBe("held");
			expect(part.done).toBe(false);
		}
		if (mode === "cancelled") await vm.fetchStreamCancel(head.streamId);
	} finally {
		// The two abandoned cases must succeed without an explicit stream cancel.
		await vm.dispose();
	}
}, 15_000);

it.each([
	"pending-read",
	"pending-read-cancel-dispose",
	"cancel-dispose",
] as const)("VM disposal settles an owned stream during %s", async (mode) => {
	const observe = <T>(promise: Promise<T>) =>
		promise.then(
			(value) => ({ ok: true as const, value }),
			(error: unknown) => ({ ok: false as const, error: String(error) }),
		);
	async function bounded<T>(promise: Promise<T>, label: string): Promise<T> {
		let timer: ReturnType<typeof setTimeout> | undefined;
		try {
			return await Promise.race([
				promise,
				new Promise<never>((_, reject) => {
					timer = setTimeout(
						() => reject(Error(`${label} exceeded 5s`)),
						5_000,
					);
				}),
			]);
		} finally {
			clearTimeout(timer);
		}
	}
	const vm = await AgentOs.create({
		permissions: { fs: "allow", childProcess: "allow", network: "deny" },
	});
	let read: ReturnType<typeof observe> | undefined;
	let cancel: ReturnType<typeof observe> | undefined;
	try {
		await vm.process.spawn("node", [
			"-e",
			"require('http').createServer((q,r)=>{r.writeHead(200);r.write('held-response');}).listen(3000,()=>require('fs').writeFileSync('/tmp/ready','1'));",
		]);
		const deadline = Date.now() + 5_000;
		while (!(await vm.filesystem.exists("/tmp/ready"))) {
			if (Date.now() > deadline)
				throw Error("guest server readiness exceeded 5s");
			await new Promise((resolve) => setTimeout(resolve, 20));
		}
		const head = await vm.fetchStreamStart(
			3000,
			new Request("http://guest.gameinc.io/"),
		);
		expect(head.status).toBe(200);
		if (mode !== "cancel-dispose") {
			const initial = await vm.fetchStreamRead(head.streamId, 64 * 1024);
			expect(Buffer.from(initial.body).toString()).toBe("held-response");
			expect(initial.done).toBe(false);
			read = observe(vm.fetchStreamRead(head.streamId, 4));
			let settled = false;
			void read.then(() => {
				settled = true;
			});
			// A competing NOT_FOUND proves the first operation owns the removed
			// lease. A delay alone would not establish that ownership.
			const competing = await bounded(
				observe(vm.fetchStreamRead(head.streamId, 4)),
				"competing read",
			);
			expect(competing.ok).toBe(false);
			if (!competing.ok)
				expect(competing.error).toContain("VM_FETCH_STREAM_NOT_FOUND");
			expect(settled).toBe(false);
		}
		if (mode !== "pending-read") {
			// Concurrent client submissions; the cancel/dispose backend overlap
			// is deliberately not claimed to be deterministic.
			cancel = observe(vm.fetchStreamCancel(head.streamId));
		}
	} finally {
		const disposal = await observe(vm.dispose());
		const readResult =
			read !== undefined
				? await bounded(read, "in-flight read settlement")
				: undefined;
		if (cancel !== undefined) await bounded(cancel, "cancel settlement");
		expect(disposal.ok, JSON.stringify(disposal)).toBe(true);
		if (readResult) expect(readResult.ok).toBe(false);
	}
}, 15_000);
