/**
 * Parallel npm publish with bounded concurrency, exponential backoff retries,
 * and idempotent "already published" handling.
 *
 * Works for both preview and release flows — the only per-flow input is the
 * dist-tag and the `releaseMode` flag (which toggles strict preflight).
 *
 * All packages publish with `npm publish` (never `pnpm publish`). `npm publish`
 * preserves the `0755` executable bit on the bundled sidecar binary, which
 * `pnpm publish` normalizes to `0644`. `workspace:*` dependency specs are
 * rewritten to literal versions by `bumpPackageJsons` (full mode) before this
 * runs, so plain `npm publish` resolves them correctly.
 */
import { spawn } from "node:child_process";
import { scoped } from "./logger.ts";
import type { Package } from "./packages.ts";

export type NpmPackage = Package & { tarball?: string };

const log = scoped("npm");

export interface PublishAllOptions {
	/** npm dist-tag (e.g. a preview tag, rc, next, latest). */
	tag: string;
	/** Max simultaneous publishes. */
	parallel?: number;
	/** Publish this many platform packages before starting dependent packages. */
	platformCount?: number;
	/** Max retries per package. */
	retries?: number;
	/** Initial backoff in ms (doubled per retry). */
	initialBackoffMs?: number;
	/**
	 * When true, fail hard if every package is already published. Preview
	 * mode treats this as an idempotent no-op; release mode treats it as a
	 * "you forgot to bump the version" error.
	 */
	releaseMode?: boolean;
	/** Pass `--dry-run` to npm publish (local verification, publishes nothing). */
	dryRun?: boolean;
}

export type PublishStatus =
	| "success"
	| "retried-success"
	| "already-exists"
	| "failed";

export interface PublishResult {
	pkg: NpmPackage;
	status: PublishStatus;
	attempts: number;
	lastError?: string;
}

export interface PublishSummary {
	results: PublishResult[];
	counts: {
		success: number;
		retried: number;
		alreadyExists: number;
		failed: number;
	};
	elapsedSeconds: number;
}

const ALREADY_PUBLISHED_PATTERNS = [
	"cannot publish over the previously published versions",
	"cannot publish over previously published version",
	"You cannot publish over",
];

function isAlreadyPublished(output: string): boolean {
	return ALREADY_PUBLISHED_PATTERNS.some((p) => output.includes(p));
}

function isRetryable(output: string): boolean {
	if (isAlreadyPublished(output)) return false;
	return (
		output.includes("ECONNRESET") ||
		output.includes("ETIMEDOUT") ||
		output.includes("ENOTFOUND") ||
		output.includes("EAI_AGAIN") ||
		output.includes("socket hang up") ||
		output.includes("npm error 503") ||
		output.includes("npm error 502") ||
		output.includes("npm error 504") ||
		output.includes("npm error 429") ||
		output.includes("ERR_STREAM_PREMATURE_CLOSE") ||
		// Some npm errors don't tag the status clearly; if we don't see a
		// definitive "already published" we can retry once.
		!/npm error (code|E[A-Z]+)/.test(output)
	);
}

function extractError(output: string, maxLines = 3): string {
	const lines = output
		.split("\n")
		.filter((l) => /npm error/i.test(l) && !l.includes("A complete log"))
		.slice(0, maxLines);
	if (lines.length === 0) {
		return output.trim().split("\n").slice(-maxLines).join(" | ");
	}
	return lines.join(" | ");
}

function runNpmPublish(
	pkg: NpmPackage,
	tag: string,
	dryRun: boolean,
): Promise<{ code: number; output: string }> {
	return new Promise((resolvePromise) => {
		const args = ["publish", "--access", "public", "--tag", tag];
		if (dryRun) args.push("--dry-run");
		if (pkg.tarball) {
			args.push("--ignore-scripts", "--registry", "https://registry.npmjs.org", "--", pkg.tarball);
		}
		const child = spawn("npm", args, {
			cwd: pkg.dir,
			stdio: ["ignore", "pipe", "pipe"],
			env: process.env,
		});
		const chunks: Buffer[] = [];
		child.stdout.on("data", (c) => chunks.push(c));
		child.stderr.on("data", (c) => chunks.push(c));
		child.once("error", (error) => {
			resolvePromise({ code: 1, output: error.message });
		});
		child.on("close", (code) => {
			resolvePromise({
				code: code ?? 1,
				output: Buffer.concat(chunks).toString("utf8"),
			});
		});
	});
}

async function publishOne(
	pkg: NpmPackage,
	opts: Required<
		Pick<PublishAllOptions, "tag" | "retries" | "initialBackoffMs" | "dryRun">
	>,
): Promise<PublishResult> {
	for (let attempt = 1; attempt <= opts.retries + 1; attempt++) {
		const { code, output } = await runNpmPublish(pkg, opts.tag, opts.dryRun);
		if (code === 0) {
			return {
				pkg,
				status: attempt === 1 ? "success" : "retried-success",
				attempts: attempt,
			};
		}
		if (isAlreadyPublished(output)) {
			return { pkg, status: "already-exists", attempts: attempt };
		}
		if (!isRetryable(output) || attempt > opts.retries) {
			return {
				pkg,
				status: "failed",
				attempts: attempt,
				lastError: extractError(output),
			};
		}
		const delay = opts.initialBackoffMs * 2 ** (attempt - 1);
		log.info(`  [retry ${attempt}/${opts.retries}] ${pkg.name} — waiting ${delay}ms`);
		await new Promise((r) => setTimeout(r, delay));
	}
	return { pkg, status: "failed", attempts: opts.retries + 1 };
}

function printResult(r: PublishResult): void {
	const name = r.pkg.name.padEnd(48);
	const symbol =
		r.status === "success" || r.status === "retried-success"
			? "✓"
			: r.status === "already-exists"
				? "="
				: "✗";
	const suffix =
		r.status === "retried-success"
			? ` (after ${r.attempts} attempts)`
			: r.status === "failed"
				? ` — ${r.lastError ?? "unknown error"}`
				: "";
	log.info(`  ${symbol} ${name}${suffix}`);
}

export async function publishPackages(
	packages: readonly NpmPackage[],
	opts: PublishAllOptions,
): Promise<PublishSummary> {
	const parallel = opts.parallel ?? 16;
	const retries = opts.retries ?? 3;
	const initialBackoffMs = opts.initialBackoffMs ?? 2000;
	const tag = opts.tag;
	const dryRun = opts.dryRun ?? false;

	log.info(
		`publishing ${packages.length} packages | tag=${tag} | parallel=${parallel} | retries=${retries}${dryRun ? " | DRY RUN" : ""}`,
	);

	const queue: NpmPackage[] = [];
	const results: PublishResult[] = [];
	const startedAt = Date.now();

	async function worker(): Promise<void> {
		while (true) {
			const pkg = queue.shift();
			if (!pkg) return;
			const result = await publishOne(pkg, {
				tag,
				retries,
				initialBackoffMs,
				dryRun,
			});
			printResult(result);
			results.push(result);
		}
	}

	async function publishBatch(batch: readonly NpmPackage[]): Promise<void> {
		queue.push(...batch);
		await Promise.all(Array.from({ length: Math.min(parallel, batch.length) }, worker));
	}

	const platformCount = opts.platformCount ?? 0;
	await publishBatch(packages.slice(0, platformCount));
	if (!results.some((result) => result.status === "failed")) {
		await publishBatch(packages.slice(platformCount));
	}

	const elapsed = (Date.now() - startedAt) / 1000;
	const counts = {
		success: results.filter((r) => r.status === "success").length,
		retried: results.filter((r) => r.status === "retried-success").length,
		alreadyExists: results.filter((r) => r.status === "already-exists").length,
		failed: results.filter((r) => r.status === "failed").length,
	};

	log.info("");
	log.info(`summary (${elapsed.toFixed(1)}s)`);
	log.info(`  ${counts.success} succeeded`);
	if (counts.retried > 0) log.info(`  ${counts.retried} succeeded after retry`);
	if (counts.alreadyExists > 0)
		log.info(`  ${counts.alreadyExists} already published (no-op)`);
	if (counts.failed > 0) {
		log.error(`  ${counts.failed} FAILED`);
		for (const r of results.filter((x) => x.status === "failed")) {
			log.error(`    - ${r.pkg.name}: ${r.lastError}`);
		}
		throw new Error(`${counts.failed} package(s) failed to publish`);
	}

	// In release mode, if *every* package was already published, treat it as
	// an error — almost certainly a missed version bump. Reruns of successful
	// releases are OK because partial rerun (a few packages re-publish) still
	// has at least one success.
	if (
		opts.releaseMode &&
		!dryRun &&
		counts.success === 0 &&
		counts.retried === 0 &&
		counts.failed === 0 &&
		counts.alreadyExists === packages.length
	) {
		throw new Error(
			`release mode: all ${packages.length} packages already published at this version. Did you forget to bump the version?`,
		);
	}

	return { results, counts, elapsedSeconds: elapsed };
}
