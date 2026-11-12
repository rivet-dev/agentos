#!/usr/bin/env node
// Stage the templates built from the same pinned upstream Git source as bin/git.
import { execFileSync } from "node:child_process";
import { chmodSync, cpSync, existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// The TypeScript checks job runs before the separate WASM command build.
// Full package builds omit this flag and require the upstream assets below.
if (process.env.AGENTOS_CI_CODE_ONLY_BUILD === "1") {
	console.log(
		"skipped upstream Git templates and submodule scripts for CI code-only build",
	);
	process.exit(0);
}

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repositoryRoot = resolve(packageRoot, "../..");
const cToolchain = join(repositoryRoot, "toolchain", "c");
const makefile = readFileSync(join(cToolchain, "Makefile"), "utf8");
const version = makefile.match(/^GIT_VERSION\s*:=\s*(\S+)\s*$/m)?.[1];
if (!version) throw new Error("cannot find pinned GIT_VERSION in toolchain/c/Makefile");

const source = join(cToolchain, "build", "git-upstream", `git-${version}`, "templates");
if (!existsSync(join(source, "Makefile"))) {
	throw new Error(`Git source templates are absent at ${source}; build toolchain cmd/git first`);
}
execFileSync("make", ["-C", source, "SHELL_PATH=/bin/sh", "PERL_PATH=perl", "all"], {
	stdio: "inherit",
});

const built = join(source, "blt");
if (!existsSync(join(built, "description")) || !existsSync(join(built, "info", "exclude"))) {
	throw new Error(`upstream Git did not build its standard templates in ${built}`);
}
const target = join(packageRoot, "share", "git-core", "templates");
rmSync(join(packageRoot, "share"), { recursive: true, force: true });
mkdirSync(dirname(target), { recursive: true });
cpSync(built, target, { recursive: true });
console.log(`staged upstream Git ${version} templates`);

// Git's submodule porcelain is an upstream shell script that dispatches to
// submodule--helper. Its two sourced libraries must live beside git's
// executable at the configured gitexecdir, not in the build host's tree.
const gitSource = resolve(source, "..");
const scripts = ["git-submodule", "git-sh-setup", "git-sh-i18n"];
execFileSync("make", [
	"-C", gitSource,
	"uname_S=WASI",
	"prefix=/opt/agentos",
	"gitexecdir=/opt/agentos/bin",
	"template_dir=/opt/agentos/share/git-core/templates",
	"NO_GETTEXT=1", "NO_PERL=1", "NO_PYTHON=1",
	...scripts,
], { stdio: "inherit" });
const bin = join(packageRoot, "bin");
mkdirSync(bin, { recursive: true });
for (const script of scripts) {
	const sourceScript = join(gitSource, script);
	if (!existsSync(sourceScript)) throw new Error(`upstream Git did not build ${script}`);
	const destination = join(bin, script);
	cpSync(sourceScript, destination);
	chmodSync(destination, 0o755);
}
console.log(`staged upstream Git ${version} submodule scripts`);
