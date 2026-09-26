import { cpSync, mkdirSync, rmSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const DEFAULT_SOFTWARE = [
	"coreutils",
	"sed",
	"grep",
	"gawk",
	"findutils",
	"diffutils",
	"tar",
	"gzip",
];

const packageRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = join(packageRoot, "..", "..");
const outputDir = join(packageRoot, "dist", "default-software");

rmSync(outputDir, { recursive: true, force: true });
mkdirSync(outputDir, { recursive: true });

for (const name of DEFAULT_SOFTWARE) {
	const source = join(repoRoot, "software", name, "dist", "package.aospkg");
	const size = statSync(source).size;
	if (size === 0) {
		throw new Error(`default software artifact is empty: ${source}`);
	}
	cpSync(source, join(outputDir, `${name}.aospkg`));
}

process.stdout.write(
	`staged ${DEFAULT_SOFTWARE.length} default software artifacts -> ${outputDir}\n`,
);
