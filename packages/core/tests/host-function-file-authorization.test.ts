import { afterEach, describe, expect, test } from "vitest";
import { z } from "zod";
import { AgentOs, createSnapshotExport } from "../src/index.js";

describe("host function file input authorization", () => {
	let vm: AgentOs | undefined;
	const calls: string[] = [];
	afterEach(async () => {
		await vm?.dispose();
		vm = undefined;
		calls.length = 0;
	});

	async function createVm({
		denyPublicRead = false,
		boundedRead = false,
		uid = 1000,
		boundedProcesses = false,
	} = {}) {
		vm = await AgentOs.create({
			defaultSoftware: false,
			limits: {
				resources: {
					...(boundedRead ? { maxPreadBytes: 4096 } : {}),
					...(boundedProcesses ? { maxProcesses: 4 } : {}),
				},
			},
			user: { uid, gid: uid },
			permissions: {
				fs: denyPublicRead
					? {
							default: "allow",
							rules: [
								{
									mode: "deny",
									operations: ["read"],
									paths: ["/inputs/public.json"],
								},
							],
						}
					: "allow",
				childProcess: "allow",
				hostFunction: "allow",
			},
			rootFilesystem: {
				lowers: [
					createSnapshotExport([
						{
							path: "/inputs",
							type: "directory",
							mode: "0755",
							uid: 0,
							gid: 0,
						},
						{
							path: "/inputs/zero.json",
							type: "file",
							mode: "0000",
							uid: 0,
							gid: 0,
							content: '{"marker":"mode-zero"}',
							encoding: "utf8",
						},
						{
							path: "/inputs/large.json",
							type: "file",
							mode: "0644",
							uid: 0,
							gid: 0,
							content: JSON.stringify({ marker: "x".repeat(8192) }),
							encoding: "utf8",
						},
						{
							path: "/inputs/private.json",
							type: "file",
							mode: "0600",
							uid: 0,
							gid: 0,
							content: '{"marker":"private"}',
							encoding: "utf8",
						},
						{
							path: "/inputs/public.json",
							type: "file",
							mode: "0644",
							uid: 0,
							gid: 0,
							content: '{"marker":"public"}',
							encoding: "utf8",
						},
						{
							path: "/inputs/link.json",
							type: "symlink",
							mode: "0777",
							uid: 0,
							gid: 0,
							target: "/inputs/private.json",
						},
						{
							path: "/inputs/closed",
							type: "directory",
							mode: "0700",
							uid: 0,
							gid: 0,
						},
						{
							path: "/inputs/closed/value.json",
							type: "file",
							mode: "0644",
							uid: 0,
							gid: 0,
							content: '{"marker":"untraversable"}',
							encoding: "utf8",
						},
					]),
				],
			},
			hostFunctions: {
				echo: {
					read: {
						inputSchema: z.object({ marker: z.string() }),
						execute: ({ marker }) => {
							calls.push(marker);
							return { marker };
						},
					},
				},
			},
		});
		return vm;
	}

	async function run(command: string, args: string[], cwd?: string) {
		let stdout = "",
			stderr = "";
		const { pid } = await vm!.process.spawn(command, args, {
			...(cwd ? { cwd } : {}),
			onStdout: (data) => {
				stdout += new TextDecoder().decode(data);
			},
			onStderr: (data) => {
				stderr += new TextDecoder().decode(data);
			},
		});
		const { exitCode } = await vm!.process.wait(pid);
		return { exitCode, stdout, stderr };
	}

	for (const registry of [false, true]) {
		const command = registry ? "agentos" : "agentos-echo";
		const prefix = registry ? ["echo", "read"] : ["read"];
		test(`${command} rejects file and directory DAC bypasses, including guest child calls`, async () => {
			await createVm();
			const control = await run("node", [
				"-e",
				"try { require('fs').readFileSync('/inputs/private.json'); process.exitCode=1 } catch(e) { console.log(e.code) }",
			]);
			expect(control.stdout).toBe("EACCES\n");
			for (const path of [
				"/inputs/private.json",
				"/inputs/zero.json",
				"/inputs/link.json",
				"/inputs/closed/value.json",
			]) {
				const result = await run(command, [...prefix, "--json-file", path]);
				expect(result.exitCode, JSON.stringify(result)).not.toBe(0);
				expect(result.stderr).toContain("EACCES");
			}
			const child = await run("node", [
				"-e",
				`const r=require('child_process').spawnSync(${JSON.stringify(command)}, ${JSON.stringify([...prefix, "--json-file", "/inputs/private.json"])}, {encoding:'utf8'}); console.log(JSON.stringify({status:r.status,stdout:r.stdout,stderr:r.stderr}));`,
			]);
			const result = JSON.parse(child.stdout);
			expect(result.status, child.stdout).not.toBe(0);
			expect(result.stderr).toContain("EACCES");
			expect(calls).toEqual([]);
		});

		test(`${command} reads with child uid overrides instead of parent credentials`, async () => {
			await createVm({ uid: 0 });
			const root = await run(command, [
				...prefix,
				"--json-file",
				"/inputs/private.json",
			]);
			expect(root.exitCode, JSON.stringify(root)).toBe(0);
			expect(calls).toEqual(["private"]);
			calls.length = 0;
			const child = await run("node", [
				"-e",
				`const r=require('child_process').spawnSync(${JSON.stringify(command)}, ${JSON.stringify([...prefix, "--json-file", "/inputs/private.json"])}, {uid:1000,gid:1000,encoding:'utf8'}); console.log(JSON.stringify({status:r.status,stdout:r.stdout,stderr:r.stderr,error:r.error?.message}));`,
			]);
			const result = JSON.parse(child.stdout);
			expect(result.error).toBeUndefined();
			expect(result.status, child.stdout).not.toBe(0);
			expect(result.stderr).toContain("EACCES");
			expect(calls).toEqual([]);
		});

		test(`${command} releases process capacity after repeated file input failures`, async () => {
			await createVm({ boundedProcesses: true });
			for (let attempt = 0; attempt < 6; attempt++) {
				const denied = await run(command, [
					...prefix,
					"--json-file",
					"/inputs/missing.json",
				]);
				expect(denied.exitCode, JSON.stringify(denied)).toBe(1);
				expect(denied.stderr).toContain("ENOENT");
			}
			const child = await run("node", [
				"-e",
				`const cp=require('child_process'); for(let i=0;i<6;i++){const r=cp.spawnSync(${JSON.stringify(command)}, ${JSON.stringify([...prefix, "--json-file", "/inputs/missing.json"])}, {encoding:'utf8'}); if(r.status!==1 || !r.stderr.includes('ENOENT')) throw new Error(JSON.stringify(r));} console.log('released');`,
			]);
			expect(child.exitCode, JSON.stringify(child)).toBe(0);
			expect(child.stdout).toBe("released\n");
			const allowed = await run(command, [
				...prefix,
				"--json-file",
				"/inputs/public.json",
			]);
			expect(allowed.exitCode, JSON.stringify(allowed)).toBe(0);
			expect(calls).toEqual(["public"]);
		});

		test(`${command} preserves allowed relative file, JSON and flag input`, async () => {
			await createVm();
			for (const args of [
				["--json-file", "public.json"],
				["--json", '{"marker":"json"}'],
				["--marker", "flag"],
			]) {
				const result = await run(command, [...prefix, ...args], "/inputs");
				expect(result.exitCode, JSON.stringify(result)).toBe(0);
			}
			if (registry) {
				for (const args of [
					["list-host-functions"],
					["echo", "read", "--help"],
				]) {
					const result = await run("agentos", args);
					expect(result.exitCode, JSON.stringify(result)).toBe(0);
				}
			}
			expect(calls).toEqual(["public", "json", "flag"]);
		});

		test(`${command} rejects trailing arguments after JSON input`, async () => {
			await createVm();
			const result = await run(command, [
				...prefix,
				"--json",
				'{"marker":"valid-json"}',
				"unexpected-extra-argument",
			]);
			expect(result.exitCode, JSON.stringify(result)).toBe(1);
			expect(result.stderr).toContain("Unknown flag: --json");
			expect(calls).toEqual([]);
		});

		test(`${command} bounds file input before allocation`, async () => {
			await createVm({ boundedRead: true });
			const result = await run(command, [
				...prefix,
				"--json-file",
				"/inputs/large.json",
			]);
			expect(result.exitCode, JSON.stringify(result)).not.toBe(0);
			expect(result.stderr).toMatch(/max_pread_bytes|maxPreadBytes/);
			expect(calls).toEqual([]);
		});

		test(`${command} honors filesystem policy for file input`, async () => {
			await createVm({ denyPublicRead: true });
			const result = await run(command, [
				...prefix,
				"--json-file",
				"/inputs/public.json",
			]);
			expect(result.exitCode, JSON.stringify(result)).not.toBe(0);
			expect(calls).toEqual([]);
		});
	}
});
