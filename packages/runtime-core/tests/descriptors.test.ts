import { describe, expect, it } from "vitest";
import {
	toGeneratedMountDescriptor,
	toGeneratedPackageDescriptor,
	toGeneratedSidecarPlacement,
} from "../src/descriptors.js";

describe("descriptors", () => {
	it("maps shared and explicit sidecar placements", () => {
		expect(toGeneratedSidecarPlacement({ kind: "shared" })).toEqual({
			tag: "SidecarPlacementShared",
			val: { pool: null },
		});
		expect(
			toGeneratedSidecarPlacement({ kind: "shared", pool: "workers" }),
		).toEqual({
			tag: "SidecarPlacementShared",
			val: { pool: "workers" },
		});
		expect(
			toGeneratedSidecarPlacement({
				kind: "explicit",
				sidecar_id: "sidecar-1",
			}),
		).toEqual({
			tag: "SidecarPlacementExplicit",
			val: { sidecarId: "sidecar-1" },
		});
	});

	it("maps mount descriptors and serializes plugin config as JSON", () => {
		expect(
			toGeneratedMountDescriptor({
				guest_path: "/workspace",
				read_only: true,
				plugin: {
					id: "host",
					config: { source: "/tmp/project", depth: 2 },
				},
			}),
		).toEqual({
			guestPath: "/workspace",
			readOnly: true,
			plugin: {
				id: "host",
				config: '{"source":"/tmp/project","depth":2}',
			},
		});
	});

	it("preserves omitted mount defaults for sidecar normalization", () => {
		expect(
			toGeneratedMountDescriptor({
				guest_path: "/workspace",
				plugin: { id: "js_bridge" },
			}),
		).toEqual({
			guestPath: "/workspace",
			readOnly: null,
			plugin: { id: "js_bridge", config: null },
		});
	});

	it("forwards package paths or exact opaque package bytes without metadata", () => {
		expect(toGeneratedPackageDescriptor({ path: "/packages/demo.aospkg" })).toEqual({
			tag: "PackagePath",
			val: { path: "/packages/demo.aospkg" },
		});

		const backing = new Uint8Array([99, 1, 2, 3, 88]);
		const content = backing.subarray(1, 4);
		const encoded = toGeneratedPackageDescriptor({ content });
		expect(encoded.tag).toBe("PackageInline");
		if (encoded.tag !== "PackageInline") throw new Error("expected inline package");
		expect(new Uint8Array(encoded.val.content)).toEqual(new Uint8Array([1, 2, 3]));
		expect(encoded.val.content.byteLength).toBe(3);
	});
});
