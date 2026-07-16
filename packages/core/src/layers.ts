import type { BaseFilesystemSnapshot } from "./base-filesystem.js";
import type { FilesystemEntry } from "./filesystem-snapshot.js";
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

/**
 * Caller-provided durable layer store. AgentOS forwards these explicit handles
 * but does not provide a second client-side filesystem or overlay engine.
 */
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
