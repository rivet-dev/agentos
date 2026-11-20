import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import everything, {
	coreutils,
	curl,
	diffutils,
	duckdb,
	envsubst,
	fd,
	file,
	findutils,
	gawk,
	git,
	ssh,
	grep,
	gzip,
	jq,
	ripgrep,
	sed,
	sqlite3,
	tar,
	tree,
	unzip,
	vim,
	wget,
	yq,
	zip,
} from "../src/index.js";

const packageDir = new URL("..", import.meta.url).pathname;

const expectedMembers = [
	coreutils,
	sed,
	grep,
	gawk,
	findutils,
	diffutils,
	tar,
	gzip,
	curl,
	wget,
	duckdb,
	envsubst,
	git,
	ssh,
	sqlite3,
	vim,
	zip,
	unzip,
	jq,
	ripgrep,
	fd,
	tree,
	file,
	yq,
];

describe("everything meta-package", () => {
	it("has a registry manifest", () => {
		const manifest = JSON.parse(
			readFileSync(join(packageDir, "agentos-package.json"), "utf8"),
		);

		expect(manifest.registry).toMatchObject({
			title: "Everything",
			category: "meta",
		});
	});

	it("exports every command package descriptor once", () => {
		expect(everything).toEqual(expectedMembers);
		expect(new Set(everything).size).toBe(everything.length);
		expect(everything).toContain(git);
		expect(everything).toContain(ssh);
		for (const descriptor of everything) {
			expect(descriptor).toEqual({
				packagePath: expect.stringMatching(/\/package\.aospkg$/),
			});
		}
	});
});
