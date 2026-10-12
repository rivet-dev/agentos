// Nightly: compare Python stat metadata with native CPython on Linux.
import { spawnSync } from "node:child_process";
import { linkSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const metadata = `
import os, json, sys
base = sys.argv[1]
os.makedirs(base, exist_ok=True)
path = base + '/file'
alias = base + '/alias'
fd = os.open(path, os.O_RDWR)
os.fchmod(fd, 0o640)
os.fchown(fd, os.getuid(), os.getgid())
os.utime(path, ns=(1234567890123000000, 1234567990456000000))
def fields(s):
    return [s.st_uid, s.st_gid, s.st_nlink, s.st_mode & 0o777, s.st_size, s.st_atime_ns, s.st_mtime_ns]
rows = [['path', fields(os.stat(path))], ['descriptor', fields(os.fstat(fd))]]
rows.append(['identity', os.stat(path).st_ino == os.stat(alias).st_ino == os.fstat(fd).st_ino])
os.unlink(path)
rows.append(['unlinked-alias', os.fstat(fd).st_nlink])
os.unlink(alias)
rows.append(['unlinked-final', os.fstat(fd).st_nlink])
os.close(fd)
print(json.dumps(rows))
`;

test("Python stat and fstat preserve ownership, links, timestamps and inode identity", async () => {
	const root = mkdtempSync(join(tmpdir(), "agentos-python-stat-"));
	let vm: AgentOs | undefined;
	try {
		writeFileSync(join(root, "file"), "metadata");
		linkSync(join(root, "file"), join(root, "alias"));
		const native = spawnSync("python3", ["-c", metadata, root], {
			encoding: "utf8",
			timeout: 10_000,
		});
		expect(native.status, native.stderr).toBe(0);
		const uid = process.getuid!();
		const gid = process.getgid!();
		vm = await AgentOs.create({
			defaultSoftware: false,
			user: { uid, gid, euid: uid, egid: gid },
		});
		const seeded = await vm.process.execFile(
			"node",
			[
				"-e",
				"const fs = require('node:fs'); fs.mkdirSync('/tmp/stat-metadata'); fs.writeFileSync('/tmp/stat-metadata/file', 'metadata'); fs.linkSync('/tmp/stat-metadata/file', '/tmp/stat-metadata/alias');",
			],
			{ timeoutMs: 5_000, output: { capture: "all" } },
		);
		expect(seeded.exitCode, seeded.stderr).toBe(0);
		const guest = await vm.process.execFile(
			"python",
			["-c", metadata, "/tmp/stat-metadata"],
			{
				timeoutMs: 30_000,
				output: { capture: "all" },
			},
		);
		expect(guest.exitCode, guest.stderr).toBe(0);
		expect(JSON.parse(guest.stdout)).toEqual(JSON.parse(native.stdout));
	} finally {
		await vm?.dispose();
		rmSync(root, { recursive: true, force: true });
	}
}, 60_000);
