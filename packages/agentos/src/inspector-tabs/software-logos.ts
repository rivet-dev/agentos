// Package logos, vendored from the registry site
// (website/public/images/registry) so the inspector works offline. Keyed by
// the package basename (SoftwareBundle.slug); packages without a logo fall
// back to a letter avatar in the Software list.
import browserbase from "./assets/software-logos/browserbase.svg";
import coreutils from "./assets/software-logos/coreutils.svg";
import curl from "./assets/software-logos/curl.svg";
import duckdb from "./assets/software-logos/duckdb.svg";
import git from "./assets/software-logos/git.svg";
import jq from "./assets/software-logos/jq.svg";
import nodejs from "./assets/software-logos/nodejs.svg";
import python from "./assets/software-logos/python.svg";
import sqlite3 from "./assets/software-logos/sqlite3.svg";

/** Dark-mode legibility per logo, hue-true: brightness lifts dark brand colors
 * without shifting them (GNU red stays red, curl navy stays navy); invert is
 * reserved for near-black monochrome marks, which have no hue to break.
 * Colorful logos (git, python, node, …) need nothing. */
export const SOFTWARE_LOGO_DARK_CLASS: Record<string, string> = {
	// GNU family, #A42E2B dark red.
	coreutils: "dark:brightness-[1.8]",
	// Deep navy / purple marks.
	curl: "dark:brightness-[2.6]",
	sqlite3: "dark:brightness-[2.6]",
	// Near-black monochrome marks.
	duckdb: "dark:invert",
	jq: "dark:invert",
};

export const SOFTWARE_LOGOS: Record<string, string> = {
	browserbase,
	coreutils,
	curl,
	duckdb,
	git,
	jq,
	node: nodejs,
	nodejs,
	python,
	sqlite3,
};
