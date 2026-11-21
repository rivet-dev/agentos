import { createHash } from "node:crypto";
import { appendFileSync, existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { $ } from "execa";
import semver from "semver";

/**
 * Publish context. Resolved once per workflow run by the `context-output` CI
 * subcommand and passed through every subsequent step via GitHub Actions job
 * outputs + per-step flags.
 */
export type Trigger = "branch" | "release";

export interface PublishContext {
	trigger: Trigger;
	/** Resolved version string, never null. */
	version: string;
	/** npm dist-tag. */
	npmTag: string;
	/** Full commit object ID. */
	sha: string;
	/** Only meaningful when trigger === "release". */
	latest: boolean;
	/** Branch name. Only set when trigger === "branch". */
	branch?: string;
	repoRoot: string;
}

/** Override set accepted by the local release cutter. */
export interface ResolveOverrides {
	trigger?: Trigger;
	version?: string;
	latest?: boolean;
	branch?: string;
	sha?: string;
}

function findRepoRoot(): string {
	if (process.env.GITHUB_WORKSPACE && existsSync(process.env.GITHUB_WORKSPACE)) {
		return process.env.GITHUB_WORKSPACE;
	}
	let dir = dirname(fileURLToPath(import.meta.url));
	for (let i = 0; i < 10; i++) {
		if (existsSync(join(dir, "pnpm-workspace.yaml"))) return dir;
		dir = dirname(dir);
	}
	throw new Error("Could not locate repo root (no pnpm-workspace.yaml)");
}

/**
 * Base for all preview pre-release strings. Hardcoded to `0.0.0` so preview
 * versions like `0.0.0-preview-my-branch-0123456789abcdef.g<commit>` never look like real releases and
 * always sort below any published `X.Y.Z`. Using a committed `package.json`
 * version as the base would just embed whatever stale number happened to be
 * committed there. It has no semantic relationship to the branch being
 * previewed.
 */
const PREVIEW_BASE_VERSION = "0.0.0";

async function readCommitSha(repoRoot: string): Promise<string> {
	const envSha = process.env.GITHUB_SHA;
	if (envSha) return envSha;
	const { stdout } = await $({ cwd: repoRoot })`git rev-parse HEAD`;
	return stdout.trim();
}

async function readBranchName(repoRoot: string): Promise<string> {
	const envRef = process.env.GITHUB_REF_NAME;
	if (envRef) return envRef;
	const { stdout } = await $({
		cwd: repoRoot,
	})`git rev-parse --abbrev-ref HEAD`;
	return stdout.trim();
}

/**
 * Keep preview tags outside the release namespace and distinguish branches
 * that normalize to the same readable slug.
 */
function previewBranchIdentifier(branch: string): string {
	const cleaned = branch
		.toLowerCase()
		.replace(/[^a-z0-9-]+/g, "-")
		.replace(/-+/g, "-")
		.replace(/^-+|-+$/g, "")
		.slice(0, 64);
	const digest = createHash("sha256").update(branch).digest("hex").slice(0, 16);
	return `preview-${cleaned || "branch"}-${digest}`;
}

function readInputFromEvent<T = unknown>(name: string): T | undefined {
	const path = process.env.GITHUB_EVENT_PATH;
	if (!path || !existsSync(path)) return undefined;
	try {
		const event = JSON.parse(readFileSync(path, "utf-8")) as {
			inputs?: Record<string, unknown>;
		};
		const v = event.inputs?.[name];
		return v as T | undefined;
	} catch {
		return undefined;
	}
}

function parseBoolInput(v: unknown, fallback: boolean): boolean {
	if (typeof v === "boolean") return v;
	if (typeof v === "string") {
		if (v === "true") return true;
		if (v === "false") return false;
	}
	return fallback;
}

function deriveTrigger(overrides: ResolveOverrides | undefined): Trigger {
	if (overrides?.trigger) return overrides.trigger;
	const eventName = process.env.GITHUB_EVENT_NAME;
	if (eventName === "workflow_dispatch") {
		const version = readInputFromEvent<string>("version");
		if (typeof version === "string" && version.length > 0) return "release";
		return "branch";
	}
	// Default for local invocation without overrides (unusual): assume release
	// so missing fields are caught loudly.
	return "release";
}

function computeNpmTag(
	trigger: Trigger,
	version: string,
	latest: boolean,
	branch?: string,
): string {
	if (trigger === "branch") {
		if (!branch) {
			throw new Error("branch trigger requires branch to compute npm tag");
		}
		return previewBranchIdentifier(branch);
	}
	// release
	if (semver.prerelease(version)?.[0] === "rc") return "rc";
	return latest ? "latest" : "next";
}

function computeVersion(
	trigger: Trigger,
	base: string,
	sha: string,
	branch: string | undefined,
	overrideVersion: string | undefined,
): string {
	if (overrideVersion) return overrideVersion;
	if (trigger === "branch") {
		if (!branch) {
			throw new Error("branch trigger requires branch to compute version");
		}
		return `${base}-${previewBranchIdentifier(branch)}.g${sha}`;
	}
	throw new Error("release trigger requires an explicit version override");
}

/** Validate a canonical version before publishing or writing version metadata. */
export function validatePublishVersion(version: string): string {
	if (semver.valid(version) === null || !/^[0-9][0-9A-Za-z.+-]*$/.test(version)) {
		throw new TypeError("Publish version must be a valid SemVer string");
	}
	return version;
}

export function validatePublishSha(sha: string): string {
	if (!/^(?:[0-9a-f]{40}|[0-9a-f]{64})$/i.test(sha)) {
		throw new TypeError("Publish artifact staging requires a full hexadecimal commit SHA");
	}
	return sha.toLowerCase();
}

export function canPromoteLatest(version: string): boolean {
	return semver.prerelease(version) === null;
}

/**
 * Resolve the publish context. Pure function of environment + overrides.
 * Not memoized: each subcommand process re-reads env, and the `context-output`
 * subcommand exists specifically so downstream steps receive stable values via
 * `$GITHUB_OUTPUT` / flags instead of re-resolving.
 */
export async function resolveContext(
	overrides: ResolveOverrides = {},
): Promise<PublishContext> {
	const repoRoot = findRepoRoot();
	const trigger = deriveTrigger(overrides);

	const sha = validatePublishSha(overrides.sha ?? (await readCommitSha(repoRoot)));

	let branch = overrides.branch;
	if (trigger === "branch" && !branch) {
		branch = await readBranchName(repoRoot);
	}

	// Release version: override > workflow_dispatch input > error.
	let version = overrides.version;
	if (!version && trigger === "release") {
		const input = readInputFromEvent<string>("version");
		if (typeof input === "string" && input.length > 0) version = input;
	}

	if (trigger !== "release") {
		version = computeVersion(
			trigger,
			PREVIEW_BASE_VERSION,
			sha,
			branch,
			version,
		);
	} else if (!version) {
		throw new Error(
			"release trigger requires version (pass --version or workflow_dispatch input)",
		);
	}

	validatePublishVersion(version);

	// Latest: override > workflow_dispatch input > false.
	let latest = overrides.latest;
	if (latest === undefined) {
		const input = readInputFromEvent<unknown>("latest");
		latest = parseBoolInput(input, false);
	}
	if (trigger !== "release" || !canPromoteLatest(version)) latest = false;

	const npmTag = computeNpmTag(trigger, version, latest, branch);

	return {
		trigger,
		version,
		npmTag,
		sha,
		latest,
		branch,
		repoRoot,
	};
}

/** Write every context field to `$GITHUB_OUTPUT` so downstream steps read via needs.*. */
export function writeContextToGithubOutput(ctx: PublishContext): void {
	const path = process.env.GITHUB_OUTPUT;
	if (!path) {
		// When invoked locally for debugging, print to stdout in the same format.
		console.log(`trigger=${ctx.trigger}`);
		console.log(`version=${ctx.version}`);
		console.log(`npm_tag=${ctx.npmTag}`);
		console.log(`sha=${ctx.sha}`);
		console.log(`latest=${ctx.latest}`);
		return;
	}
	const lines = [
		`trigger=${ctx.trigger}`,
		`version=${ctx.version}`,
		`npm_tag=${ctx.npmTag}`,
		`sha=${ctx.sha}`,
		`latest=${ctx.latest}`,
	];
	// Append (do not overwrite) in case other steps also wrote to GITHUB_OUTPUT.
	appendFileSync(path, `${lines.join("\n")}\n`);
}
