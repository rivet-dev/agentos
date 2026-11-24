// Nightly: mappings retain their opened file even when descriptor numbers are reused.
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const lifetime = `import os,mmap,json,sys
base=sys.argv[1]
os.makedirs(base,exist_ok=True)
p=base+'/file'
fd=os.open(p,os.O_RDWR|os.O_CREAT|os.O_EXCL,0o600)
os.write(fd,b'abcdefghijklmnop')
alias=os.dup(fd)
m=mmap.mmap(fd,8,access=mmap.ACCESS_WRITE)
os.close(fd)
os.unlink(p)
m[0:2]=b'AB'
m.flush()
rows=[['shared',m[:].decode()]]
m.close()
os.lseek(alias,0,os.SEEK_SET)
rows.append(['unlinked',os.read(alias,16).decode()])
p=mmap.mmap(alias,8,access=mmap.ACCESS_COPY)
p[0:2]=b'XY'
p.close()
os.lseek(alias,0,os.SEEK_SET)
rows.append(['private',os.read(alias,16).decode()])
before = len(os.listdir('/proc/self/fd'))
for _ in range(80):
    temporary = mmap.mmap(alias, 8, access=mmap.ACCESS_COPY)
    temporary.close()
rows.append(['descriptors-released', len(os.listdir('/proc/self/fd')) == before])
os.close(alias)
print(json.dumps(rows))`;

const closeRange = `import os,mmap,json,sys
base=sys.argv[1]
os.makedirs(base,exist_ok=True)
fd=os.open(base+'/file',os.O_RDWR|os.O_CREAT|os.O_EXCL,0o600)
os.write(fd,b'abcdefghijklmnop')
before=len(os.listdir('/proc/self/fd'))
rows=[]
for index in range(8):
    mapped=mmap.mmap(fd,8,access=mmap.ACCESS_WRITE)
    os.closerange(fd+1,64)
    mapped[0:1]=bytes([65+index])
    mapped.flush()
    mapped.close()
    os.lseek(fd,0,os.SEEK_SET)
    rows.append(os.read(fd,8).decode())
rows.append(len(os.listdir('/proc/self/fd'))==before)
os.close(fd)
print(json.dumps(rows))`;

for (const mode of ["closed", "reused", "close-range"]) {
	const reuse = mode === "reused";
	test(`Python mmap retains descriptor ownership: ${mode}`, async () => {
		let code = mode === "close-range" ? closeRange : lifetime;
		if (reuse) {
			code = code.replace(
				"os.unlink(p)\nm[0:2]",
				"os.unlink(p)\nreplacement=os.open(base+'/replacement',os.O_RDWR|os.O_CREAT|os.O_EXCL,0o600)\nassert replacement == fd\nos.write(replacement,b'REPLACEMENT-DATA')\nm[0:2]",
			);
			code = code.replace(
				"print(json.dumps(rows))",
				"os.lseek(replacement,0,os.SEEK_SET)\nrows.append(['replacement',os.read(replacement,16).decode()])\nos.close(replacement)\nprint(json.dumps(rows))",
			);
		}
		const root = mkdtempSync(join(tmpdir(), "agentos-python-map-"));
		let vm: AgentOs | undefined;
		try {
			const native = spawnSync("python3", ["-c", code, root], {
				encoding: "utf8",
				timeout: 10_000,
			});
			expect(native.status, native.stderr).toBe(0);
			vm = await AgentOs.create({
				defaultSoftware: false,
				limits: { resources: { maxOpenFds: 64 } },
			});
			const guest = await vm.process.execFile(
				"python",
				["-c", code, "/tmp/mapping"],
				{ timeoutMs: 30_000, output: { capture: "all" } },
			);
			expect(guest.exitCode, guest.stderr).toBe(0);
			expect(JSON.parse(guest.stdout)).toEqual(JSON.parse(native.stdout));
		} finally {
			await vm?.dispose();
			rmSync(root, { recursive: true, force: true });
		}
	}, 60_000);
}
