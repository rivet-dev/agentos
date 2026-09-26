import assert from "node:assert/strict";
import { test } from "node:test";
import { setRivetClient } from "../src/inspector-tabs/lib/actor-client.ts";
import { agentOsSource } from "../src/inspector-tabs/lib/source.ts";
import { OutputRouter } from "../src/inspector-tabs/lib/terminal-output.ts";

function useTerminal(shellId, read) {
	setRivetClient(
		{
			getForId: () => ({
				terminal: {
					list: async () => [
						{ terminal: { generation: 1, shellId }, pid: 7, running: true },
					],
					output: { read },
				},
			}),
		},
		`actor-${shellId}`,
	);
}

test("terminal snapshot reads within the actor's 256-event page limit", async () => {
	const requests = [];
	useTerminal("paged", async (request) => {
		requests.push(request);
		const first = Number(request.after ?? -1) + 1;
		const count = Math.min(request.maxEvents, 300 - first);
		const events = Array.from({ length: count }, (_, index) => ({
			sequence: first + index,
			data: new Uint8Array([65]),
		}));
		return {
			generation: 1,
			events,
			nextCursor: events.at(-1)?.sequence ?? null,
			hasMore: first + count < 300,
			truncated: false,
		};
	});

	const snapshot = await agentOsSource.shellSnapshot("paged");
	assert.equal(new TextDecoder().decode(snapshot.data), "A".repeat(300));
	assert.equal(snapshot.seq, 299);
	assert.deepEqual(requests.map((request) => request.maxEvents), [256, 256]);
	assert.deepEqual(requests.map((request) => request.maxBytes), [700 * 1024, 700 * 1024]);
	assert.deepEqual(requests.map((request) => request.after), [undefined, 255]);
});

test("empty snapshot keeps sequence zero available for buffered live output", async () => {
	useTerminal("empty", async () => ({
		generation: 1,
		events: [],
		nextCursor: null,
		hasMore: false,
		truncated: false,
	}));

	const snapshot = await agentOsSource.shellSnapshot("empty");
	assert.equal(snapshot.seq, -1);
	assert.equal(snapshot.data.byteLength, 0);
});

test("terminal snapshot stops an endlessly growing replay", async () => {
	let reads = 0;
	useTerminal("growing", async () => {
		const sequence = reads++;
		return {
			generation: 1,
			events: [{ sequence, data: new Uint8Array([65]) }],
			nextCursor: sequence,
			hasMore: true,
			truncated: false,
		};
	});

	await assert.rejects(
		agentOsSource.shellSnapshot("growing"),
		/8-page inspector limit/,
	);
	assert.equal(reads, 8);
});

function page(events, hasMore = false, truncated = false) {
	return {
		generation: 1,
		events: events.map(([sequence, data]) => ({ sequence, data })),
		nextCursor: events.at(-1)?.[0] ?? null,
		hasMore,
		truncated,
	};
}

test("snapshot retains exact bigint cursors and reports truncation on any page", async () => {
	const sequence = 9007199254740992n;
	let reads = 0;
	useTerminal("bigint", async (request) => {
		assert.equal(request.after, reads === 0 ? undefined : sequence);
		return page([[sequence + BigInt(reads), Uint8Array.of(65)]], reads++ === 0, reads === 2);
	});
	const snapshot = await agentOsSource.shellSnapshot("bigint");
	assert.equal(snapshot.seq, sequence + 1n);
	assert.equal(snapshot.truncated, true);
});

test("snapshot preserves an incomplete UTF-8 character for the live decoder", async () => {
	useTerminal("utf8", async () => page([[0, Uint8Array.of(0xe2, 0x82)]]));
	const snapshot = await agentOsSource.shellSnapshot("utf8");
	const decoder = new TextDecoder();
	assert.equal(decoder.decode(snapshot.data, { stream: true }), "");
	assert.equal(decoder.decode(Uint8Array.of(0xac)), "€");
});

test("snapshot allows exactly 2 MiB but rejects larger aggregate replay", async () => {
	for (const extra of [0, 1]) {
		let reads = 0;
		const shell = `bytes-${extra}`;
		useTerminal(shell, async () => {
			const sequence = reads++;
			return page([[sequence, new Uint8Array(512 * 1024 + (sequence === 3 ? extra : 0))]], reads < 4);
		});
		if (extra) {
			await assert.rejects(agentOsSource.shellSnapshot(shell), /2 MiB inspector limit/);
		} else {
			assert.equal((await agentOsSource.shellSnapshot(shell)).data.byteLength, 2 * 1024 * 1024);
		}
	}
});

test("snapshot rejects missing, stalled, and inconsistent cursors", async () => {
	for (const [name, response] of [
		["missing", { ...page([[0, Uint8Array.of(65)]], true), nextCursor: null }],
		["empty", { ...page([], true), nextCursor: 0 }],
		["empty-cursor", { ...page([]), nextCursor: 0 }],
		["mismatch", { ...page([[0, Uint8Array.of(65)]]), nextCursor: 1 }],
		["unordered", page([[1, Uint8Array.of(65)], [0, Uint8Array.of(66)]])],
	]) {
		useTerminal(`invalid-${name}`, async () => response);
		await assert.rejects(agentOsSource.shellSnapshot(`invalid-${name}`), /cursor|out-of-order/);
	}
	let reads = 0;
	useTerminal("stalled", async () => {
		reads++;
		return page([[0, Uint8Array.of(65)]], true);
	});
	await assert.rejects(agentOsSource.shellSnapshot("stalled"), /out-of-order/);
	assert.equal(reads, 2);
});

test("snapshot rejects responses exceeding either requested page bound", async () => {
	for (const [name, events] of [
		["events", Array.from({ length: 257 }, (_, seq) => [seq, Uint8Array.of(65)])],
		["bytes", [[0, new Uint8Array(700 * 1024 + 1)]]],
	]) {
		useTerminal(`page-limit-${name}`, async () => page(events));
		await assert.rejects(agentOsSource.shellSnapshot(`page-limit-${name}`), /page limit/);
	}
});

test("disposing a pane stops pagination after the in-flight response", async () => {
	const controller = new AbortController();
	let reads = 0;
	useTerminal("aborted", async () => {
		reads++;
		controller.abort();
		return page([[0, Uint8Array.of(65)]], true);
	});
	await assert.rejects(agentOsSource.shellSnapshot("aborted", "screen", controller.signal), { name: "AbortError" });
	assert.equal(reads, 1);
});

test("an actor change during replay cannot route later pages to the new actor", async () => {
	let reads = 0;
	useTerminal("actor-switch", async () => {
		const sequence = reads++;
		setRivetClient({ getForId: () => { throw new Error("wrong actor"); } }, "other-actor");
		return page([[sequence, Uint8Array.of(65)]], reads === 1);
	});
	assert.equal((await agentOsSource.shellSnapshot("actor-switch")).data.byteLength, 2);
});

test("router accepts sequence zero after an empty snapshot and deduplicates late broadcasts", () => {
	const warnings = [];
	const chunks = [];
	const router = new OutputRouter((error) => warnings.push(error));
	router.push("zero", 0, Uint8Array.of(65));
	router.attach("zero", (chunk) => chunks.push(...chunk), -1);
	router.push("zero", 0, Uint8Array.of(65));
	router.push("zero", 1, Uint8Array.of(66));
	assert.deepEqual(chunks, [65, 66]);
	assert.deepEqual(warnings, []);

	router.attach("replayed", (chunk) => chunks.push(...chunk), 2);
	router.push("replayed", 1, Uint8Array.of(65));
	router.push("replayed", 2, Uint8Array.of(66));
	router.push("replayed", 3, Uint8Array.of(67));
	assert.deepEqual(chunks, [65, 66, 67]);
});

test("router keeps bigint watermarks precise", () => {
	const chunks = [];
	const router = new OutputRouter((error) => { throw error; });
	const sequence = 9007199254740992n;
	router.attach("big", (chunk) => chunks.push(...chunk), sequence);
	router.push("big", sequence, Uint8Array.of(65));
	router.push("big", sequence + 1n, Uint8Array.of(66));
	assert.deepEqual(chunks, [66]);
});

test("buffer overflow is bounded, reported once, and ignored when replay covered it", () => {
	for (const covered of [false, true]) {
		const warnings = [];
		const chunks = [];
		const router = new OutputRouter((error) => warnings.push(error.message));
		router.push("large", 0, new Uint8Array(256 * 1024 + 1));
		router.push("large", 1, Uint8Array.of(65));
		router.attach("large", (chunk) => chunks.push(...chunk), covered ? 0 : -1);
		assert.deepEqual(chunks, [65]);
		assert.equal(warnings.length, covered ? 0 : 1);
		if (!covered) assert.match(warnings[0], /truncated/);
	}
});

test("event count bounds zero-byte pending chunks and stale detach cannot remove a new writer", () => {
	const warnings = [];
	const chunks = [];
	const router = new OutputRouter((error) => warnings.push(error));
	for (let seq = 0; seq < 2049; seq++) router.push("events", seq, new Uint8Array());
	const detach = router.attach("events", () => {});
	assert.equal(warnings.length, 1);
	router.attach("events", (chunk) => chunks.push(...chunk), 2048);
	detach();
	router.push("events", 2049, Uint8Array.of(65));
	assert.deepEqual(chunks, [65]);
});
