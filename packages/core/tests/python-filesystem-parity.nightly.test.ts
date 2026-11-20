// Nightly: loads the bundled Python runtime and compares with native CPython.
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const creationModes = `
import os, json, sys
base = sys.argv[1]
os.makedirs(base, exist_ok=True)
rows = []
for mask in [0, 0o022, 0o077, 0o777]:
    old = os.umask(mask)
    try:
        for mode in [0o666, 0o600, 0o644]:
            path = base + '/file-' + str(mask) + '-' + str(mode)
            fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, mode)
            try:
                os.write(fd, b'payload')
            finally:
                os.close(fd)
            rows.append(['file', mask, mode, os.stat(path).st_mode & 0o777, os.stat(path).st_size])
        path = base + '/dir-' + str(mask)
        os.mkdir(path, 0o777)
        rows.append(['directory', mask, 0o777, os.stat(path).st_mode & 0o777])
        # Restore traversal so the native fixture can be removed safely.
        os.chmod(path, 0o700)
    finally:
        os.umask(old)
print(json.dumps(rows))
`;

test("Python creation modes and umask match Linux, including writes through a mode-zero open file", async () => {
	const nativeRoot = mkdtempSync(join(tmpdir(), "agentos-python-mode-"));
	let vm: AgentOs | undefined;
	try {
		const native = spawnSync("python3", ["-c", creationModes, nativeRoot], {
			encoding: "utf8",
			timeout: 10_000,
		});
		expect(native.status, native.stderr).toBe(0);
		vm = await AgentOs.create({ defaultSoftware: false });
		const guest = await vm.process.execFile(
			"python",
			["-c", creationModes, "/tmp/creation-modes"],
			{
				timeoutMs: 30_000,
				output: { capture: "all" },
			},
		);
		expect(guest.exitCode, guest.stderr).toBe(0);
		expect(JSON.parse(guest.stdout)).toEqual(JSON.parse(native.stdout));
	} finally {
		await vm?.dispose();
		rmSync(nativeRoot, { recursive: true, force: true });
	}
}, 60_000);

const descriptorLifetime = `
import os, json, sys, mmap
base = sys.argv[1]
os.makedirs(base, exist_ok=True)
path = base + '/opened'
rows = []
fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_EXCL, 0o600)
os.write(fd, b'abcdef')
try:
    os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o777)
except OSError as error:
    rows.append(['exclusive', type(error).__name__])
alias = os.dup(fd)
os.close(fd)
os.lseek(alias, 0, os.SEEK_SET)
rows.append(['dup-read', os.read(alias, 6).decode()])
os.fchmod(alias, 0)
os.unlink(path)
os.lseek(alias, 0, os.SEEK_SET)
os.write(alias, b'XY')
os.lseek(alias, 0, os.SEEK_SET)
rows.append(['unlinked', os.read(alias, 6).decode(), os.fstat(alias).st_mode & 0o777])
os.ftruncate(alias, 4)
rows.append(['truncate', os.fstat(alias).st_size])
shared = mmap.mmap(alias, 4, access=mmap.ACCESS_WRITE)
shared[0:2] = b'MM'
shared.flush()
shared.close()
os.lseek(alias, 0, os.SEEK_SET)
rows.append(['mapped-shared', os.read(alias, 4).decode()])
private = mmap.mmap(alias, 4, access=mmap.ACCESS_COPY)
private[0:2] = b'NO'
private.close()
os.lseek(alias, 0, os.SEEK_SET)
rows.append(['mapped-private', os.read(alias, 4).decode()])
os.close(alias)
path = base + '/write-only'
fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o200)
os.write(fd, b'one')
os.close(fd)
fd = os.open(path, os.O_WRONLY | os.O_APPEND)
os.write(fd, b'two')
os.close(fd)
os.chmod(path, 0o600)
with open(path, 'rb') as source:
    rows.append(['append', source.read().decode()])
fd = os.open(path, os.O_WRONLY | os.O_TRUNC)
os.close(fd)
rows.append(['open-truncate', os.stat(path).st_size])
print(json.dumps(rows))
`;

test("Python open descriptions survive dup, unlink, chmod, mapping, and append like Linux", async () => {
	const nativeRoot = mkdtempSync(join(tmpdir(), "agentos-python-fd-"));
	let vm: AgentOs | undefined;
	try {
		const native = spawnSync(
			"python3",
			["-c", descriptorLifetime, nativeRoot],
			{
				encoding: "utf8",
				timeout: 10_000,
			},
		);
		expect(native.status, native.stderr).toBe(0);
		vm = await AgentOs.create({ defaultSoftware: false });
		const guest = await vm.process.execFile(
			"python",
			["-c", descriptorLifetime, "/tmp/descriptor-lifetime"],
			{
				timeoutMs: 30_000,
				output: { capture: "all" },
			},
		);
		expect(guest.exitCode, guest.stderr).toBe(0);
		expect(JSON.parse(guest.stdout)).toEqual(JSON.parse(native.stdout));
	} finally {
		await vm?.dispose();
		rmSync(nativeRoot, { recursive: true, force: true });
	}
}, 60_000);

const failedOpen = `
import os, sys, errno, json
if sys.platform != 'emscripten':
    import resource
    resource.setrlimit(resource.RLIMIT_NOFILE, (64, 64))
base = sys.argv[1]
os.makedirs(base, exist_ok=True)
path = base + '/preserved'
with open(path, 'w') as target:
    target.write('preserved')
opened = []
errors = []
try:
    for index in range(128):
        try:
            opened.append(os.open(path, os.O_RDONLY))
        except OSError as error:
            errors.append(errno.errorcode[error.errno])
            break
    try:
        fd = os.open(path, os.O_WRONLY | os.O_TRUNC)
        os.close(fd)
        errors.append('unexpected-open')
    except OSError as error:
        errors.append(errno.errorcode[error.errno])
finally:
    for fd in opened:
        os.close(fd)
with open(path) as source:
    print(json.dumps([errors, source.read()]))
`;

test("Python failed descriptor admission preserves an O_TRUNC target", async () => {
	const nativeRoot = mkdtempSync(join(tmpdir(), "agentos-python-admission-"));
	let vm: AgentOs | undefined;
	try {
		const native = spawnSync("python3", ["-c", failedOpen, nativeRoot], {
			encoding: "utf8",
			timeout: 10_000,
		});
		expect(native.status, native.stderr).toBe(0);
		expect(JSON.parse(native.stdout)).toEqual([
			["EMFILE", "EMFILE"],
			"preserved",
		]);
		vm = await AgentOs.create({
			defaultSoftware: false,
			limits: { resources: { maxOpenFds: 64 } },
		});
		const guest = await vm.process.execFile(
			"python",
			["-c", failedOpen, "/tmp/failed-open"],
			{
				timeoutMs: 30_000,
				output: { capture: "all" },
			},
		);
		expect(guest.exitCode, guest.stderr).toBe(0);
		// The VM-wide cap corresponds to Linux ENFILE; the native fixture uses
		// the process-wide RLIMIT_NOFILE (EMFILE). Both must preserve the file.
		expect(JSON.parse(guest.stdout)).toEqual([
			["ENFILE", "ENFILE"],
			JSON.parse(native.stdout)[1],
		]);
	} finally {
		await vm?.dispose();
		rmSync(nativeRoot, { recursive: true, force: true });
	}
}, 60_000);

test("Python duplicated descriptors obey the VM descriptor limit and leave the original usable", async () => {
	const vm = await AgentOs.create({
		defaultSoftware: false,
		limits: { resources: { maxOpenFds: 64 } },
	});
	try {
		const program = `
import os, errno, json
fd = os.open('/tmp/dup-limit', os.O_RDWR | os.O_CREAT, 0o600)
aliases = []
error_name = None
try:
    for index in range(128):
        try:
            aliases.append(os.dup(fd))
        except OSError as error:
            error_name = errno.errorcode[error.errno]
            break
finally:
    for alias in aliases:
        os.close(alias)
os.write(fd, b'alive')
os.lseek(fd, 0, os.SEEK_SET)
contents = os.read(fd, 5).decode()
os.close(fd)
print(json.dumps([error_name, contents]))
`;
		const result = await vm.process.execFile("python", ["-c", program], {
			timeoutMs: 30_000,
			output: { capture: "all" },
		});
		expect(result.exitCode, result.stderr).toBe(0);
		expect(JSON.parse(result.stdout)).toEqual(["ENFILE", "alive"]);
	} finally {
		await vm.dispose();
	}
}, 60_000);
