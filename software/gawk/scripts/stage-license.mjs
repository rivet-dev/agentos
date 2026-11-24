import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const packageRoot = new URL("../", import.meta.url);
// Empty registry placeholders may be built without a command corpus.
if (existsSync(new URL("bin/awk", packageRoot))) {
	const source = JSON.parse(
		readFileSync(new URL("share/doc/gawk/SOURCE.json", packageRoot), "utf8"),
	);
	const artifact = new URL("../../toolchain/c/build/awk.COPYING", packageRoot);
	if (!existsSync(artifact)) {
		throw new Error(
			"GNU awk license artifact is missing; run make -C toolchain/c build/awk before packaging",
		);
	}
	const license = readFileSync(artifact);
	const digest = createHash("sha256").update(license).digest("hex");
	if (digest !== source.licenseSha256)
		throw new Error(
			"GNU awk COPYING checksum does not match pinned source metadata",
		);
	const destination = new URL("share/licenses/gawk/", packageRoot);
	mkdirSync(destination, { recursive: true });
	writeFileSync(new URL("COPYING", destination), license);
	process.stdout.write(
		`Staged GNU awk license at ${fileURLToPath(destination)}\n`,
	);
}
