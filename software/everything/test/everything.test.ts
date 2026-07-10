import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import everything, {
	codex,
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
	grep,
	gzip,
	jq,
	ripgrep,
	sed,
	sqlite3,
	tar,
	tree,
	procps,
	psmisc,
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
	sqlite3,
	vim,
	zip,
	unzip,
	jq,
	ripgrep,
	fd,
	tree,
	procps,
	psmisc,
	file,
	yq,
	codex,
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
		for (const descriptor of everything) {
			expect(descriptor).toEqual({
				packagePath: expect.stringMatching(/\/package\.aospkg$/),
			});
		}
	});
});
