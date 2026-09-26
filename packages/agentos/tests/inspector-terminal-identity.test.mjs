import assert from "node:assert/strict";
import { test } from "node:test";
import { setRivetClient } from "../src/inspector-tabs/lib/actor-client.ts";
import { agentOsSource, shellsQueryOptions } from "../src/inspector-tabs/lib/source.ts";

function deferred() {
	let resolve;
	const promise = new Promise((done) => { resolve = done; });
	return { promise, resolve };
}

function actor(id, generation) {
	const terminal = { generation, shellId: "shared-shell" };
	const calls = [];
	const entries = [{ terminal, pid: 7, running: true }];
	const record = async (method, request) => {
		calls.push([method, request]);
		assert.deepEqual(request.terminal, terminal);
	};
	return {
		id, calls, entries,
		terminal: {
			list: async () => { calls.push(["list"]); return entries; },
			open: async () => { calls.push(["open"]); return terminal; },
			stdin: { write: (request) => record("write", request) },
			pty: { resize: (request) => record("resize", request) },
			close: (request) => record("close", request),
			output: { read: async (request) => {
				await record("read", request);
				return { generation, events: [], nextCursor: null, hasMore: false, truncated: false };
			} },
		},
	};
}

function select(target) {
	setRivetClient({ getForId: () => target }, target.id);
}

test("identical shell IDs on different actors never share cached handles", async () => {
	const first = actor("first", 7);
	const second = actor("second", 9);
	select(first);
	await agentOsSource.openShell(80, 24);
	select(second);
	await agentOsSource.writeShell("shared-shell", "second");
	select(first);
	await agentOsSource.resizeShell("shared-shell", 100, 30);
	assert.deepEqual(first.calls.map(([method]) => method), ["open", "resize"]);
	assert.deepEqual(second.calls.map(([method]) => method), ["list", "write"]);
});

for (const [name, invoke] of [
	["write", () => agentOsSource.writeShell("shared-shell", "first")],
	["resize", () => agentOsSource.resizeShell("shared-shell", 100, 30)],
	["close", () => agentOsSource.closeShell("shared-shell")],
	["read", () => agentOsSource.shellSnapshot("shared-shell")],
]) {
	test(`terminal ${name} remains on its actor while lookup is in flight`, async () => {
		const first = actor(`first-${name}`, 7);
		const second = actor(`second-${name}`, 9);
		const lookup = deferred();
		first.terminal.list = () => lookup.promise;
		select(first);
		const operation = invoke();
		select(second);
		await agentOsSource.openShell(80, 24);
		lookup.resolve(first.entries);
		await operation;
		await agentOsSource.writeShell("shared-shell", "second");
		assert.deepEqual(first.calls.map(([method]) => method), [name]);
		assert.deepEqual(second.calls.map(([method]) => method), ["open", "write"]);
	});
}

for (const kind of ["open", "list"]) {
	test(`a late terminal ${kind} response only populates its captured actor cache`, async () => {
		const first = actor(`late-first-${kind}`, 7);
		const second = actor(`late-second-${kind}`, 9);
		const response = deferred();
		first.terminal[kind] = () => response.promise;
		select(first);
		const operation = kind === "open"
			? agentOsSource.openShell(80, 24)
			: shellsQueryOptions(first.id).queryFn();
		select(second);
		await agentOsSource.openShell(80, 24);
		response.resolve(kind === "open" ? first.entries[0].terminal : first.entries);
		await operation;
		await agentOsSource.writeShell("shared-shell", "second");
		select(first);
		await agentOsSource.writeShell("shared-shell", "first");
		assert.deepEqual(first.calls.map(([method]) => method), ["write"]);
		assert.deepEqual(second.calls.map(([method]) => method), ["open", "write"]);
	});
}

test("terminal list refresh removes handles closed by another client", async () => {
	const target = actor("pruned", 7);
	select(target);
	await agentOsSource.openShell(80, 24);
	target.terminal.list = async () => [];
	await shellsQueryOptions(target.id).queryFn();
	await assert.rejects(agentOsSource.writeShell("shared-shell", "stale"), /no longer available/);
	assert.deepEqual(target.calls.map(([method]) => method), ["open"]);
});
