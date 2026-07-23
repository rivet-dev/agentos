import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync, statSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import commandCode from "../dist/index.js";

const packageDir = resolve(import.meta.dirname, "..");

test("package exposes the genuine Command Code v1 CLI aliases", () => {
	const pkg = JSON.parse(readFileSync(resolve(packageDir, "package.json"), "utf8"));
	const manifest = JSON.parse(
		readFileSync(resolve(packageDir, "agentos-package.json"), "utf8"),
	);
	const stagedBuild = JSON.parse(
		readFileSync(
			resolve(packageDir, "dist/command-code/agentos-build.json"),
			"utf8",
		),
	);

	assert.equal(pkg.version, "0.0.1");
	assert.equal(pkg.license, "UNLICENSED");
	assert.equal(pkg.devDependencies["command-code"], "1.1.0");
	assert.deepEqual(
		Object.keys(pkg.bin).sort(),
		["cmd", "cmdc", "command-code", "commandcode"],
	);
	assert.deepEqual(manifest.commands, [
		"cmd",
		"cmdc",
		"command-code",
		"commandcode",
	]);
	assert.equal(manifest.registry.category, "agents");
	assert.equal(stagedBuild.sourcePackage, "command-code@1.1.0");
	assert.equal(typeof commandCode.packagePath, "string");
	assert.equal(statSync(commandCode.packagePath).isFile(), true);

	const packResult = JSON.parse(
		execFileSync("npm", ["pack", "--dry-run", "--json"], {
			cwd: packageDir,
			encoding: "utf8",
		}),
	);
	assert.equal(
		packResult[0].files.some((file) => file.path === "dist/package.aospkg"),
		true,
	);
});

test("every packaged alias starts the upstream CLI", () => {
	for (const alias of ["cmd", "cmdc", "command-code", "commandcode"]) {
		const output = execFileSync(resolve(packageDir, `dist/package/bin/${alias}`), [
			"--version",
		], {
			encoding: "utf8",
		});
		assert.equal(output.trim(), "1.1.0");
	}
});
