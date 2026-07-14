import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
	existsSync,
	mkdirSync,
	readFileSync,
	writeFileSync,
} from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../../..");
const packageRoot = resolve(repoRoot, "examples/experiments/browser-base-shell");
const commandsDir = resolve(
	repoRoot,
	"registry/native/target/wasm32-wasip1/release/commands",
);
const shCommand = resolve(commandsDir, "sh");
const vimCommand = resolve(
	commandsDir,
	"vim",
);

function runMake(command) {
	const result = spawnSync("make", ["-C", "registry/native", `cmd/${command}`], {
		cwd: repoRoot,
		stdio: "inherit",
	});
	if (result.error) throw result.error;
	if (result.status !== 0) process.exit(result.status ?? 1);
}

// The missing-child-PID behavior lives in a vendored Brush patch. Fingerprint
// its tracked build inputs so an old ignored target artifact cannot silently
// preserve the warning after a checkout or branch update.
const shInputs = [
	resolve(repoRoot, "registry/native/crates/commands/sh/Cargo.toml"),
	resolve(repoRoot, "registry/native/crates/commands/sh/src/main.rs"),
	resolve(
		repoRoot,
		"registry/native/patches/crates/brush-core/0004-wasi-child-pid.patch",
	),
];
const shFingerprint = createHash("sha256")
	.update(shInputs.map((path) => readFileSync(path)).join("\0"))
	.digest("hex");
const cacheDir = resolve(packageRoot, ".cache");
const shStamp = resolve(cacheDir, "sh-command.sha256");
const currentShFingerprint = existsSync(shStamp)
	? readFileSync(shStamp, "utf8").trim()
	: "";
if (!existsSync(shCommand) || currentShFingerprint !== shFingerprint) {
	runMake("sh");
	mkdirSync(cacheDir, { recursive: true });
	writeFileSync(shStamp, `${shFingerprint}\n`);
} else {
	console.log(`using current Brush command ${shCommand}`);
}

if (!existsSync(vimCommand)) runMake("vim");
else console.log(`using existing Vim command ${vimCommand}`);
