// Generates docs/registry.json from this repo's software/ catalog.
//
// The output ships in the docs bundle; rivet-website copies it in at assemble
// time, so the catalog stays owned by the repo that defines it.
//
// A package is listed iff its agentos-package.json has a `registry` block with
// both `title` and `description` — no fallbacks. The public catalog references
// the logical artifact name used by the release manifest, never an npm package.
// `featured` is deliberately not part of the block — the website hardcodes
// featured slugs in src/data/registry.ts.
//
// The output is committed. When software/ is not present (e.g. the website
// Docker build, whose context is website/ only), the committed file is used
// as-is.
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const softwareRoot = join(repoRoot, "software");
const outPath = join(repoRoot, "docs", "registry.json");

if (!existsSync(softwareRoot)) {
	if (existsSync(outPath)) {
		console.log("gen-registry: software/ not found, using committed registry.json");
		process.exit(0);
	}
	console.error("gen-registry: software/ not found and no committed registry.json");
	process.exit(1);
}

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));

const entries = [];
for (const dir of readdirSync(softwareRoot, { withFileTypes: true })) {
	if (!dir.isDirectory()) continue;
	const pkgDir = join(softwareRoot, dir.name);
	const manifestPath = join(pkgDir, "agentos-package.json");
	if (!existsSync(manifestPath)) continue;
	const manifest = readJson(manifestPath);
	const meta = manifest.registry;
	if (!meta?.title || !meta?.description) continue;
	// Meta packages were npm dependency arrays. The URL registry contains only
	// concrete immutable artifacts; clients install each desired artifact URL.
	if (
		(!Array.isArray(manifest.commands) || manifest.commands.length === 0) &&
		(typeof manifest.name !== "string" || manifest.name.length === 0)
	) {
		continue;
	}

	const entry = {
		slug: meta.slug ?? dir.name,
		title: meta.title,
		description: meta.description,
		// `types` may place software in a specialized docs section (for example,
		// browserbase appears under Browsers).
		types: meta.types ?? ["software"],
		category: meta.category,
		priority: meta.priority ?? 0,
		artifactName: dir.name,
		status: meta.docsHref ? "docs" : "available",
	};
	if (meta.beta) entry.beta = true;
	if (meta.icon) entry.icon = meta.icon;
	if (meta.image) entry.image = meta.image;
	if (meta.docsHref) entry.docsHref = meta.docsHref;
	entries.push(entry);
}

const seen = new Set();
for (const entry of entries) {
	if (seen.has(entry.slug)) {
		console.error(`gen-registry: duplicate slug "${entry.slug}"`);
		process.exit(1);
	}
	seen.add(entry.slug);
	if (entry.image) {
		const imagePath = join(repoRoot, "docs", "public", entry.image);
		if (!existsSync(imagePath)) {
			console.error(`gen-registry: ${entry.slug} references missing image ${entry.image}`);
			process.exit(1);
		}
	}
}

entries.sort(
	(a, b) =>
		a.types[0].localeCompare(b.types[0]) ||
		b.priority - a.priority ||
		a.title.localeCompare(b.title),
);

writeFileSync(outPath, JSON.stringify({ entries }, null, "\t") + "\n");
console.log(`gen-registry: wrote ${entries.length} entries`);
