import { afterEach, describe, expect, test } from "vitest";
import { AgentOs, createSnapshotExport } from "../src/index.js";

describe("child executable header privacy", () => {
	let vm: AgentOs | undefined;
	afterEach(async () => {
		await vm?.dispose();
		vm = undefined;
	});
	for (const nested of [false, true]) {
		test(`${nested ? "nested" : "root"} exec errors preserve the requested path without exposing unreadable headers`, async () => {
			const marker = "synthetic-private-interpreter";
			vm = await AgentOs.create({
				defaultSoftware: false,
				user: { uid: 1000, gid: 1000 },
				permissions: { fs: "allow", childProcess: "allow" },
				limits: { resources: { maxProcesses: 4 } },
				rootFilesystem: {
					lowers: [
						createSnapshotExport([
							...(
								[
									["protected", "0700"],
									["execute-only", "0111"],
								] as const
							).map(([name, mode]) => ({
								path: `/${name}`,
								type: "file" as const,
								mode,
								uid: 0,
								gid: 0,
								content: `#!/missing/${marker}\n`,
								encoding: "utf8" as const,
							})),
							{
								path: "/public.js",
								type: "file",
								mode: "0755",
								uid: 0,
								gid: 0,
								content: 'console.log("public-ok")',
								encoding: "utf8",
							},
						]),
					],
				},
			});
			const checks = `
				const assert=require('node:assert/strict'), fs=require('node:fs'), cp=require('node:child_process');
				for (let i=0;i<6;i++) for (const [path,code] of [['/protected','EACCES'],['/execute-only','ENOENT']]) {
					assert.throws(()=>fs.readFileSync(path),{code:'EACCES'});
					const r=cp.spawnSync(path,[],{encoding:'utf8'});
					assert.equal(r.error?.code,code,JSON.stringify(r));
					assert(!JSON.stringify(r).includes(${JSON.stringify(marker)}),'unreadable header leaked: '+JSON.stringify(r));
					assert(r.error.message.includes(path),JSON.stringify(r));
				}
				const r=cp.spawnSync('/public.js',[],{encoding:'utf8'});
				assert.equal(r.status,0,JSON.stringify(r));assert.equal(r.stdout,'public-ok\\n');
				console.log('header-private');
			`;
			const source = nested
				? `
				const assert=require('node:assert/strict');
				const r=require('node:child_process').spawnSync('node',['-e',${JSON.stringify(checks)}],{encoding:'utf8'});
				assert.equal(r.status,0,JSON.stringify(r));process.stdout.write(r.stdout);
			`
				: checks;
			const result = await vm.execArgv("node", ["-e", source]);
			expect(result.exitCode, JSON.stringify(result)).toBe(0);
			expect(result.stdout).toBe("header-private\n");
		});
	}
});
