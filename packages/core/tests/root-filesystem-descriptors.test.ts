import { describe, expect, test } from "vitest";
import type { FilesystemEntry } from "../src/filesystem-snapshot.js";
import { createSnapshotExport } from "../src/index.js";
import { serializeRootFilesystemForSidecar } from "../src/sidecar/rpc-client.js";

function toExpectedSidecarEntry(entry: FilesystemEntry) {
	const mode = Number.parseInt(entry.mode, 8);
	return {
		path: entry.path,
		kind: entry.type,
		mode,
		uid: entry.uid,
		gid: entry.gid,
		content: entry.content,
		encoding: entry.encoding,
		target: entry.target,
		executable: entry.type === "file" && (mode & 0o111) !== 0,
	};
}

describe("sidecar root filesystem descriptors", () => {
	test("serializes only caller-provided root filesystem lowers", () => {
		const configLower = createSnapshotExport([
			{
				path: "/workspace",
				type: "directory",
				mode: "0755",
				uid: 0,
				gid: 0,
			},
			{
				path: "/workspace/run.sh",
				type: "file",
				mode: "0755",
				uid: 0,
				gid: 0,
				content: "echo hi\n",
				encoding: "utf8",
			},
			{
				path: "/workspace/payload.bin",
				type: "file",
				mode: "0644",
				uid: 1000,
				gid: 1000,
				content: "AAEC",
				encoding: "base64",
			},
			{
				path: "/workspace/current",
				type: "symlink",
				mode: "0777",
				uid: 0,
				gid: 0,
				target: "/workspace/run.sh",
			},
		]);
		expect(
			serializeRootFilesystemForSidecar({
				mode: "read-only",
				disableDefaultBaseLayer: true,
				lowers: [configLower],
			}),
		).toEqual({
			mode: "read-only",
			disableDefaultBaseLayer: true,
			lowers: [
				{
					kind: "snapshot",
					entries: configLower.source.filesystem.entries.map(
						toExpectedSidecarEntry,
					),
				},
			],
		});
	});

	test("inlines the bundled base lower when callers place it explicitly", () => {
		const descriptor = serializeRootFilesystemForSidecar({
			disableDefaultBaseLayer: true,
			lowers: [{ kind: "bundled-base-filesystem" }],
		});

		expect(descriptor.mode).toBeUndefined();
		expect(descriptor.disableDefaultBaseLayer).toBe(true);
		expect(descriptor.bootstrapEntries).toBeUndefined();
		expect(descriptor.lowers).toHaveLength(1);
		expect(descriptor.lowers[0]).toEqual({
			kind: "bundledBaseFilesystem",
		});
	});

	test("does not materialize omitted sidecar defaults", () => {
		expect(serializeRootFilesystemForSidecar()).toEqual({});
		expect(
			serializeRootFilesystemForSidecar({
				mode: "ephemeral",
				disableDefaultBaseLayer: false,
				lowers: [],
			}),
		).toEqual({
			mode: "ephemeral",
			disableDefaultBaseLayer: false,
			lowers: [],
		});
	});
});
