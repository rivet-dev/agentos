import { describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import { SidecarRejectedError } from "../src/sidecar-errors.js";

const LIMIT = 1024 * 1024;
const WRITE = 1536 * 1024;
const permissions = { fs: "allow", network: "deny", childProcess: "allow", process: "allow", env: "allow", hostFunction: "deny" } as const;

async function rejection(operation: Promise<void>): Promise<unknown> {
    try { await operation; } catch (error) { return error; }
    return new Error("quota write unexpectedly succeeded");
}

describe("native trusted filesystem quota ownership", () => {
    test("preserves typed host totals, guest ENOSPC and stored bytes after rejection", async () => {
        const failures: unknown[] = [];
        const check = (assertion: () => void) => { try { assertion(); } catch (error) { failures.push(error); } };
        let sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>> | undefined;
        let vm: AgentOs | undefined;
        try {
            sidecar = await AgentOs.createSidecar();
            vm = await AgentOs.create({ defaultSoftware: false, permissions,
                sidecar: { kind: "explicit", handle: sidecar }, limits: { resources: { maxFilesystemBytes: LIMIT } } });
            const sentinel = new Uint8Array([7, 11, 19]);
            await vm.writeFile("/tmp/quota-existing.bin", sentinel);
            const first = await rejection(vm.writeFile("/tmp/quota-existing.bin", new Uint8Array(WRITE)));
            const second = await rejection(vm.writeFile("/tmp/quota-existing.bin", new Uint8Array(WRITE + 16)));
            const missing = await rejection(vm.writeFile("/tmp/quota-missing.bin", new Uint8Array(WRITE)));
            const details = [first, second, missing].map((error) => error instanceof SidecarRejectedError ? error.detail : undefined);
            for (const detail of details) {
                // Only deliberately selected contract fields are logged. No
                // request identity, native message, VM or capability fields.
                console.info("native filesystem quota projection", {
                    code: detail?.code, limit_name: detail?.limit_name,
                    configured_limit: detail?.configured_limit, requested: detail?.requested,
                    current_usage: detail?.current_usage, unit: detail?.unit,
                    configuration_path: detail?.configuration_path, errno: detail?.errno,
                });
                check(() => expect(detail).toMatchObject({ code: "ERR_AGENTOS_RESOURCE_LIMIT",
                    limit_name: "limits.resources.maxFilesystemBytes", configured_limit: LIMIT,
                    current_usage: null, unit: "bytes", configuration_path: "limits.resources.maxFilesystemBytes", errno: "ENOSPC" }));
                check(() => expect(detail?.requested).toBeGreaterThan(LIMIT));
                check(() => expect(detail?.requested).toBeGreaterThanOrEqual(WRITE));
            }
            check(() => expect(details[1]!.requested! - details[0]!.requested!).toBe(16));
            check(() => expect(details[2]!.requested).toBe(details[0]!.requested! + sentinel.length));
            check(() => expect(details[0]?.requested).toBeGreaterThan(WRITE - sentinel.length));
            check(() => expect(first).toBeInstanceOf(SidecarRejectedError));
            check(() => expect(second).toBeInstanceOf(SidecarRejectedError));
            check(() => expect(missing).toBeInstanceOf(SidecarRejectedError));
            const stored = await vm.readFile("/tmp/quota-existing.bin");
            check(() => expect(Array.from(stored)).toEqual(Array.from(sentinel)));
            check(() => expect(stored.length).toBe(sentinel.length));
            const exists = await vm.exists("/tmp/quota-missing.bin");
            check(() => expect(exists).toBe(false));
            // A later host rejection observes the identical total: the
            // trusted failed writes did not change inode/byte ownership.
            const again = await rejection(vm.writeFile("/tmp/quota-existing.bin", new Uint8Array(WRITE)));
            check(() => expect(again instanceof SidecarRejectedError ? again.detail.requested : undefined).toBe(details[0]?.requested));
            // Same kernel policy must retain POSIX errno in untrusted guest fs.
            const program = String.raw`
                const fs = require('node:fs');
                const path = '/tmp/quota-guest.bin';
                let result;
                try { fs.writeFileSync(path, Buffer.alloc(1572864)); result = { rejected: false }; }
                catch (error) { result = { rejected: true, code: error.code }; }
                // POSIX open(O_CREAT/O_TRUNC) may mutate the leaf before a
                // later ENOSPC. This diagnostic fix imposes no atomic-write
                // guarantee on guest writeFileSync.
                if (fs.existsSync(path)) fs.unlinkSync(path);
                fs.writeFileSync('/tmp/quota-guest-recovery.bin', Buffer.from([29]));
                const progress = Array.from(fs.readFileSync('/tmp/quota-guest-recovery.bin'));
                fs.unlinkSync('/tmp/quota-guest-recovery.bin');
                console.log(JSON.stringify({ result, cleaned: !fs.existsSync(path), progress }));
            `;
            const guest = await vm.execArgv("node", ["-e", program]);
            check(() => expect(guest.exitCode).toBe(0));
            check(() => expect(JSON.parse(guest.stdout.trim())).toEqual({
                result: { rejected: true, code: "ENOSPC" }, cleaned: true, progress: [29],
            }));
            await vm.writeFile("/tmp/quota-recovery.bin", new Uint8Array([23]));
            const recovery = await vm.readFile("/tmp/quota-recovery.bin");
            check(() => expect(Array.from(recovery)).toEqual([23]));
        } catch (error) { failures.push(error); }
        finally {
            if (vm) { try { await vm.dispose(); } catch (error) { failures.push(error); } }
            if (sidecar) { try { await sidecar.dispose(); } catch (error) { failures.push(error); } }
        }
        if (failures.length) throw new AggregateError(failures, "native quota contract or owning cleanup failed");
    }, 30_000);
});
