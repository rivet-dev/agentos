// Nightly: requires the built Git and OpenSSH command packages.
import { AgentOs } from "@rivet-dev/agentos";
import { describe, expect, it } from "vitest";
import buildEssential from "../src/index.js";

describe("build-essential package projection", () => {
	it("runs upstream Git and OpenSSH from this collection alone", async () => {
		const vm = await AgentOs.create({
			software: [buildEssential],
			permissions: { fs: "allow", childProcess: "allow", env: "allow" },
		});
		try {
			const git = await vm.exec("git --version");
			expect(git.exitCode).toBe(0);
			expect(git.stdout).toMatch(/^git version /);
			const gitExecPath = await vm.exec("git --exec-path");
			expect(gitExecPath.exitCode).toBe(0);
			expect(gitExecPath.stdout.trim()).toBe("/opt/agentos/bin");
			const init = await vm.exec(
				"git -c init.defaultBranch=main init /tmp/build-essential-git",
			);
			expect(init.exitCode, init.stderr).toBe(0);
			expect(init.stderr).not.toContain("templates not found");
			const description = new TextDecoder().decode(
				await vm.readFile("/tmp/build-essential-git/.git/description"),
			);
			expect(description).toContain("Unnamed repository");

			const ssh = await vm.exec("ssh -V");
			expect(ssh.exitCode).toBe(0);
			expect(ssh.stderr).toMatch(/^OpenSSH_/);
		} finally {
			await vm.dispose();
		}
	}, 60_000);
});
