/**
 * R2 client + upload + copy helpers for the releases bucket.
 *
 * Credentials come from env vars (`R2_RELEASES_ACCESS_KEY_ID` +
 * `R2_RELEASES_SECRET_ACCESS_KEY`). R2 hosting of the sidecar binary is a
 * convenience mirror for crates.io consumers — the npm platform packages bundle
 * the binary directly, so R2 upload is release-only and best-effort.
 *
 * Implementation note: we shell out to `aws s3 cp` / `aws s3api copy-object`
 * because Cloudflare R2 does not support the `x-amz-tagging-directive` header
 * the AWS SDK sends even with `--copy-props none`. The `s3api copy-object` path
 * avoids it. See:
 *   https://community.cloudflare.com/t/r2-s3-compat-doesnt-support-net-sdk-for-copy-operations-due-to-tagging-header/616867
 */
import { opendir } from "node:fs/promises";
import { join } from "node:path";
import { $ } from "execa";
import { scoped } from "./logger.js";

const log = scoped("r2");

const BUCKET = "rivet-releases";
const ENDPOINT_URL =
	"https://2a94c6a0ced8d35ea63cddc86c2681e7.r2.cloudflarestorage.com";

type R2Env = Record<string, string>;

let cached: R2Env | null = null;

const MAX_ARTIFACT_ENTRIES = 4096;
const MAX_ARTIFACT_DEPTH = 32;

function getR2Env(): R2Env {
	if (cached) return cached;
	const ak = process.env.R2_RELEASES_ACCESS_KEY_ID;
	const sk = process.env.R2_RELEASES_SECRET_ACCESS_KEY;
	if (!ak || !sk) {
		throw new Error(
			"R2_RELEASES_ACCESS_KEY_ID and R2_RELEASES_SECRET_ACCESS_KEY must be set",
		);
	}
	cached = {
		AWS_ACCESS_KEY_ID: ak,
		AWS_SECRET_ACCESS_KEY: sk,
		AWS_DEFAULT_REGION: "auto",
	};
	return cached;
}

export interface ListEntry {
	Key: string;
	Size?: number;
}
export interface ListResult {
	Contents: ListEntry[];
}

export async function listObjects(prefix: string): Promise<ListResult> {
	const env = getR2Env();
	const contents: ListEntry[] = [];
	let continuationToken: string | undefined;

	for (let pageIndex = 0; pageIndex < MAX_ARTIFACT_ENTRIES; pageIndex++) {
		const { stdout } = continuationToken
			? await $({
					env,
				})`aws s3api list-objects-v2 --no-paginate --bucket ${BUCKET} --prefix ${prefix} --continuation-token ${continuationToken} --endpoint-url ${ENDPOINT_URL}`
			: await $({
					env,
				})`aws s3api list-objects-v2 --no-paginate --bucket ${BUCKET} --prefix ${prefix} --endpoint-url ${ENDPOINT_URL}`;
		if (!stdout.trim()) throw new Error(`R2 returned an empty listing for ${prefix}`);

		const page = JSON.parse(stdout) as {
			Contents?: ListEntry[];
			IsTruncated?: boolean;
			NextContinuationToken?: string;
		};
		if (Array.isArray(page.Contents)) {
			for (const entry of page.Contents) {
				if (typeof entry.Key !== "string" || !entry.Key.startsWith(prefix)) {
					throw new Error(`R2 listed an object outside prefix ${prefix}`);
				}
				if (contents.length === MAX_ARTIFACT_ENTRIES) {
					throw new Error(`R2 listing for ${prefix} exceeds ${MAX_ARTIFACT_ENTRIES} objects`);
				}
				contents.push(entry);
			}
		}
		if (!page.IsTruncated) return { Contents: contents };
		if (!page.NextContinuationToken || page.NextContinuationToken === continuationToken) {
			throw new Error(`R2 listing for ${prefix} has an invalid continuation token`);
		}
		continuationToken = page.NextContinuationToken;
	}
	throw new Error(`R2 listing for ${prefix} exceeds ${MAX_ARTIFACT_ENTRIES} pages`);
}

/** Upload a single file to R2. */
export async function uploadFile(
	localPath: string,
	r2Key: string,
): Promise<void> {
	const env = getR2Env();
	log.info(`uploading ${localPath} -> ${r2Key}`);
	await $({
		env,
		stdio: "inherit",
	})`aws s3 cp ${localPath} s3://${BUCKET}/${r2Key} --endpoint-url ${ENDPOINT_URL} --checksum-algorithm CRC32`;
}

// Software manifests are commit markers for their content-addressed packages.
function publishOrder(relativeKey: string): number {
	if (relativeKey === "default-software.json") return 2;
	if (relativeKey === "manifest.json") return 1;
	return 0;
}

async function localArtifactFiles(directory: string): Promise<string[]> {
	const pending = [""];
	const files: string[] = [];
	let entries = 0;
	while (pending.length > 0) {
		const parent = pending.pop()!;
		for await (const entry of await opendir(join(directory, parent))) {
			const relative = parent ? `${parent}/${entry.name}` : entry.name;
			if (++entries > MAX_ARTIFACT_ENTRIES || relative.split("/").length > MAX_ARTIFACT_DEPTH) {
				throw new Error(`Artifact tree in ${directory} exceeds its entry or depth limit`);
			}
			if (entry.isDirectory()) pending.push(relative);
			else if (entry.isFile()) files.push(relative);
			else throw new Error(`Artifact entry must be a regular file or directory: ${relative}`);
		}
	}
	if (files.length === 0) throw new Error(`Artifact directory ${directory} has no files`);
	return files.sort((a, b) => publishOrder(a) - publishOrder(b) || a.localeCompare(b));
}

async function pruneStaleObjects(
	destination: ListResult,
	published: ReadonlySet<string>,
): Promise<void> {
	const env = getR2Env();
	for (const { Key } of destination.Contents) {
		if (published.has(Key)) continue;
		await $({
			env,
		})`aws s3api delete-object --bucket ${BUCKET} --key ${Key} --endpoint-url ${ENDPOINT_URL}`;
	}
}

/** Stage the exact artifact set, preserving previous objects on upload failure. */
export async function uploadDir(
	localDir: string,
	r2Prefix: string,
): Promise<void> {
	if (!r2Prefix.endsWith("/")) throw new Error("R2 upload requires a directory prefix");
	const files = await localArtifactFiles(localDir);
	const destination = await listObjects(r2Prefix);
	const uploaded = new Set<string>();
	try {
		for (const file of files) {
			const key = r2Prefix + file;
			await uploadFile(join(localDir, file), key);
			uploaded.add(key);
		}
	} catch (cause) {
		throw new Error(
			`R2 upload to ${r2Prefix} failed after uploading ${uploaded.size} of ${files.length} objects; stale objects retained`,
			{ cause },
		);
	}
	await pruneStaleObjects(destination, uploaded);
}

/**
 * Copy every object under `sourcePrefix` to `targetPrefix`. Uses `s3api
 * copy-object` per-object to avoid the R2 tagging-directive bug.
 */
export async function copyPrefix(
	sourcePrefix: string,
	targetPrefix: string,
): Promise<void> {
	if (!sourcePrefix.endsWith("/") || !targetPrefix.endsWith("/") ||
		sourcePrefix.startsWith(targetPrefix) || targetPrefix.startsWith(sourcePrefix)) {
		throw new Error("R2 copy requires distinct, non-overlapping directory prefixes");
	}
	const env = getR2Env();
	log.info(`copying ${sourcePrefix} -> ${targetPrefix}`);

	const source = await listObjects(sourcePrefix);
	if (source.Contents.length === 0) {
		throw new Error(`R2 source prefix ${sourcePrefix} has no objects`);
	}
	source.Contents.sort((a, b) =>
		publishOrder(a.Key.slice(sourcePrefix.length)) - publishOrder(b.Key.slice(sourcePrefix.length)),
	);
	const destination = await listObjects(targetPrefix);
	const published = new Set<string>();
	try {
		for (const { Key: sourceKey } of source.Contents) {
			const targetKey = targetPrefix + sourceKey.slice(sourcePrefix.length);
			const encodedSource = sourceKey.split("/").map(encodeURIComponent).join("/");
			log.info(`  ${sourceKey} -> ${targetKey}`);
			await $({
				env,
			})`aws s3api copy-object --bucket ${BUCKET} --key ${targetKey} --copy-source ${BUCKET}/${encodedSource} --endpoint-url ${ENDPOINT_URL}`;
			published.add(targetKey);
		}
	} catch (cause) {
		throw new Error(
			`R2 copy to ${targetPrefix} failed after copying ${published.size} of ${source.Contents.length} objects; stale objects retained`,
			{ cause },
		);
	}

	// Remove only previously listed keys after every new object has arrived.
	await pruneStaleObjects(destination, published);
}
