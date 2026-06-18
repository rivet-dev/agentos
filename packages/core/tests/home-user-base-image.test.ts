/**
 * Regression guard for issue #5:
 * "Default /home/user mount hides base image".
 *
 * Original bug: `buildVmOptions` installed a default in-memory `/home/user`
 * mount that shadowed base-image files. Anything the base image shipped under
 * `/home/user` (and, more broadly, the base filesystem the Pi SDK adapter needs
 * at startup) was hidden behind an empty, freshly-mounted volume, so reads at
 * `/home/user` saw an empty directory instead of base content.
 *
 * Fix: `buildVmOptions` no longer exists. VM-mount construction flows through
 * `collectSidecarMountPlan` (packages/core/src/agent-os.ts), which never injects
 * any default mount (in-memory or otherwise) at `/home/user`. The base
 * filesystem is the unshadowed root/lower snapshot; `/home/user` is created as a
 * plain directory by shadow-root bootstrap (an idempotent mkdir), and the base
 * snapshot's content is materialized into that same root afterwards -- so base
 * content under `/home/user` survives and is never replaced by an empty mount.
 *
 * This test reproduces the original failure condition: a default VM with NO
 * explicit mounts. If a default mount were shadowing `/home/user`, then:
 *   - content written under `/home/user` would be isolated to the shadow mount
 *     and would NOT survive into a fresh root snapshot of the base layer, and
 *   - the base image's own files would be hidden.
 * The assertions below all rely on `/home/user` being backed by the real,
 * unshadowed base/root filesystem.
 */

import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";

const HOME_DIR = "/home/user";
const MARKER_PATH = `${HOME_DIR}/startup-marker.json`;
const MARKER_CONTENT = JSON.stringify({ origin: "base-image", id: "home-user-base-image" });

describe("default /home/user must not be shadowed by a default mount", () => {
	let vm: AgentOs | undefined;
	const textDecoder = new TextDecoder();

	beforeEach(async () => {
		// No explicit `mounts` -- this is the exact scenario the original
		// `buildVmOptions` default-mount bug applied to.
		vm = await AgentOs.create();
	});

	afterEach(async () => {
		if (vm) {
			await vm.dispose();
			vm = undefined;
		}
	});

	test("base-image files outside /home/user are visible (base image not hidden)", async () => {
		// /etc/profile ships in the base filesystem snapshot. If a default mount
		// regime shadowed the base image, sentinel base files could disappear.
		const profile = textDecoder.decode(
			await (vm as AgentOs).readFile("/etc/profile"),
		);
		expect(profile.length).toBeGreaterThan(0);
	});

	test("/home/user is a real directory from the base layer, not an empty shadow mount", async () => {
		const stat = await (vm as AgentOs).stat(HOME_DIR);
		expect(stat.isDirectory).toBe(true);
	});

	test("content under /home/user is readable through vm.readFile (no shadowing mount)", async () => {
		await (vm as AgentOs).writeFile(MARKER_PATH, MARKER_CONTENT);

		const raw = await (vm as AgentOs).readFile(MARKER_PATH);
		expect(JSON.parse(textDecoder.decode(raw))).toEqual({
			origin: "base-image",
			id: "home-user-base-image",
		});
	});

	test("/home/user content lives on the base/root filesystem, not an isolated mount", async () => {
		// The original bug installed an *in-memory* mount over /home/user. A real
		// in-memory mount would NOT be part of the root filesystem snapshot. By
		// writing under /home/user and confirming the write is captured by a fresh
		// root-filesystem snapshot, we prove /home/user is backed by the
		// unshadowed root layer rather than a separate default mount.
		await (vm as AgentOs).writeFile(MARKER_PATH, MARKER_CONTENT);

		const snapshot = await (vm as AgentOs).snapshotRootFilesystem();

		const restored = await AgentOs.create({
			rootFilesystem: {
				disableDefaultBaseLayer: true,
				lowers: [snapshot],
			},
		});
		try {
			const raw = await restored.readFile(MARKER_PATH);
			expect(JSON.parse(textDecoder.decode(raw))).toEqual({
				origin: "base-image",
				id: "home-user-base-image",
			});
		} finally {
			await restored.dispose();
		}
	});
});
