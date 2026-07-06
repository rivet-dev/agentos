#!/usr/bin/env node
// Stage vim's runtime tree into the gitignored `share/vim/vim92/` so
// `agentos-toolchain build` ships it in dist/package and the manifest's
// `provides.files` can overlay it read-only at /usr/local/share/vim/vim92
// (VIMRUNTIME points straight at it, bypassing vim's version-dir search, so a
// 9.0/9.1 host runtime sources cleanly under the 9.2 binary).
//
// Sources, in order: $VIM_RUNTIME_SRC, then the host vim runtimes. Bulky,
// non-load-bearing subtrees (docs, tutor, spell dictionaries, translations)
// are trimmed — the runtime here exists so `vim` starts clean (defaults.vim,
// syntax, ftplugin, indent, autoload, colors), not to ship a manual.
// Missing source → skip with a notice (the package stays a valid placeholder,
// same contract as a missing command binary).
import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const target = join(packageRoot, "share", "vim", "vim92");

const TRIM = new Set(["doc", "tutor", "spell", "lang", "print", "keymap"]);

const candidates = [
	process.env.VIM_RUNTIME_SRC,
	"/usr/share/vim/vim92",
	"/usr/share/vim/vim91",
	"/usr/share/vim/vim90",
	"/usr/local/share/vim/vim92",
].filter(Boolean);

const source = candidates.find((dir) => existsSync(join(dir, "defaults.vim")));
if (!source) {
	console.log(
		"stage-runtime: no vim runtime found (set VIM_RUNTIME_SRC or install vim) — skipping; package ships without the runtime tree",
	);
	process.exit(0);
}

rmSync(join(packageRoot, "share"), { recursive: true, force: true });
mkdirSync(target, { recursive: true });
cpSync(source, target, {
	recursive: true,
	filter: (src) => {
		const rel = src.slice(source.length).split("/").filter(Boolean);
		return rel.length === 0 || !TRIM.has(rel[0]);
	},
});
console.log(`stage-runtime: ${source} -> share/vim/vim92 (trimmed: ${[...TRIM].join(", ")})`);
