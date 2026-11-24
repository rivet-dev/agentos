import { publishPackages } from "../lib/npm.ts";
import { readPackedNpm } from "../lib/packed-npm.ts";

const [directory] = process.argv.slice(2);
const { VERSION, NPM_TAG, TRIGGER, SIDECAR_PLATFORMS } = process.env;
if (!directory || !VERSION || !NPM_TAG || !SIDECAR_PLATFORMS ||
	!["branch", "release"].includes(TRIGGER ?? "")) {
	throw new Error("Packed npm publish requires a directory and resolved publish context");
}
if (TRIGGER === "release" ? !["latest", "next", "rc"].includes(NPM_TAG)
	: !/^preview-[a-z0-9-]+-[a-f0-9]{16}$/.test(NPM_TAG)) {
	throw new Error("Invalid npm tag for publish trigger");
}

const platforms = [...new Set(SIDECAR_PLATFORMS.trim().split(/\s+/))];
const packages = await readPackedNpm(directory, VERSION, platforms);
await publishPackages(packages, {
	tag: NPM_TAG,
	releaseMode: TRIGGER === "release",
	platformCount: platforms.length,
});
