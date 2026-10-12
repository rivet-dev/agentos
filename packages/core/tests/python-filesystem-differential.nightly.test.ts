// Nightly: compares the bundled Python runtime with native CPython.
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const program = `
import errno, json, os, sys

root = sys.argv[1]
os.makedirs(root, exist_ok=True)
old_umask = os.umask(0o022)
rows = []

def error_name(error):
    return errno.errorcode.get(error.errno, type(error).__name__)

def run_seed(seed):
    state = seed + 1
    handles = {}
    next_handle = 0
    base = root + '/seed-' + str(seed)
    os.mkdir(base, 0o700)
    paths = [base + '/' + name for name in ('a', 'b', 'c')]
    trace = []

    def random32():
        nonlocal state
        state ^= (state << 13) & 0xffffffff
        state ^= state >> 17
        state ^= (state << 5) & 0xffffffff
        state &= 0xffffffff
        return state

    def chosen_handle(value):
        if not handles:
            return None
        return sorted(handles)[value % len(handles)]

    try:
        for step in range(32):
            value = random32()
            operation = (step + seed) % 9 if step < 9 else value % 9
            index = (value >> 8) % 3
            path = paths[index]
            data = bytes((value & 255, (value >> 8) & 255, 0, 255))
            result = None
            try:
                if operation == 0:  # open, retaining a logical handle
                    if len(handles) == 4:
                        result = ['full']
                    else:
                        fd = os.open(path, os.O_RDWR | os.O_CREAT, 0o600)
                        handle = next_handle
                        next_handle += 1
                        handles[handle] = fd
                        result = ['open', handle]
                elif operation == 1:  # append through a separate description
                    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
                    try:
                        result = ['append', os.write(fd, data)]
                    finally:
                        os.close(fd)
                elif operation == 2:  # seek and read exact bytes
                    handle = chosen_handle(value >> 16)
                    if handle is None:
                        result = ['no-fd']
                    else:
                        fd = handles[handle]
                        offset = os.lseek(fd, (value >> 24) % 5, os.SEEK_SET)
                        result = ['read', handle, offset, os.read(fd, 12).hex()]
                elif operation == 3:  # write at the shared description offset
                    handle = chosen_handle(value >> 16)
                    if handle is None:
                        result = ['no-fd']
                    else:
                        fd = handles[handle]
                        result = ['write', handle, os.write(fd, data), os.lseek(fd, 0, os.SEEK_CUR)]
                elif operation == 4:  # permission metadata
                    mode = (0o600, 0o640, 0o644)[(value >> 16) % 3]
                    os.chmod(path, mode)
                    result = ['chmod', mode]
                elif operation == 5:  # unlink can occur while a handle remains open
                    os.unlink(path)
                    result = ['unlink']
                elif operation == 6:  # rename can replace an open destination
                    target = (index + 1 + ((value >> 16) % 2)) % 3
                    os.rename(path, paths[target])
                    result = ['rename', target]
                elif operation == 7:  # dup shares file offset
                    handle = chosen_handle(value >> 16)
                    if handle is None:
                        result = ['no-fd']
                    elif len(handles) == 4:
                        result = ['full']
                    else:
                        duplicate = next_handle
                        next_handle += 1
                        handles[duplicate] = os.dup(handles[handle])
                        result = ['dup', handle, duplicate]
                else:
                    handle = chosen_handle(value >> 16)
                    if handle is None:
                        result = ['no-fd']
                    else:
                        os.close(handles.pop(handle))
                        result = ['close', handle]
            except OSError as error:
                result = ['error', error_name(error)]
            trace.append([seed, step, operation, index, result])

        final = []
        for index, path in enumerate(paths):
            try:
                info = os.stat(path)
                with open(path, 'rb') as source:
                    contents = source.read().hex()
                final.append([index, info.st_mode & 0o777, contents])
            except OSError as error:
                final.append([index, error_name(error)])
        return [trace, final]
    finally:
        for fd in handles.values():
            os.close(fd)

try:
    for seed in range(32):
        rows.append(run_seed(seed))
finally:
    os.umask(old_umask)
print(json.dumps(rows, separators=(',', ':')))
`;

test("Python fd and path operations match native CPython for 32 reproducible seeds", async () => {
	const nativeRoot = mkdtempSync(
		join(tmpdir(), "agentos-python-differential-"),
	);
	let vm: AgentOs | undefined;
	try {
		const native = spawnSync("python3", ["-c", program, nativeRoot], {
			encoding: "utf8",
			timeout: 20_000,
			maxBuffer: 4 * 1024 * 1024,
		});
		expect(native.status, native.stderr).toBe(0);
		vm = await AgentOs.create({ defaultSoftware: false });
		const guest = await vm.process.execFile(
			"python",
			["-c", program, "/tmp/python-differential"],
			{
				timeoutMs: 60_000,
				output: { capture: "all" },
			},
		);
		expect(guest.exitCode, guest.stderr).toBe(0);
		const nativeRows = JSON.parse(native.stdout);
		const guestRows = JSON.parse(guest.stdout);
		for (let seed = 0; seed < 32; seed++) {
			for (let step = 0; step < 32; step++) {
				expect(
					guestRows[seed]?.[0]?.[step],
					`seed ${seed}, step ${step}`,
				).toEqual(nativeRows[seed][0][step]);
			}
			expect(guestRows[seed]?.[1], `final files for seed ${seed}`).toEqual(
				nativeRows[seed][1],
			);
		}
	} finally {
		await vm?.dispose();
		rmSync(nativeRoot, { recursive: true, force: true });
	}
}, 90_000);
