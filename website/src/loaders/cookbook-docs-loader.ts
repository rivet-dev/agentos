import { glob } from "astro/loaders";
import type { Loader, LoaderContext } from "astro/loaders";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/**
 * Cookbook docs loader — an Astro CONTENT-LAYER loader (runs inside Astro's
 * content sync, NOT a prebuild compile script). It loads the regular docs via
 * glob(), then injects one synthetic docs entry per example README at
 * `examples/<slug>/README.md`, with id `cookbooks/<slug>` so the [...slug].astro
 * route renders it at /cookbooks/<slug>. A "## Source" link to the example on
 * GitHub is appended.
 */

const scriptDir = dirname(fileURLToPath(import.meta.url));
// website/src/loaders -> repo root
const repoRoot = resolve(scriptDir, "../../..");
const examplesDir = resolve(repoRoot, "examples");
const GITHUB_TREE = "https://github.com/rivet-dev/agentos/tree/main/examples";

function parseFrontmatter(raw: string): { data: Record<string, string>; body: string } {
	const m = raw.match(/^---\n([\s\S]*?)\n---\n?([\s\S]*)$/);
	if (!m) return { data: {}, body: raw };
	const data: Record<string, string> = {};
	for (const line of m[1].split(/\r?\n/)) {
		const kv = line.match(/^(\w+):\s*(.*)$/);
		if (kv) data[kv[1]] = kv[2].trim().replace(/^["']|["']$/g, "");
	}
	return { data, body: m[2] };
}

// Drop a trailing "## Source" section if the README already has one; we re-add
// our own canonical link.
function stripSourceSection(body: string): string {
	return body.replace(/\n#{1,6}\s+Source\b[\s\S]*$/i, "\n");
}

async function loadCookbooks(ctx: LoaderContext) {
	if (!existsSync(examplesDir)) {
		ctx.logger.warn(`cookbook loader: examples dir not found at ${examplesDir}`);
		return;
	}
	const slugs = readdirSync(examplesDir).filter((d) =>
		existsSync(join(examplesDir, d, "README.md")),
	);
	let count = 0;
	for (const slug of slugs) {
		const readmePath = join(examplesDir, slug, "README.md");
		const raw = readFileSync(readmePath, "utf8");
		const { data, body } = parseFrontmatter(raw);
		const title = data.title || slug;
		const description = data.description;
		const github = `${GITHUB_TREE}/${slug}`;
		const transformed = `${stripSourceSection(body).trimEnd()}\n\n## Source\n\n[View source on GitHub](${github})\n`;

		const id = `cookbooks/${slug}`;
		const parsed = await ctx.parseData({
			id,
			data: { title, description },
			filePath: readmePath,
		});
		const rendered = await ctx.renderMarkdown(transformed);
		ctx.store.set({
			id,
			data: parsed,
			body: transformed,
			rendered,
			digest: ctx.generateDigest(transformed),
		});
		ctx.watcher?.add(readmePath);
		count++;
	}

	// Cookbooks overview landing.
	const overviewId = "cookbooks";
	const overviewBody =
		"agentOS cookbooks — runnable examples for every capability. Each page mirrors an example in the repo; follow the **View source on GitHub** link to run it.";
	const overviewParsed = await ctx.parseData({
		id: overviewId,
		data: { title: "Cookbooks", description: "Runnable agentOS examples." },
	});
	ctx.store.set({
		id: overviewId,
		data: overviewParsed,
		body: overviewBody,
		rendered: await ctx.renderMarkdown(overviewBody),
		digest: ctx.generateDigest(overviewBody),
	});

	ctx.logger.info(`cookbook loader: injected ${count} cookbook pages + overview`);
}

export function cookbookDocsLoader(): Loader {
	const base = glob({ pattern: "**/*.{md,mdx}", base: "./src/content/docs" });
	return {
		name: "cookbook-docs-loader",
		async load(ctx: LoaderContext) {
			await base.load(ctx);
			await loadCookbooks(ctx);
		},
	};
}
