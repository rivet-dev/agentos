import {
	closeSync,
	existsSync,
	openSync,
	readdirSync,
	readSync,
	statSync,
} from "node:fs";
import { dirname, join } from "node:path";

interface RegistryPackageRef {
	packagePath: string;
}

const AOSPKG_MAGIC = Buffer.from([0x89, 0x41, 0x4f, 0x53]);

function isPackedAospkg(path: string): boolean {
	try {
		if (statSync(path).size <= 16) return false;
		const fd = openSync(path, "r");
		try {
			const head = Buffer.alloc(4);
			readSync(fd, head, 0, 4, 0);
			return head.equals(AOSPKG_MAGIC);
		} finally {
			closeSync(fd);
		}
	} catch {
		return false;
	}
}

export function hasBuiltRegistryCommands(
	packages: readonly RegistryPackageRef[],
): boolean {
	return packages.every((pkg) => {
		const path = pkg.packagePath;
		if (path.endsWith(".aospkg")) {
			if (!isPackedAospkg(path)) return false;
			return hasCompleteCommandDir(join(dirname(path), "package"));
		}
		return (
			existsSync(join(path, "agentos-package.json")) &&
			hasCompleteCommandDir(path)
		);
	});
}

function hasCompleteCommandDir(path: string): boolean {
	if (!existsSync(path)) return true;
	const binDir = join(path, "bin");
	if (!existsSync(binDir)) return false;
	const entries = readdirSync(binDir);
	return (
		entries.length > 0 &&
		entries.every((entry) => existsSync(join(binDir, entry)))
	);
}
