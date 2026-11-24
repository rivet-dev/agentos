import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
	mkdtempSync,
	mkdirSync,
	readFileSync,
	rmSync,
	utimesSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const toolchain = fileURLToPath(new URL("../toolchain/", import.meta.url));
const epoch = 1_500_000_000;
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex").slice(0, 16);

function fixture(run) {
	const dir = mkdtempSync(join(tmpdir(), "agentos-sysroot-order-"));
	try {
		const root = join(dir, "toolchain");
		const put = (name, contents, executable = false) => {
			const path = join(root, name);
			mkdirSync(dirname(path), { recursive: true });
			writeFileSync(path, contents, { mode: executable ? 0o755 : 0o644 });
			utimesSync(path, epoch, epoch);
			return path;
		};
		put("Makefile", readFileSync(join(toolchain, "Makefile")));
		put("c/Makefile", readFileSync(join(toolchain, "c/Makefile")));
		put("std-patches/wasi-libc-overrides/probe.c", "override-v1");
		put("std-patches/wasi-libc/probe.patch", "patch-v1");
		for (const name of [
			"build-llvm-runtimes.sh",
			"wasi-clang.sh",
			"without-wasm-opt.sh",
		]) {
			put(`c/scripts/${name}`, "unused fixture prerequisite\n");
		}
		put("c/vendor/wasi-sdk/bin/clang", "#!/bin/sh\nexit 97\n", true);
		put("stub-bin/cargo", "#!/bin/sh\nexit 97\n", true);
		put("stub-bin/rustc", "#!/bin/sh\nprintf '/unused/fixture\\n'\n", true);
		put(
			"scripts/patch-wasi-libc.sh",
			`#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');
const root = path.dirname(path.dirname(__filename));
if (process.env.AGENTOS_TEST_SYSROOT_FAIL === '1') process.exit(19);
const threaded = process.argv.includes('--threads');
const file = path.join(root, 'c', threaded ? 'sysroot-threads/lib/wasm32-wasi-threads/libc.a' : 'sysroot/lib/wasm32-wasi/libc.a');
const input = fs.readFileSync(path.join(root, 'std-patches/wasi-libc-overrides/probe.c'), 'utf8') + ':' + fs.readFileSync(path.join(root, 'std-patches/wasi-libc/probe.patch'), 'utf8');
fs.mkdirSync(path.dirname(file), { recursive: true });
fs.writeFileSync(file, input);
fs.utimesSync(file, ${epoch + 100}, ${epoch + 100});
fs.appendFileSync(path.join(root, 'build-events.jsonl'), JSON.stringify({ threaded }) + '\\n');
`,
			true,
		);
		put(
			"record.mjs",
			`import {readFileSync,writeFileSync} from 'node:fs';
const [name, flags, binaryen] = process.argv.slice(2);
writeFileSync(name+'.json',JSON.stringify({flags,binaryen,libc:readFileSync('c/sysroot/lib/wasm32-wasi/libc.a','utf8')}));
`,
		);
		// Keep the actual parent dependency graph and deferred flags. Only replace
		// compiler/vendor actions; the C Makefile and its prerequisites stay real.
		put(
			"harness.mk",
			`.PHONY: vendor patch-vendor patch-std
vendor patch-vendor patch-std:
\t@:
wasm wasm-cmd fd-mapping-contract-wasm:
\t@node record.mjs $@ '$(WASI_RUSTFLAGS)' '$(AGENTOS_ENABLE_BINARYEN)'
`,
		);
		for (const name of [
			"c/sysroot/lib/wasm32-wasi/libc.a",
			"c/sysroot-threads/lib/wasm32-wasi-threads/libc.a",
		]) {
			const path = put(name, "override-v1:patch-v1");
			utimesSync(path, epoch + 10, epoch + 10);
		}
		const exec = (args, env = {}) => {
			const result = spawnSync("make", ["--no-print-directory", ...args], {
				cwd: root,
				env: {
					...process.env,
					PATH: join(root, "stub-bin") + ":" + process.env.PATH,
					AGENTOS_ENABLE_BINARYEN: "0",
					AGENTOS_TEST_SYSROOT_FAIL: "0",
					MAKE: "make",
					MAKEFLAGS: "",
					MFLAGS: "",
					MAKEOVERRIDES: "",
					GNUMAKEFLAGS: "",
					...env,
				},
				encoding: "utf8",
				timeout: 10_000,
				maxBuffer: 1024 * 1024,
			});
			assert.ifError(result.error);
			return result;
		};
		const parent = (targets, env) =>
			exec(
				[
					"-f",
					"Makefile",
					"-f",
					"harness.mk",
					"RUST_STD_SRC=/unused/fixture",
					"-j4",
					...targets,
				],
				env,
			);
		const owner = (threaded = false) =>
			exec(["-C", "c", threaded ? "sysroot-threads" : "sysroot"]);
		const mutate = (name, value) => {
			const path = put(name, value);
			utimesSync(path, epoch + 20, epoch + 20);
		};
		const libc = (threaded = false) =>
			readFileSync(
				join(
					root,
					threaded
						? "c/sysroot-threads/lib/wasm32-wasi-threads/libc.a"
						: "c/sysroot/lib/wasm32-wasi/libc.a",
				),
				"utf8",
			);
		const record = (name) => JSON.parse(readFileSync(join(root, name + ".json"), "utf8"));
		const builds = () => {
			try {
				return readFileSync(join(root, "build-events.jsonl"), "utf8")
					.trim()
					.split("\n")
					.filter(Boolean).length;
			} catch (error) {
				if (error.code === "ENOENT") return 0;
				throw error;
			}
		};
		run({ root, parent, owner, mutate, libc, record, builds });
	} finally {
		rmSync(dir, { recursive: true, force: true });
	}
}

for (const target of ["wasm", "wasm-cmd", "fd-mapping-contract-wasm"]) {
	test(`${target} observes changed libc overrides before Rust links`, () =>
		fixture((f) => {
			f.mutate("std-patches/wasi-libc-overrides/probe.c", "override-v2");
			const result = f.parent([target]);
			assert.equal(result.status, 0, result.stderr);
			const rust = f.record(target);
			assert.equal(f.owner().status, 0);
			assert.equal(f.libc(), "override-v2:patch-v1");
			assert.equal(
				rust.libc,
				f.libc(),
				"Rust observed stale libc before the owning Makefile refreshed it",
			);
			assert(rust.flags.includes(`agentos-libc-${digest(f.libc())}-std-`));
			assert.equal(rust.binaryen, "0");
		}));
}

test("changed libc patches refresh the parent archive", () =>
	fixture((f) => {
		f.mutate("std-patches/wasi-libc/probe.patch", "patch-v2");
		assert.equal(f.parent(["wasm"]).status, 0);
		assert.equal(f.record("wasm").libc, "override-v1:patch-v2");
	}));

test("threaded parent proxy checks the owning archive inputs", () =>
	fixture((f) => {
		f.mutate("std-patches/wasi-libc-overrides/probe.c", "override-v2");
		assert.equal(f.parent(["sysroot-threads"]).status, 0);
		assert.equal(f.libc(true), "override-v2:patch-v1");
	}));

test("a missing archive is still created before Rust links", () =>
	fixture((f) => {
		rmSync(join(f.root, "c/sysroot/lib/wasm32-wasi/libc.a"));
		assert.equal(f.parent(["wasm"]).status, 0);
		assert.equal(f.record("wasm").libc, "override-v1:patch-v1");
		assert.equal(f.builds(), 1);
	}));

test("unchanged owned inputs do not rebuild libc", () =>
	fixture((f) => {
		assert.equal(f.parent(["wasm"]).status, 0);
		assert.equal(f.builds(), 0);
		const first = f.record("wasm");
		assert.equal(first.libc, "override-v1:patch-v1");
		assert.equal(f.parent(["wasm"]).status, 0);
		assert.equal(f.builds(), 0);
		assert.deepEqual(f.record("wasm"), first);
	}));

test("sysroot failure prevents the Rust consumer", () =>
	fixture((f) => {
		f.mutate("std-patches/wasi-libc-overrides/probe.c", "override-v2");
		assert.notEqual(f.parent(["wasm"], { AGENTOS_TEST_SYSROOT_FAIL: "1" }).status, 0);
		assert.throws(() => f.record("wasm"), { code: "ENOENT" });
	}));

test("parallel Rust consumers share one completed sysroot check", () =>
	fixture((f) => {
		f.mutate("std-patches/wasi-libc-overrides/probe.c", "override-v2");
		assert.equal(f.parent(["wasm", "wasm-cmd", "fd-mapping-contract-wasm"]).status, 0);
		assert.equal(f.builds(), 1);
		for (const name of ["wasm", "wasm-cmd", "fd-mapping-contract-wasm"]) {
			assert.equal(f.record(name).libc, "override-v2:patch-v1");
		}
	}));
