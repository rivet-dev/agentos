import { describe, expect, it } from "vitest";
import { isPackageDescriptor } from "../src/agentos-package.js";

// The package PROJECTION (staging dir, bin/current/man symlink farms, the shared
// hardlink content cache, version-keyed invalidation) now lives in the secure-exec
// SIDECAR — see `crates/sidecar/tests/package_projection.rs` for the on-disk layout
// + inode-sharing assertions. The only thing left client-side is the descriptor
// surface, so this is a thin discriminator test.
describe("agentos-package descriptor surface", () => {
	it("discriminates the dir-only package descriptor", () => {
		expect(isPackageDescriptor("/x")).toBe(true);
		expect(isPackageDescriptor({ name: "p", dir: "/x" })).toBe(false);
		expect(isPackageDescriptor({ dir: "/x" })).toBe(false);
		expect(isPackageDescriptor(null)).toBe(false);
	});
});
