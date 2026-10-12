#!/usr/bin/env node
// Stage vim's runtime tree into the gitignored `share/vim/vim92/` so
// `agentos-toolchain build` ships it in dist/package and the manifest's
// `provides.files` can overlay it read-only at /usr/local/share/vim/vim92
// (VIMRUNTIME points straight at it, bypassing vim's version-dir search).
//
// The runtime comes from $VIM_RUNTIME_SRC when explicitly overridden, or from
// the same pinned upstream checkout used to build the binary. Bulky,
// non-load-bearing subtrees (docs, tutor, spell dictionaries, translations)
// are trimmed — the runtime here exists so `vim` starts clean (defaults.vim,
// syntax, ftplugin, indent, autoload, colors), not to ship a manual.
import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repositoryRoot = resolve(packageRoot, "../..");
const target = join(packageRoot, "share", "vim", "vim92");
const pinnedSource = join(
	repositoryRoot,
	"toolchain",
	"c",
	"libs",
	"vim",
	"runtime",
);

const RUNTIME_DIRECTORIES = new Set([
	"autoload",
	"colors",
	"compiler",
	"ftplugin",
	"import",
	"indent",
	"macros",
	"pack",
	"plugin",
	"syntax",
]);
const DEVELOPMENT_DIRECTORIES = new Set(["generator", "testdir"]);

const source = process.env.VIM_RUNTIME_SRC ?? pinnedSource;
if (!existsSync(join(source, "defaults.vim")) && !process.env.VIM_RUNTIME_SRC) {
	console.log("stage-runtime: fetching the pinned Vim source");
	execFileSync(
		"make",
		["-C", join(repositoryRoot, "toolchain", "c"), "fetch-vim"],
		{ stdio: "inherit" },
	);
}
if (!existsSync(join(source, "defaults.vim"))) {
	throw new Error(
		`stage-runtime: Vim runtime not found at ${source}; set VIM_RUNTIME_SRC to a valid runtime directory`,
	);
}

function includeRuntimePath(parts) {
	if (parts.length === 0) return true;
	if (parts.length === 1 && parts[0].endsWith(".vim")) return true;
	return (
		RUNTIME_DIRECTORIES.has(parts[0]) &&
		!parts.some((segment) => DEVELOPMENT_DIRECTORIES.has(segment))
	);
}

// A partial source checkout must fail before replacing the staged runtime.
// Artifact inputs without Git metadata retain the existing staging path.
if (
	resolve(source) === pinnedSource &&
	existsSync(join(dirname(pinnedSource), ".git"))
) {
	const tracked = execFileSync(
		"git",
		[
			"--no-optional-locks", "-C", dirname(pinnedSource),
			"ls-tree", "-r", "--name-only", "-z", "HEAD", "--", "runtime",
		],
		{ encoding: "utf8", timeout: 10_000, maxBuffer: 16 * 1024 * 1024 },
	).split("\0").filter(Boolean);
	const missing = tracked.filter((path) =>
		includeRuntimePath(path.split("/").slice(1)) &&
		!existsSync(join(dirname(pinnedSource), path)),
	);
	if (missing.length > 0) {
		throw new Error(
			`stage-runtime: pinned Vim checkout is missing ${missing.length} selected tracked runtime files; restore these paths from the pinned source before staging: ${missing.slice(0, 20).join(", ")}`,
		);
	}
}

rmSync(join(packageRoot, "share"), { recursive: true, force: true });
mkdirSync(target, { recursive: true });
cpSync(source, target, {
	recursive: true,
	filter: (src) =>
		includeRuntimePath(src.slice(source.length).split("/").filter(Boolean)),
});
console.log(`stage-runtime: ${source} -> share/vim/vim92`);
