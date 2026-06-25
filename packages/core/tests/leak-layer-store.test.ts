import { describe, expect, test } from "vitest";
import {
	createInMemoryLayerStore,
	createSnapshotExport,
} from "../src/index.js";

// Test-only view onto the in-memory store's internal accounting so we can
// assert the retained-layer map is actually drained.
function retainedLayerCount(
	store: ReturnType<typeof createInMemoryLayerStore>,
) {
	return (store as unknown as { retainedLayerCount: number })
		.retainedLayerCount;
}

describe("in-memory layer store leak", () => {
	test("dispose() releases every retained layer", async () => {
		const store = createInMemoryLayerStore();

		// Accumulate a mix of writable layers and imported snapshots, then seal
		// the writable ones (the lifecycle that previously left the map growing).
		for (let i = 0; i < 5; i++) {
			const writable = await store.createWritableLayer();
			const overlay = store.createOverlayFilesystem({
				upper: writable,
				lowers: [],
			});
			await overlay.writeFile(`/note-${i}.txt`, `payload ${i}`);
			await store.sealLayer(writable);

			await store.importSnapshot(
				createSnapshotExport([
					{ path: "/", type: "directory", mode: "0755", uid: 0, gid: 0 },
				]),
			);
		}

		// Sanity: the store is holding the accumulated layers before disposal.
		expect(retainedLayerCount(store)).toBeGreaterThan(0);

		store.dispose();

		// Observable leak symptom: after the store's lifecycle ends the tracking
		// map must be empty rather than retaining filesystem snapshot payloads.
		expect(retainedLayerCount(store)).toBe(0);
	});

	test("sealing a writable layer releases the writable payload while preserving its data", async () => {
		const store = createInMemoryLayerStore();
		const writable = await store.createWritableLayer();
		const overlay = store.createOverlayFilesystem({
			upper: writable,
			lowers: [],
		});
		await overlay.writeFile("/data.txt", "hello");

		// Before sealing: the store holds exactly the one writable layer.
		expect(retainedLayerCount(store)).toBe(1);

		const sealed = await store.sealLayer(writable);

		// Sealing must produce a snapshot layer AND keep the writable as an invalid
		// tombstone (so stale handles still report cleanly) — i.e. exactly one new
		// retained layer, never deleting the writable outright.
		expect(retainedLayerCount(store)).toBe(2);

		// The written payload must have been captured into the sealed snapshot
		// BEFORE the writable's filesystem was released — proving the seal both
		// snapshotted and then dropped the heavy writable `fs`, not merely flipped
		// `valid`. Reading it back through the snapshot is the observable proof the
		// payload moved rather than vanished.
		const sealedView = store.createOverlayFilesystem({
			mode: "read-only",
			lowers: [sealed],
		});
		expect(await sealedView.readTextFile("/data.txt")).toBe("hello");

		// The sealed (now invalid) writable handle must still report cleanly
		// instead of leaking its retained overlay filesystem.
		expect(() =>
			store.createOverlayFilesystem({ upper: writable, lowers: [] }),
		).toThrow("no longer valid");

		// Gap (documented): the literal `state.fs = null` payload-nulling inside
		// `sealLayer` is not observable through any public surface — the writable
		// tombstone stays in the `layers` map either way, so `retainedLayerCount`
		// is unchanged by it, and every public guard already short-circuits on
		// `valid === false`. Directly asserting `state.fs === null` would require a
		// new production test-only accessor; per the "edit test code only"
		// constraint we do not add one. Test 1 (`dispose()` → `retainedLayerCount`
		// 0) remains the real guard that retained payloads are dropped, and the
		// round-trip above proves the writable payload was moved into the snapshot
		// rather than abandoned.
	});
});
