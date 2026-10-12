import { execFileSync, spawnSync } from "node:child_process";
import {
	cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, it } from "vitest";

const script = fileURLToPath(new URL("../scripts/stage-runtime.mjs", import.meta.url));
const temporary = [];
const fixtureEnv = Object.fromEntries(
	Object.entries(process.env).filter(([name]) => !name.startsWith("GIT_")),
);
const selected = [
	"defaults.vim",
	"autoload/dist/fixture.vim",
	"import/dist/fixture.vim",
	"pack/dist/opt/fixture/plugin/fixture.vim",
];

function fixture(withGit = true) {
	const root = mkdtempSync(join(tmpdir(), "vim-runtime-stage-"));
	temporary.push(root);
	const packageRoot = join(root, "software/vim");
	const gitRoot = join(root, "toolchain/c/libs/vim");
	const runtime = join(gitRoot, "runtime");
	const entry = join(packageRoot, "scripts/stage-runtime.mjs");
	mkdirSync(dirname(entry), { recursive: true });
	cpSync(script, entry);
	for (const relative of [...selected, "doc/excluded.txt"]) {
		const path = join(runtime, relative);
		mkdirSync(dirname(path), { recursive: true });
		writeFileSync(path, `" synthetic ${relative}\n`);
	}
	if (withGit) {
		const git = (args) => execFileSync("git", ["-c", "maintenance.auto=false", "-C", gitRoot, ...args], {
			env: fixtureEnv, timeout: 10_000, maxBuffer: 1024 * 1024,
		});
		git(["-c", "init.templateDir=", "init", "--quiet"]);
		git(["add", "--", "runtime"]);
		git([
			"-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
			"-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null",
			"commit", "--quiet", "-m", "fixture",
		]);
	}
	return { root, packageRoot, runtime, entry };
}

function stage(value) {
	const env = { ...fixtureEnv };
	delete env.VIM_RUNTIME_SRC;
	return spawnSync(process.execPath, [value.entry], {
		cwd: value.root, env, encoding: "utf8", timeout: 10_000,
		maxBuffer: 1024 * 1024,
	});
}

afterEach(() => {
	for (const path of temporary.splice(0)) rmSync(path, { recursive: true, force: true });
});

it("stages selected tracked dist paths and permits omitted excluded documentation", () => {
	const value = fixture();
	rmSync(join(value.runtime, "doc/excluded.txt"));
	const result = stage(value);
	expect(result.error).toBeUndefined();
	expect(result.status, result.stderr).toBe(0);
	for (const relative of selected) {
		expect(readFileSync(join(value.packageRoot, "share/vim/vim92", relative), "utf8"))
			.toBe(`" synthetic ${relative}\n`);
	}
});

it("rejects missing selected dist paths before replacing existing staged data", () => {
	const value = fixture();
	for (const path of ["autoload/dist", "import/dist", "pack/dist"]) {
		rmSync(join(value.runtime, path), { recursive: true });
	}
	const sentinel = join(value.packageRoot, "share/preserved.txt");
	mkdirSync(dirname(sentinel), { recursive: true });
	writeFileSync(sentinel, "preserve\n");
	const result = stage(value);
	expect(result.error).toBeUndefined();
	expect(result.status).not.toBe(0);
	expect(result.stderr).toContain("missing 3 selected tracked runtime files");
	for (const path of selected.slice(1)) expect(result.stderr).toContain(path);
	expect(readFileSync(sentinel, "utf8")).toBe("preserve\n");
});

it("keeps the existing no-Git artifact staging route", () => {
	const value = fixture(false);
	const result = stage(value);
	expect(result.error).toBeUndefined();
	expect(result.status, result.stderr).toBe(0);
	for (const relative of selected) {
		expect(existsSync(join(value.packageRoot, "share/vim/vim92", relative))).toBe(true);
	}
});
