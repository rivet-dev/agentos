// Package logos, vendored from the registry site
// (website/public/images/registry) so the inspector works offline. Keyed by
// the package basename (SoftwareBundle.slug); packages without a logo fall
// back to a letter avatar in the Software list.
import browserbase from "./assets/software-logos/browserbase.svg";
import claudeCode from "./assets/software-logos/claude-code.svg";
import codex from "./assets/software-logos/codex.svg";
import coreutils from "./assets/software-logos/coreutils.svg";
import curl from "./assets/software-logos/curl.svg";
import diffutils from "./assets/software-logos/diffutils.svg";
import duckdb from "./assets/software-logos/duckdb.svg";
import findutils from "./assets/software-logos/findutils.svg";
import gawk from "./assets/software-logos/gawk.svg";
import git from "./assets/software-logos/git.svg";
import grep from "./assets/software-logos/grep.svg";
import gzip from "./assets/software-logos/gzip.svg";
import jq from "./assets/software-logos/jq.svg";
import nodejs from "./assets/software-logos/nodejs.svg";
import opencode from "./assets/software-logos/opencode.svg";
import pi from "./assets/software-logos/pi.svg";
import python from "./assets/software-logos/python.svg";
import sed from "./assets/software-logos/sed.svg";
import sqlite3 from "./assets/software-logos/sqlite3.svg";
import superMemory from "./assets/software-logos/super-memory.svg";
import tar from "./assets/software-logos/tar.svg";
import vim from "./assets/software-logos/vim.svg";
import wget from "./assets/software-logos/wget.svg";

/** Dark-mode legibility per logo, hue-true: brightness lifts dark brand colors
 * without shifting them (GNU red stays red, curl navy stays navy); invert is
 * reserved for near-black monochrome marks, which have no hue to break.
 * Colorful logos (git, python, node, …) need nothing. */
export const SOFTWARE_LOGO_DARK_CLASS: Record<string, string> = {
	// GNU family, #A42E2B dark red.
	coreutils: "dark:brightness-[1.8]",
	diffutils: "dark:brightness-[1.8]",
	findutils: "dark:brightness-[1.8]",
	gawk: "dark:brightness-[1.8]",
	grep: "dark:brightness-[1.8]",
	gzip: "dark:brightness-[1.8]",
	sed: "dark:brightness-[1.8]",
	tar: "dark:brightness-[1.8]",
	wget: "dark:brightness-[1.8]",
	// Deep navy / purple marks.
	codex: "dark:brightness-[2.1]",
	curl: "dark:brightness-[2.6]",
	sqlite3: "dark:brightness-[2.6]",
	// Near-black monochrome marks.
	duckdb: "dark:invert",
	jq: "dark:invert",
	opencode: "dark:invert",
	pi: "dark:invert",
};

export const SOFTWARE_LOGOS: Record<string, string> = {
	browserbase,
	"claude-code": claudeCode,
	codex,
	coreutils,
	curl,
	diffutils,
	duckdb,
	findutils,
	gawk,
	git,
	grep,
	gzip,
	jq,
	node: nodejs,
	nodejs,
	opencode,
	pi,
	python,
	sed,
	sqlite3,
	"super-memory": superMemory,
	tar,
	vim,
	wget,
};
