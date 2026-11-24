import { afterEach, describe, expect, test } from "vitest";
import { AgentOs, createSnapshotExport } from "../src/index.js";

describe("POSIX child script authorization", () => {
	let vm: AgentOs | undefined;
	afterEach(async () => {
		await vm?.dispose();
		vm = undefined;
	});
	for (const uid of [0, 1000]) {
		test(`uid ${uid} parent preserves script authorization through shell interpreter resolution`, async () => {
			vm = await AgentOs.create({
				user: { uid, gid: uid },
				permissions: { fs: "allow", childProcess: "allow" },
				limits: { resources: { maxProcesses: 4 } },
				rootFilesystem: {
					lowers: [
						createSnapshotExport([
							{
								path: "/scripts",
								type: "directory",
								mode: "0755",
								uid: 0,
								gid: 0,
							},
							...(
								[
									["private", "0700"],
									["public", "0755"],
									["execute-only", "0111"],
									["read-only", "0644"],
								] as const
							).map(([name, mode]) => ({
								path: `/scripts/${name}`,
								type: "file" as const,
								mode,
								uid: 0,
								gid: 0,
								content: "#!/bin/echo\n",
								encoding: "utf8" as const,
							})),
							{
								path: "/scripts/link",
								type: "symlink",
								mode: "0777",
								uid: 0,
								gid: 0,
								target: "/scripts/private",
							},
						]),
					],
				},
			});
			const result = await vm.execArgv("node", [
				"-e",
				`
				const assert=require('node:assert/strict'), cp=require('node:child_process');
				const opts={uid:1000,gid:1000,cwd:'/scripts',encoding:'utf8'};
				for(let i=0;i<6;i++) for(const path of ['/scripts/private','./private','/scripts/link','/scripts/read-only']) {
					const r=cp.spawnSync('sh',['-c',path],opts);
					assert.notEqual(r.status,0,JSON.stringify({path,...r})); assert.equal(r.stdout,'');
				}
				for(const path of ['/scripts/public','/scripts/execute-only']) {
					const r=cp.spawnSync('sh',['-c',path],opts);
					assert.equal(r.status,0,JSON.stringify(r));assert.equal(r.stdout,path+'\\n');
				}
				console.log('posix-authorized');
			`,
			]);
			expect(result.exitCode, JSON.stringify(result)).toBe(0);
			expect(result.stdout).toBe("posix-authorized\n");
		});
	}
});
