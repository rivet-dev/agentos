/**
 * Checks each docs bundle (docs/ for agentOS, secure-exec/docs/ for Secure Exec):
 *   - every page has title + description frontmatter
 *   - /{product}/{docs,tutorials,...} links resolve to an MDX file
 *   - #anchors match ## / ### headings (same-page and cross-page)
 *   - sidebar hrefs resolve (https:// and website-owned routes are skipped)
 *   - <CodeSnippet file="..."> paths exist at the repo root, and region=
 *     markers have a matching docs:start / docs:end pair
 * Does not compile MDX; theme components live in rivet-dev/website.
 */

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const BUNDLES = [
	{
		product: "agentos",
		docsDir: "docs",
		collections: new Set(["docs", "tutorials", "integrations", "use-cases"]),
	},
	{
		product: "secure-exec",
		docsDir: "secure-exec/docs",
		collections: new Set(["docs"]),
	},
];

const PRODUCT_ALT = BUNDLES.map((bundle) => bundle.product).join("|");
const MD_CROSS_LINK = new RegExp(
	`\\]\\((/(?:${PRODUCT_ALT})/[^)\\s#]+)(#[^)\\s]+)?\\)`,
	"g",
);
const HREF_CROSS_LINK = new RegExp(
	`href="(/(?:${PRODUCT_ALT})/[^"#]+)(#[^"]+)?"`,
	"g",
);

// `--root` lets tests point the checker at a temp tree instead of this repo.
const argv = process.argv.slice(2);
let root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
for (let i = 0; i < argv.length; i++) {
	if (argv[i] === "--root") {
		root = resolve(argv[++i]);
	}
}

const failures = [];
const fail = (message) => failures.push(message);
const rel = (path) => relative(root, path).split(sep).join("/");
/** Convert a string index into a 1-based line number. */
const lineNumberAt = (text, index) => text.slice(0, index).split("\n").length;

/** Heading text to slug: "Foo & Bar" → "foo--bar". */
const slugifyHeading = (text) =>
	text
		.trim()
		.toLowerCase()
		// replace all contiguous whitespace with hyphens, e.g. "foo   bar" → "foo-bar"
		.replace(/\s+/g, "-")
		// remove all non-word and non-hyphen characters, e.g. "foo & bar!" → "foo--bar"
		.replace(/[^\w-]/g, "")
		// trim leading/trailing hyphens, e.g. "--foo-bar--" → "foo-bar"
		.replace(/^-+|-+$/g, "");

/**
 * Recursively walks through a directory and calls the visit callback
 * on every .mdx file found.
 */
function walkMdx(dir, visit) {
	if (!existsSync(dir)) return;
	for (const entry of readdirSync(dir, { withFileTypes: true })) {
		const path = join(dir, entry.name);
		if (entry.isDirectory()) {
			walkMdx(path, visit);
		} else if (entry.name.endsWith(".mdx")) {
			visit(path);
		}
	}
}

/**
 * Map a site path like /agentos/docs/quickstart to an MDX file on disk.
 * Returns { path }, { missing }, or { skip } when the href is not part
 * of a bundle we own (e.g. /agentos/registry).
 */
function hrefToMdxPath(hrefPath) {
	const parts = hrefPath.replace(/\/+$/, "").split("/").filter(Boolean);
	if (parts.length < 2) {
		return { skip: true };
	}
	const bundle = BUNDLES.find((entry) => entry.product === parts[0]);
	if (!bundle) {
		return { skip: true };
	}
	const collection = parts[1];
	if (!bundle.collections.has(collection)) {
		return { skip: true };
	}
	const slugParts = parts.slice(2);
	const baseDir = join(root, bundle.docsDir, "content", collection);
	let candidates;
	if (slugParts.length === 0) {
		candidates = [join(baseDir, "index.mdx")];
	} else {
		const slugPath = slugParts.join("/");
		candidates = [
			join(baseDir, `${slugPath}.mdx`),
			join(baseDir, slugPath, "index.mdx"),
		];
	}
	for (const candidate of candidates) {
		if (existsSync(candidate)) {
			return { path: candidate };
		}
	}
	return { missing: hrefPath };
}

/** Fail if #fragment is not a ## / ### heading slug in the target page. */
function checkFragment(sourcePath, line, targetText, fragment, suffix) {
	const headings = new Set();
	for (const headingLine of targetText.split("\n")) {
		// For each line in the page, check if it starts with '##' or '###'.
		const match = /^(#{2,3})\s+(.+)$/.exec(headingLine);
		// If it matches, generate a slug from the heading text and add it to the set of headings for lookup.
		if (match) {
			headings.add(slugifyHeading(match[2]));
		}
	}
	const fragSlug = slugifyHeading(decodeURIComponent(fragment));
	if (!headings.has(fragSlug)) {
		fail(`${rel(sourcePath)}:${line}: broken anchor #${fragment}${suffix}`);
	}
}

/**
 * Check one in-repo product URL found in sourcePath.
 * skip = a route this repo does not own (e.g. /agentos/registry); ignore it.
 * missing = this bundle should have that page, but the MDX file is gone.
 * If the URL has #fragment, also require that heading on the target page.
 */
function checkCrossPage(sourcePath, sourceText, line, pathPart, fragment) {
	const resolved = hrefToMdxPath(pathPart);
	if (resolved.skip) {
		return;
	}
	if (resolved.missing) {
		fail(
			`${rel(sourcePath)}:${line}: broken link ${pathPart}${fragment ? `#${fragment}` : ""}`,
		);
		return;
	}
	if (fragment) {
		const targetText =
			resolved.path === sourcePath
				? sourceText
				: readFileSync(resolved.path, "utf8");
		checkFragment(sourcePath, line, targetText, fragment, ` on ${pathPart}`);
	}
}

/** Recursively check every href in a sidebar.json. */
function walkSidebar(node, sidebarRel) {
	if (Array.isArray(node)) {
		for (const item of node) {
			walkSidebar(item, sidebarRel);
		}
		return;
	}
	if (typeof node !== "object" || node === null) {
		return;
	}
	if (typeof node.href === "string") {
		const href = node.href;
		if (/^https?:\/\//i.test(href)) {
			return;
		}
		const resolved = hrefToMdxPath(href.replace(/\/+$/, ""));
		if (resolved.skip) {
			return;
		}
		if (resolved.missing) {
			fail(`${sidebarRel}: broken sidebar href ${href}`);
		}
	}
	for (const value of Object.values(node)) {
		walkSidebar(value, sidebarRel);
	}
}

let pageCount = 0;

for (const bundle of BUNDLES) {
	const docsDir = join(root, bundle.docsDir);
	if (!existsSync(docsDir)) {
		fail(`${bundle.docsDir}/ is missing`);
		continue;
	}
	const contentRoot = join(docsDir, "content");
	const sidebarPath = join(docsDir, "sidebar.json");
	if (!existsSync(contentRoot)) {
		fail(`${bundle.docsDir}/content/ is missing`);
		continue;
	}

	// Scan every docs page in this bundle.
	walkMdx(contentRoot, (mdxPath) => {
		pageCount += 1;
		const text = readFileSync(mdxPath, "utf8");

		// Each page must have a frontmatter with a title and description.
		const fm = /^---\n([\s\S]*?)\n---/.exec(text);
		if (!fm) {
			fail(`${rel(mdxPath)}: missing frontmatter`);
		} else {
			if (!/^title:\s/m.test(fm[1])) {
				fail(`${rel(mdxPath)}: frontmatter missing title`);
			}
			if (!/^description:\s/m.test(fm[1])) {
				fail(`${rel(mdxPath)}: frontmatter missing description`);
			}
		}

		// Links to other product docs pages must point at a real MDX file, including any #anchor.
		for (const match of text.matchAll(MD_CROSS_LINK)) {
			checkCrossPage(
				mdxPath,
				text,
				lineNumberAt(text, match.index),
				match[1],
				match[2]?.slice(1) ?? "",
			);
		}
		// Same check for href="/agentos/..." and href="/secure-exec/..." on MDX components.
		for (const match of text.matchAll(HREF_CROSS_LINK)) {
			checkCrossPage(
				mdxPath,
				text,
				lineNumberAt(text, match.index),
				match[1],
				match[2]?.slice(1) ?? "",
			);
		}
		// Same-page #anchors must match a heading on this file.
		for (const match of text.matchAll(/\]\((#[^)\s]+)\)/g)) {
			checkFragment(
				mdxPath,
				lineNumberAt(text, match.index),
				text,
				match[1].slice(1),
				"",
			);
		}

		// Embedded example files in <CodeSnippet> must exist in the repo.
		// If the snippet names a region, the file must have matching docs:start / docs:end markers in order.
		for (const match of text.matchAll(/<CodeSnippet\s+([^>]*)>/g)) {
			const attrs = match[1];
			const file = /\bfile="([^"]+)"/.exec(attrs)?.[1];
			if (!file) {
				continue;
			}
			const line = lineNumberAt(text, match.index);
			const full = join(root, file);
			if (!existsSync(full)) {
				fail(`${rel(mdxPath)}:${line}: CodeSnippet file missing: ${file}`);
				continue;
			}
			const region = /\bregion="([^"]+)"/.exec(attrs)?.[1];
			if (!region) {
				continue;
			}
			let startLine = -1;
			let endLine = -1;
			for (const [index, sourceLine] of readFileSync(full, "utf8").split("\n").entries()) {
				const startName = /docs:start\s+(\S+)/.exec(sourceLine)?.[1];
				if (startLine < 0 && startName === region) {
					startLine = index;
				}
				const endName = /docs:end\s+(\S+)/.exec(sourceLine)?.[1];
				if (endLine < 0 && endName === region) {
					endLine = index;
				}
			}
			if (startLine < 0 || endLine < 0) {
				fail(
					`${rel(mdxPath)}:${line}: CodeSnippet region "${region}" missing in ${file}`,
				);
			} else if (startLine >= endLine) {
				fail(
					`${rel(mdxPath)}:${line}: CodeSnippet region "${region}" end precedes start in ${file}`,
				);
			}
		}
	});

	if (!existsSync(sidebarPath)) {
		fail(`${bundle.docsDir}/sidebar.json is missing`);
	} else {
		walkSidebar(JSON.parse(readFileSync(sidebarPath, "utf8")), `${bundle.docsDir}/sidebar.json`);
	}
}

if (failures.length > 0) {
	for (const failure of failures) {
		console.error(`check-docs: ${failure}`);
	}
	process.exit(1);
}

console.log(`check-docs: OK (${pageCount} pages)`);
