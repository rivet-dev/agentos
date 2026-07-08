/**
 * Copies the Pi SDK V8 snapshot bundle into the assembled `dist/package/` closure
 * and refreshes `dist/package.tar`.
 *
 * The descriptor's `packageTar` points at `dist/package.tar` (the clean runtime closure the
 * sidecar projects into `/opt/agentos/pi/<version>`). The agent-os host reads the
 * snapshot bundle from `<dir>/dist/sdk-snapshot.js`
 * (`resolveAgentSnapshotBundle()` in `@rivet-dev/agentos-core`), so the bundle —
 * built by `build-snapshot-bundle.mjs` at `dist/sdk-snapshot.js` — must be mirrored
 * to `dist/package/dist/sdk-snapshot.js` (plus its `.sha256`). The toolchain `pack`
 * rebuilds `dist/package/` from scratch, so this runs AFTER pack and then refreshes
 * the tar.
 */
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { packAospkgFromTar } from "@rivet-dev/agentos-toolchain";

const here = dirname(fileURLToPath(import.meta.url));
const pkgRoot = join(here, "..");
const srcDir = join(pkgRoot, "dist");
const destDir = join(pkgRoot, "dist", "package", "dist");

const files = ["sdk-snapshot.js", "sdk-snapshot.js.sha256"];

const missing = files.filter((f) => !existsSync(join(srcDir, f)));
if (missing.length > 0) {
	throw new Error(
		`copy-snapshot-into-package: missing snapshot artifact(s) ${missing.join(", ")} in ${srcDir}; ` +
			`run build-snapshot-bundle.mjs first`,
	);
}
if (!existsSync(join(pkgRoot, "dist", "package", "agentos-package.json"))) {
	throw new Error(
		`copy-snapshot-into-package: dist/package not assembled; run the toolchain pack first`,
	);
}

mkdirSync(destDir, { recursive: true });
for (const f of files) {
	copyFileSync(join(srcDir, f), join(destDir, f));
}
execFileSync("tar", ["-cf", join(pkgRoot, "dist", "package.tar"), "-C", join(pkgRoot, "dist", "package"), "."], {
	stdio: "pipe",
});
// The .aospkg is the runtime artifact and was packed BEFORE the snapshot was
// mirrored in — re-pack it from the refreshed tar so snapshotBundlePath and
// the bundle bytes land in the packed manifest + mount tar.
packAospkgFromTar(
	join(pkgRoot, "dist", "package.tar"),
	join(pkgRoot, "dist", "package.aospkg"),
);
console.log(
	`copy-snapshot-into-package: mirrored ${files.join(", ")} -> dist/package/dist/, refreshed dist/package.tar, re-packed dist/package.aospkg`,
);
