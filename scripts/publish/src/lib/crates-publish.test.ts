import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { findCratePublishOrderProblems } from "./crates-publish.js";

function write(root: string, rel: string, contents: string) {
	const path = join(root, rel);
	mkdirSync(join(path, ".."), { recursive: true });
	writeFileSync(path, contents);
}

function crate(root: string, name: string, extra = "") {
	write(
		root,
		`crates/${name}/Cargo.toml`,
		`[package]\nname = "${name}"\nversion.workspace = true\nedition = "2021"\nlicense = "MIT"\ndescription = "x"\n${extra}`,
	);
	write(root, `crates/${name}/src/lib.rs`, "");
}

// Real cargo packages a small workspace; the check reads the tarballs, as it
// does for the agentOS crates.
test("a crate listed before an optional agentOS dependency fails the rehearsal", () => {
	const root = mkdtempSync(join(tmpdir(), "publish-crate-order-"));
	try {
		write(
			root,
			"Cargo.toml",
			[
				"[workspace]",
				'members = ["crates/agentos-client", "crates/agentos-vm"]',
				'resolver = "2"',
				"[workspace.package]",
				'version = "0.0.0-rehearsal.0"',
				"[workspace.dependencies]",
				'agentos-client = { path = "crates/agentos-client", version = "0.0.0-rehearsal.0" }',
				"",
			].join("\n"),
		);
		crate(root, "agentos-client");
		crate(root, "agentos-vm", "[dependencies]\nagentos-client = { workspace = true, optional = true }\n");
		const order = ["agentos-vm", "agentos-client"];
		const targetDir = join(root, "target");
		execFileSync(
			"cargo",
			["package", "--offline", "--allow-dirty", "--no-verify", "-p", "agentos-vm", "-p", "agentos-client"],
			{ cwd: root, stdio: "ignore", env: { ...process.env, CARGO_TARGET_DIR: targetDir } },
		);

		const problems = findCratePublishOrderProblems(root, order, "0.0.0-rehearsal.0", targetDir);

		assert.deepEqual(problems, [
			"agentos-vm depends on agentos-client (normal, optional), but agentos-client publishes later (position 2 > 1 in RUST_CRATE_ORDER); cargo publish -p agentos-vm cannot resolve it on crates.io",
		]);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});
