// Nightly: compares directory descriptor ownership with native CPython.
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const program = `
import errno, json, os, stat, subprocess, sys

if sys.platform != 'emscripten':
    import resource
    resource.setrlimit(resource.RLIMIT_NOFILE, (64, 64))

root = sys.argv[1]
base = root + '/directory'
os.makedirs(base, exist_ok=True)
with open(base + '/child', 'wb') as target:
    target.write(b'x')

def result(call):
    try:
        return ['ok', call()]
    except OSError as error:
        return ['error', errno.errorcode.get(error.errno, type(error).__name__)]
    except (AttributeError, TypeError, ValueError) as error:
        return ['unsupported', type(error).__name__]

flags = os.O_RDONLY | os.O_DIRECTORY
fd = os.open(base, flags)
controls = {}
controls['fstat_dir'] = result(lambda: stat.S_ISDIR(os.fstat(fd).st_mode))
controls['listdir_fd'] = result(lambda: sorted(os.listdir(fd)))
cwd = os.getcwd()
def change_directory():
    os.fchdir(fd)
    try:
        return os.getcwd() == base
    finally:
        os.chdir(cwd)
controls['fchdir_fd'] = result(change_directory)
alias = os.dup(fd)
controls['dup_same_inode'] = result(lambda: os.fstat(fd).st_ino == os.fstat(alias).st_ino)
os.close(fd)
controls['dup_survives_close'] = result(lambda: stat.S_ISDIR(os.fstat(alias).st_mode))
moved = root + '/renamed'
os.rename(base, moved)
controls['listdir_after_rename'] = result(lambda: sorted(os.listdir(alias)))
def change_after_rename():
    os.fchdir(alias)
    try:
        return os.getcwd() == moved
    finally:
        os.chdir(cwd)
controls['fchdir_after_rename'] = result(change_after_rename)
external = root + '/external'
subprocess.run(['node', '-e', "require('node:fs').renameSync(process.argv[1], process.argv[2])", moved, external], check=True, capture_output=True)
controls['fstat_after_external_rename'] = result(lambda: stat.S_ISDIR(os.fstat(alias).st_mode))
def chmod_after_external_rename():
    os.fchmod(alias, 0o700)
    return stat.S_IMODE(os.stat(external).st_mode) == 0o700
controls['fchmod_after_external_rename'] = result(chmod_after_external_rename)
controls['listdir_after_external_rename'] = result(lambda: sorted(os.listdir(alias)))
def change_after_external_rename():
    os.fchdir(alias)
    try:
        return os.getcwd() == external
    finally:
        os.chdir(cwd)
controls['fchdir_after_external_rename'] = result(change_after_external_rename)
os.close(alias)
base = external

opened = []
terminal_error = None
try:
    for _ in range(128):
        try:
            opened.append(os.open(base, flags))
        except OSError as error:
            terminal_error = errno.errorcode.get(error.errno, type(error).__name__)
            break
finally:
    for descriptor in opened:
        os.close(descriptor)
reopened = os.open(base, flags)
os.close(reopened)
print(json.dumps({'controls': controls, 'opened': len(opened), 'terminal_error': terminal_error, 'reopened': True}))
`;

test("Python directory fds consume the descriptor budget and retain Linux lifetime", async () => {
	const nativeRoot = mkdtempSync(join(tmpdir(), "agentos-python-dirfd-"));
	let vm: AgentOs | undefined;
	try {
		const native = spawnSync("python3", ["-c", program, nativeRoot], {
			encoding: "utf8",
			timeout: 10_000,
		});
		expect(native.status, native.stderr).toBe(0);
		const nativeRow = JSON.parse(native.stdout);
		expect(nativeRow.terminal_error).toBe("EMFILE");
		expect(nativeRow.opened).toBeLessThan(128);
		vm = await AgentOs.create({
			defaultSoftware: false,
			limits: { resources: { maxOpenFds: 64 } },
		});
		const guest = await vm.process.execFile(
			"python",
			["-c", program, "/tmp/dirfd-control"],
			{
				timeoutMs: 30_000,
				output: { capture: "all" },
			},
		);
		expect(guest.exitCode, guest.stderr).toBe(0);
		const guestRow = JSON.parse(guest.stdout);
		// The VM-wide limit is Linux ENFILE; native RLIMIT_NOFILE is EMFILE.
		expect(
			guestRow.terminal_error,
			JSON.stringify({ nativeRow, guestRow }),
		).toBe("ENFILE");
		expect(guestRow.opened).toBeLessThan(128);
		expect(guestRow.reopened).toBe(true);
		expect(guestRow.controls).toEqual(nativeRow.controls);
	} finally {
		await vm?.dispose();
		rmSync(nativeRoot, { recursive: true, force: true });
	}
}, 60_000);
