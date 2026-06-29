import { randomUUID } from "node:crypto";
import type { BaseFilesystemSnapshot } from "./base-filesystem.js";
import {
	createFilesystemFromEntries,
	type FilesystemEntry,
	snapshotVirtualFilesystem,
} from "./filesystem-snapshot.js";
import { createOverlayBackend } from "./overlay-filesystem.js";
import type { VirtualFileSystem } from "./runtime-compat.js";

export type OverlayFilesystemMode = "ephemeral" | "read-only";

export interface FilesystemSnapshotExport {
	format: "agentos-filesystem-snapshot-v1";
	filesystem: {
		entries: FilesystemEntry[];
	};
}

export type RootSnapshotExport = {
	kind: "snapshot-export";
	source: FilesystemSnapshotExport;
};

export interface LayerHandle {
	kind: "writable" | "snapshot";
	storeId: string;
	layerId: string;
}

export interface WritableLayerHandle extends LayerHandle {
	kind: "writable";
	leaseId: string;
}

export interface SnapshotLayerHandle extends LayerHandle {
	kind: "snapshot";
}

export type SnapshotImportSource =
	| {
			kind: "base-filesystem-artifact";
			source: BaseFilesystemSnapshot | unknown;
	  }
	| { kind: "snapshot-export"; source: FilesystemSnapshotExport | unknown };

export interface LayerStore {
	readonly storeId: string;
	createWritableLayer(): Promise<WritableLayerHandle>;
	importSnapshot(source: SnapshotImportSource): Promise<SnapshotLayerHandle>;
	openSnapshotLayer(layerId: string): Promise<SnapshotLayerHandle>;
	sealLayer(layer: WritableLayerHandle): Promise<SnapshotLayerHandle>;
	createOverlayFilesystem(
		options:
			| {
					mode?: "ephemeral";
					upper: WritableLayerHandle;
					lowers: SnapshotLayerHandle[];
			  }
			| {
					mode: "read-only";
					lowers: SnapshotLayerHandle[];
			  },
	): VirtualFileSystem;
	dispose(): void;
}

interface WritableLayerState {
	kind: "writable";
	// Cleared once the layer is sealed so the heavy filesystem payload is
	// released; the (now invalid) state is kept as a tombstone.
	fs: VirtualFileSystem | null;
	leaseId: string;
	valid: boolean;
	activeOverlay: VirtualFileSystem | null;
}

interface SnapshotLayerState {
	kind: "snapshot";
	snapshot: FilesystemSnapshotExport;
	fs: VirtualFileSystem;
}

type LayerState = WritableLayerState | SnapshotLayerState;

function cloneSnapshotHandle(
	storeId: string,
	layerId: string,
): SnapshotLayerHandle {
	return { kind: "snapshot", storeId, layerId };
}

function cloneWritableHandle(
	storeId: string,
	layerId: string,
	leaseId: string,
): WritableLayerHandle {
	return { kind: "writable", storeId, layerId, leaseId };
}

function isBaseFilesystemSnapshot(
	value: unknown,
): value is BaseFilesystemSnapshot {
	if (!value || typeof value !== "object") {
		return false;
	}

	const filesystem = (value as { filesystem?: unknown }).filesystem;
	if (!filesystem || typeof filesystem !== "object") {
		return false;
	}

	return Array.isArray((filesystem as { entries?: unknown }).entries);
}

function isFilesystemSnapshotExport(
	value: unknown,
): value is FilesystemSnapshotExport {
	if (!value || typeof value !== "object") {
		return false;
	}

	if (
		(value as { format?: unknown }).format !== "agentos-filesystem-snapshot-v1"
	) {
		return false;
	}

	const filesystem = (value as { filesystem?: unknown }).filesystem;
	if (!filesystem || typeof filesystem !== "object") {
		return false;
	}

	return Array.isArray((filesystem as { entries?: unknown }).entries);
}

function normalizeSnapshotExport(
	source: SnapshotImportSource,
): FilesystemSnapshotExport {
	if (source.kind === "base-filesystem-artifact") {
		if (!isBaseFilesystemSnapshot(source.source)) {
			throw new Error("Invalid base filesystem artifact");
		}
		return {
			format: "agentos-filesystem-snapshot-v1",
			filesystem: {
				entries: source.source.filesystem.entries,
			},
		};
	}

	if (!isFilesystemSnapshotExport(source.source)) {
		throw new Error("Invalid snapshot export");
	}

	return source.source;
}

export function createSnapshotExport(
	entries: FilesystemEntry[],
): RootSnapshotExport {
	return {
		kind: "snapshot-export",
		source: {
			format: "agentos-filesystem-snapshot-v1",
			filesystem: { entries },
		},
	};
}

export function createInMemoryLayerStore(): LayerStore {
	const storeId = `memory-layer-store:${randomUUID()}`;
	const layers = new Map<string, LayerState>();

	function getLayerState(handle: LayerHandle): LayerState {
		if (handle.storeId !== storeId) {
			throw new Error(
				`Layer ${handle.layerId} belongs to store ${handle.storeId}, not ${storeId}`,
			);
		}

		const state = layers.get(handle.layerId);
		if (!state) {
			throw new Error(`Unknown layer: ${handle.layerId}`);
		}
		if (state.kind !== handle.kind) {
			throw new Error(`Layer kind mismatch for ${handle.layerId}`);
		}
		return state;
	}

	const store: LayerStore & {
		/** test-only: number of layer states still retained by the store */
		readonly retainedLayerCount: number;
	} = {
		storeId,

		get retainedLayerCount(): number {
			return layers.size;
		},

		async createWritableLayer(): Promise<WritableLayerHandle> {
			const layerId = randomUUID();
			const leaseId = randomUUID();
			layers.set(layerId, {
				kind: "writable",
				fs: await createFilesystemFromEntries([
					{
						path: "/",
						type: "directory",
						mode: "0755",
						uid: 0,
						gid: 0,
					},
				]),
				leaseId,
				valid: true,
				activeOverlay: null,
			});
			return cloneWritableHandle(storeId, layerId, leaseId);
		},

		async importSnapshot(
			source: SnapshotImportSource,
		): Promise<SnapshotLayerHandle> {
			const snapshot = normalizeSnapshotExport(source);
			const layerId = randomUUID();
			layers.set(layerId, {
				kind: "snapshot",
				snapshot,
				fs: await createFilesystemFromEntries(snapshot.filesystem.entries),
			});
			return cloneSnapshotHandle(storeId, layerId);
		},

		async openSnapshotLayer(layerId: string): Promise<SnapshotLayerHandle> {
			const state = layers.get(layerId);
			if (!state || state.kind !== "snapshot") {
				throw new Error(`Unknown snapshot layer: ${layerId}`);
			}
			return cloneSnapshotHandle(storeId, layerId);
		},

		async sealLayer(layer: WritableLayerHandle): Promise<SnapshotLayerHandle> {
			const state = getLayerState(layer);
			if (state.kind !== "writable") {
				throw new Error(`Layer ${layer.layerId} is not writable`);
			}
			const baseFs = state.activeOverlay ?? state.fs;
			if (!state.valid || state.leaseId !== layer.leaseId || !baseFs) {
				throw new Error(`Writable layer ${layer.layerId} is no longer valid`);
			}

			const entries = await snapshotVirtualFilesystem(baseFs);
			const snapshot = createSnapshotExport(entries).source;
			const layerId = randomUUID();

			layers.set(layerId, {
				kind: "snapshot",
				snapshot,
				fs: await createFilesystemFromEntries(snapshot.filesystem.entries),
			});

			// Release the writable layer's filesystem payload now that it is sealed;
			// the invalid tombstone is retained so stale handles still report cleanly.
			state.valid = false;
			state.activeOverlay = null;
			state.fs = null;

			return cloneSnapshotHandle(storeId, layerId);
		},

		createOverlayFilesystem(options): VirtualFileSystem {
			const lowers = options.lowers.map((lower) => {
				const state = getLayerState(lower);
				if (state.kind !== "snapshot") {
					throw new Error(`Layer ${lower.layerId} is not a snapshot`);
				}
				return lower;
			});

			if (options.mode === "read-only") {
				return createOverlayBackend({
					mode: "read-only",
					lowers: lowers.map((lower) => {
						const state = getLayerState(lower);
						if (state.kind !== "snapshot") {
							throw new Error(`Layer ${lower.layerId} is not a snapshot`);
						}
						return state.fs;
					}),
				});
			}

			const upperState = getLayerState(options.upper);
			if (upperState.kind !== "writable") {
				throw new Error(`Layer ${options.upper.layerId} is not writable`);
			}
			if (!upperState.valid || upperState.leaseId !== options.upper.leaseId) {
				throw new Error(
					`Writable layer ${options.upper.layerId} is no longer valid`,
				);
			}
			if (upperState.activeOverlay) {
				throw new Error(
					`Writable layer ${options.upper.layerId} is already attached to an overlay`,
				);
			}
			if (!upperState.fs) {
				throw new Error(
					`Writable layer ${options.upper.layerId} is no longer valid`,
				);
			}

			const overlay = createOverlayBackend({
				upper: upperState.fs,
				lowers: lowers.map((lower) => {
					const state = getLayerState(lower);
					if (state.kind !== "snapshot") {
						throw new Error(`Layer ${lower.layerId} is not a snapshot`);
					}
					return state.fs;
				}),
			});
			upperState.activeOverlay = overlay;
			return overlay;
		},

		dispose(): void {
			// Release every retained layer's filesystem payload so the store does
			// not accumulate snapshots/writable layers for the process lifetime.
			layers.clear();
		},
	};

	return store;
}
