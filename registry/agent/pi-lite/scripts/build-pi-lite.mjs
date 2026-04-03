#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import {
	mkdirSync,
	statSync,
	writeFileSync,
	copyFileSync,
	chmodSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { homedir } from "node:os";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const packageDir = resolve(__dirname, "..");
const sourceDir = resolve(
	process.env.PI_LITE_SOURCE_DIR ?? join(homedir(), "misc", "pi_agent_rust"),
);
const outputDir = resolve(packageDir, "bin");
const outputBin = resolve(outputDir, "pi-lite");
const manifestPath = resolve(outputDir, "pi-lite.manifest.json");
const sqliteBuildDir = resolve(sourceDir, "target/pi-lite-sqlite");
const builtSqliteLib = resolve(sqliteBuildDir, "libsqlite3.so");
const outputSqliteLib = resolve(outputDir, "libsqlite3.so");
const stubPath = resolve(
	sourceDir,
	"legacy_pi_mono_code/pi-mono/packages/ai/src/models.generated.ts",
);
const builtBin = resolve(sourceDir, "target/release/pi");

function run(command, args, options = {}) {
	const result = spawnSync(command, args, {
		stdio: "inherit",
		...options,
	});
	if (result.status !== 0) {
		throw new Error(
			`Command failed (${result.status ?? "unknown"}): ${command} ${args.join(" ")}`,
		);
	}
	return result;
}

function findBundledSqliteSource() {
	const result = spawnSync(
		"bash",
		[
			"-lc",
			[
				"find",
				resolve(homedir(), ".cargo/registry/src"),
				"-path",
				"'*libsqlite3-sys-*/sqlite3/sqlite3.c'",
				"|",
				"head",
				"-n",
				"1",
			].join(" "),
		],
		{
			encoding: "utf8",
		},
	);
	if (result.status !== 0 || !result.stdout.trim()) {
		throw new Error("Unable to locate bundled sqlite3.c from libsqlite3-sys");
	}
	return result.stdout.trim();
}

mkdirSync(dirname(stubPath), { recursive: true });
mkdirSync(sqliteBuildDir, { recursive: true });
try {
	statSync(stubPath);
} catch {
	writeFileSync(
		stubPath,
		[
			"// Stub generated for local Agent OS pi-lite builds when the pi-mono submodule is absent.",
			"// Upstream uses the same approach in its release workflow.",
			"export const MODELS = {} as const;",
			"",
		].join("\n"),
		"utf8",
	);
}

run(
	"cc",
	[
		"-O2",
		"-fPIC",
		"-shared",
		findBundledSqliteSource(),
		"-o",
		builtSqliteLib,
		"-ldl",
		"-lpthread",
	],
	{
		cwd: sourceDir,
	},
);

run(
	"cargo",
	["build", "--release", "--locked", "--bin", "pi", "--no-default-features"],
	{
		cwd: sourceDir,
		env: {
			...process.env,
			LIBRARY_PATH: [sqliteBuildDir, process.env.LIBRARY_PATH]
				.filter(Boolean)
				.join(":"),
			CARGO_PROFILE_RELEASE_OPT_LEVEL:
				process.env.CARGO_PROFILE_RELEASE_OPT_LEVEL ?? "z",
			CARGO_PROFILE_RELEASE_LTO: process.env.CARGO_PROFILE_RELEASE_LTO ?? "fat",
			CARGO_PROFILE_RELEASE_CODEGEN_UNITS:
				process.env.CARGO_PROFILE_RELEASE_CODEGEN_UNITS ?? "1",
			CARGO_PROFILE_RELEASE_PANIC:
				process.env.CARGO_PROFILE_RELEASE_PANIC ?? "abort",
			CARGO_PROFILE_RELEASE_STRIP:
				process.env.CARGO_PROFILE_RELEASE_STRIP ?? "symbols",
			CARGO_PROFILE_RELEASE_DEBUG:
				process.env.CARGO_PROFILE_RELEASE_DEBUG ?? "0",
		},
	},
);

mkdirSync(outputDir, { recursive: true });
copyFileSync(builtBin, outputBin);
copyFileSync(builtSqliteLib, outputSqliteLib);
chmodSync(outputBin, 0o755);
chmodSync(outputSqliteLib, 0o755);

const gitHead = spawnSync("git", ["rev-parse", "HEAD"], {
	cwd: sourceDir,
	encoding: "utf8",
});

writeFileSync(
	manifestPath,
	`${JSON.stringify(
		{
			sourceDir,
			sourceCommit:
				gitHead.status === 0 ? gitHead.stdout.trim() : undefined,
			build: {
				command: "cargo build --release --locked --bin pi --no-default-features",
				profileOverrides: {
					optLevel: process.env.CARGO_PROFILE_RELEASE_OPT_LEVEL ?? "z",
					lto: process.env.CARGO_PROFILE_RELEASE_LTO ?? "fat",
					codegenUnits:
						process.env.CARGO_PROFILE_RELEASE_CODEGEN_UNITS ?? "1",
					panic: process.env.CARGO_PROFILE_RELEASE_PANIC ?? "abort",
					strip: process.env.CARGO_PROFILE_RELEASE_STRIP ?? "symbols",
					debug: process.env.CARGO_PROFILE_RELEASE_DEBUG ?? "0",
				},
			},
			output: {
				path: outputBin,
				sizeBytes: statSync(outputBin).size,
			},
			sqliteLibrary: {
				path: outputSqliteLib,
				sizeBytes: statSync(outputSqliteLib).size,
			},
		},
		null,
		2,
	)}\n`,
	"utf8",
);
