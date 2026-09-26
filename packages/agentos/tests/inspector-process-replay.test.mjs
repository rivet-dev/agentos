import assert from "node:assert/strict";
import { test } from "node:test";
import { drainProcessReplay, ProcessOutputDecoder } from "../src/inspector-tabs/lib/process-output.ts";
import { setRivetClient } from "../src/inspector-tabs/lib/actor-client.ts";
import { agentOsSource } from "../src/inspector-tabs/lib/source.ts";

function page(events, hasMore = false, generation = 1) {
	return {
		generation,
		events: events.map((sequence) => ({
			sequence,
			stream: "stdout",
			data: new Uint8Array([65]),
			timestampMs: 0,
		})),
		nextCursor: events.at(-1) ?? null,
		hasMore,
		truncated: false,
		end: { exitCode: 0 },
	};
}

test("process replay drains available pages in one bounded poll", async () => {
	const requests = [];
	const replay = await drainProcessReplay(async (after) => {
		requests.push(after);
		return after === undefined
			? page(Array.from({ length: 256 }, (_, index) => index), true)
			: page(Array.from({ length: 44 }, (_, index) => index + 256));
	}, 1);
	assert.deepEqual(requests, [undefined, 255]);
	assert.equal(replay.events.length, 300);
	assert.equal(replay.nextCursor, 299);
	assert.equal(replay.hasMore, false);
	assert.equal(replay.end.exitCode, 0);
});

test("process replay preserves bigint cursors and aggregate truncation", async () => {
	const first = 9007199254740992n;
	let reads = 0;
	const replay = await drainProcessReplay(async (after) => {
		assert.equal(after, reads === 0 ? undefined : first);
		const result = page([first + BigInt(reads)], reads === 0);
		result.truncated = reads++ === 0;
		return result;
	}, 1);
	assert.equal(replay.nextCursor, first + 1n);
	assert.equal(replay.truncated, true);
});

test("process replay caps continuous output at eight pages per poll", async () => {
	let reads = 0;
	const replay = await drainProcessReplay(async () => page([reads++], true), 1);
	assert.equal(reads, 8);
	assert.equal(replay.events.length, 8);
	assert.equal(replay.hasMore, true);
	assert.equal(replay.nextCursor, 7);
});

test("process replay rejects a stale generation and non-progressing cursor", async () => {
	await assert.rejects(
		drainProcessReplay(async () => page([0], false, 2), 1),
		/different VM generation/,
	);
	await assert.rejects(
		drainProcessReplay(async () => page([], true), 1),
		/did not advance its cursor/,
	);
	await assert.rejects(
		drainProcessReplay(async () => page([0]), 1, 0),
		/out-of-order sequences/,
	);
});

test("closing process details stops after the current page instead of draining the backlog", async () => {
	const controller = new AbortController();
	let reads = 0;
	await assert.rejects(
		drainProcessReplay(async () => {
			reads++;
			controller.abort();
			return page([0], true);
		}, 1, undefined, controller.signal),
		{ name: "AbortError" },
	);
	assert.equal(reads, 1);
});

test("process replay applies byte bounds to decoded data and rejects oversized event pages", async () => {
	const response = page([0, 1]);
	response.events[0].data = ["$Uint8Array", Buffer.alloc(32 * 1024).toString("base64")];
	response.events[1].data = new Uint8Array(32 * 1024);
	const replay = await drainProcessReplay(async () => response, 1);
	assert.equal(replay.events[0].data.byteLength, 32 * 1024);
	response.events[1].data = new Uint8Array(32 * 1024 + 1);
	await assert.rejects(drainProcessReplay(async () => response, 1), /byte page limit/);
	await assert.rejects(
		drainProcessReplay(async () => page(Array.from({ length: 257 }, (_, index) => index)), 1),
		/event page limit/,
	);
});

test("process replay remains pinned to one actor across a multi-page drain", async () => {
	const requests = [];
	setRivetClient({ getForId: () => ({ process: { output: { read: async (request) => {
		requests.push(request);
		setRivetClient({ getForId: () => { throw new Error("wrong actor"); } }, "other");
		return page([requests.length - 1], requests.length === 1);
	} } } }) }, "original");
	const replay = await drainProcessReplay(agentOsSource.processOutputReader(1, 7), 1);
	assert.equal(replay.nextCursor, 1);
	assert.deepEqual(requests.map((request) => request.after), [undefined, 0]);
	for (const request of requests) {
		assert.deepEqual(request.process, { generation: 1, pid: 7 });
		assert.equal(request.maxBytes, 64 * 1024);
		assert.equal(request.maxEvents, 256);
	}
});

function outputEvent(sequence, data, stream = "stdout") {
	return { sequence, data: Uint8Array.from(data), stream, timestampMs: 0 };
}

test("process text decoding spans pages and polls without mixing stdout and stderr", () => {
	const decoder = new ProcessOutputDecoder();
	assert.equal(decoder.decode(outputEvent(0, [0xe2, 0x82])), "");
	assert.equal(decoder.decode(outputEvent(1, [0xc2], "stderr")), "");
	assert.equal(decoder.decode(outputEvent(2, [0xac])), "€");
	assert.equal(decoder.decode(outputEvent(3, [0xa3], "stderr")), "£");
	assert.equal(decoder.finish(), "");
});

test("process text decoding does not complete a character across a lost event", () => {
	const decoder = new ProcessOutputDecoder();
	assert.equal(decoder.decode(outputEvent(0, [0xe2, 0x82])), "");
	assert.equal(decoder.decode(outputEvent(2, [0xac])), "�");
	assert.equal(decoder.decode(outputEvent(3, [0xc2])), "");
	assert.equal(decoder.finish(), "�");
	assert.equal(decoder.finish(), "");
});

test("process replay reports sequence gaps even if the server omits its truncation flag", async () => {
	const replay = await drainProcessReplay(async () => page([2, 3]), 1, 0);
	assert.equal(replay.truncated, true);
});
