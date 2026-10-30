import { cpSync, mkdirSync, readFileSync, rmSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const packageRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = join(packageRoot, "..", "..");
const listPath = join(repoRoot, "software", "default-software.json");
const outputDir = join(packageRoot, "dist", "default-software");
// The checks job compiles TypeScript before its parallel WASM producer finishes.
// Publishing and runtime tests omit this flag and still require all packages.
if (process.env.AGENTOS_CI_CODE_ONLY_BUILD === "1") {
	rmSync(outputDir, { recursive: true, force: true });
	process.stdout.write("skipped default software staging for CI code-only build\n");
	process.exit(0);
}
const defaultSoftware = JSON.parse(readFileSync(listPath, "utf8"));

rmSync(outputDir, { recursive: true, force: true });
mkdirSync(outputDir, { recursive: true });

for (const name of defaultSoftware) {
	const source = join(repoRoot, "software", name, "dist", "package.aospkg");
	const size = statSync(source).size;
	if (size === 0) {
		throw new Error(`default software artifact is empty: ${source}`);
	}
	cpSync(source, join(outputDir, `${name}.aospkg`));
}
// The runtime reads the list from the staged directory, so the published
// package carries it next to the artifacts.
cpSync(listPath, join(outputDir, "default-software.json"));

process.stdout.write(
	`staged ${defaultSoftware.length} default software artifacts -> ${outputDir}\n`,
);
