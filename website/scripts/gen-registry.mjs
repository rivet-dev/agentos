// Generates src/generated/registry.json from the monorepo's registry/ tree.
//
// A package is listed iff its agentos-package.json has a `registry` block with
// both `title` and `description` — no fallbacks. Everything else is derived:
// slug from the directory name (overridable via `registry.slug`), type from
// the parent dir (agent/software), npm package name from package.json, and
// for agents the agent id from the manifest `name` plus docs status when
// `registry.docsHref` is set. `featured` is deliberately not part of the
// block — the website hardcodes featured slugs in src/data/registry.ts.
//
// The output is committed. When registry/ is not present (e.g. the website
// Docker build, whose context is website/ only), the committed file is used
// as-is.
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const websiteDir = dirname(dirname(fileURLToPath(import.meta.url)));
const registryRoot = join(websiteDir, "..", "registry");
const outPath = join(websiteDir, "src", "generated", "registry.json");

if (!existsSync(registryRoot)) {
	if (existsSync(outPath)) {
		console.log("gen-registry: registry/ not found, using committed registry.json");
		process.exit(0);
	}
	console.error("gen-registry: registry/ not found and no committed registry.json");
	process.exit(1);
}

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));

// Keep in sync with website/src/data/registry-icons.ts. An icon name missing
// from REGISTRY_ICONS resolves to an undefined component and crashes the
// registry island on hydration, so fail the build here instead.
const KNOWN_ICONS = new Set([
	"HardDrive",
	"Database",
	"Monitor",
	"Globe",
	"Wrench",
	"Search",
	"FileSearch",
	"FolderTree",
	"Archive",
	"ArchiveRestore",
	"Braces",
	"FileQuestion",
	"Download",
	"FilePen",
	"Package",
	"Hammer",
]);

const entries = [];
// Meta-package composition: resolved after the scan because it needs the full
// npm-name -> slug map (a meta-package can depend on a package scanned later).
const nameToSlug = new Map();
const pendingIncludes = [];
for (const type of ["agent", "software"]) {
	const typeDir = join(registryRoot, type);
	for (const dir of readdirSync(typeDir, { withFileTypes: true })) {
		if (!dir.isDirectory()) continue;
		const pkgDir = join(typeDir, dir.name);
		const manifestPath = join(pkgDir, "agentos-package.json");
		if (!existsSync(manifestPath)) continue;
		const manifest = readJson(manifestPath);
		const meta = manifest.registry;
		if (!meta?.title || !meta?.description) continue;

		const pkg = readJson(join(pkgDir, "package.json"));
		const entry = {
			slug: meta.slug ?? dir.name,
			title: meta.title,
			description: meta.description,
			// A package's section defaults to its registry/ parent dir; `types`
			// overrides it (e.g. browserbase is a software package listed under
			// Browsers).
			types: meta.types ?? [type],
			priority: meta.priority ?? 0,
			package: pkg.name,
			status: meta.docsHref ? "docs" : "available",
		};
		if (meta.beta) entry.beta = true;
		if (meta.icon) entry.icon = meta.icon;
		if (meta.image) entry.image = meta.image;
		if (Array.isArray(manifest.commands) && manifest.commands.length > 0) {
			entry.commands = manifest.commands;
		}
		nameToSlug.set(pkg.name, entry.slug);
		if (type === "software") {
			const depNames = Object.keys(pkg.dependencies ?? {}).filter((name) =>
				name.startsWith("@agentos-software/"),
			);
			if (depNames.length > 0) pendingIncludes.push({ entry, depNames });
		}
		if (type === "agent") {
			// Every agent has a docs page; link it even for plain "available"
			// entries (which keep their npm install rendering). Agents whose
			// manifest carries no runtime `name` (e.g. codex, which only has a
			// registry block) fall back to the directory name as agent id.
			const agentId = manifest.name ?? dir.name;
			entry.docsHref = meta.docsHref ?? `/docs/agents/${agentId}`;
			entry.agentId = agentId;
		} else if (meta.docsHref) {
			entry.docsHref = meta.docsHref;
		}
		entries.push(entry);
	}
}

for (const { entry, depNames } of pendingIncludes) {
	const includes = [];
	for (const name of depNames) {
		const slug = nameToSlug.get(name);
		if (!slug) {
			// A dep without a registry block (unlisted) shouldn't sink the whole
			// meta-package listing.
			console.warn(`gen-registry: ${entry.slug} depends on unlisted ${name}`);
			continue;
		}
		includes.push(slug);
	}
	if (includes.length > 0) entry.includes = includes;
}

const seen = new Set();
for (const entry of entries) {
	if (seen.has(entry.slug)) {
		console.error(`gen-registry: duplicate slug "${entry.slug}"`);
		process.exit(1);
	}
	seen.add(entry.slug);
	if (entry.icon && !KNOWN_ICONS.has(entry.icon)) {
		console.error(
			`gen-registry: ${entry.slug} references unknown icon "${entry.icon}" (add it to website/src/data/registry-icons.ts and KNOWN_ICONS above)`,
		);
		process.exit(1);
	}
	if (entry.image) {
		const imagePath = join(websiteDir, "public", entry.image);
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
