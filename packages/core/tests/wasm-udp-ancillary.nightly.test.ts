import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { AgentOs, createSnapshotExport } from "../src/index.js";

const root = fileURLToPath(new URL("../../../", import.meta.url));
const compiler = resolve(root, "toolchain/c/scripts/wasi-clang.sh");
const sysroot = resolve(root, "toolchain/c/sysroot");
const fixture = resolve(root, "toolchain/test-programs/udp_receive_ancillary_contract.c");
const hasToolchain = existsSync(resolve(root, "toolchain/c/vendor/wasi-sdk/bin/clang")) &&
    existsSync(join(sysroot, "lib/wasm32-wasi/libc.a"));
if (process.env.AGENTOS_REQUIRE_UDP_ANCILLARY === "1" && !hasToolchain) {
    throw new Error("UDP ancillary acceptance requires the owned WASM compiler and rebuilt sysroot");
}

// Included by test:nightly's tests/*.nightly.test.ts discovery. The required
// nightly CI and final acceptance set AGENTOS_REQUIRE_UDP_ANCILLARY=1 so missing assets cannot skip.
describe.skipIf(!hasToolchain)("WASM UDP receive ancillary Linux contract", () => {
    let directory: string;
    let image: Buffer;
    let expected: string;
    beforeAll(() => {
        directory = mkdtempSync(join(tmpdir(), "agentos-udp-ancillary-"));
        const wasm = join(directory, "probe.wasm");
        const native = join(directory, "probe-native");
        for (const [command, args] of [
            [compiler, ["--target=wasm32-wasip1", `--sysroot=${sysroot}`, "-O2", fixture, "-o", wasm]],
            [process.env.CC ?? "cc", ["-O2", "-Wall", "-Wextra", "-Werror", fixture, "-o", native]],
        ] as const) {
            const built = spawnSync(command, args, {encoding: "utf8", timeout: 30_000, maxBuffer: 64 * 1024});
            expect(built.status, built.error?.message ?? built.stderr).toBe(0);
        }
        const reference = spawnSync(native, [], {encoding: "utf8", timeout: 5_000, maxBuffer: 64 * 1024});
        expect(reference.status, reference.error?.message ?? reference.stderr).toBe(0);
        expect(reference.stdout).not.toContain("=no");
        expected = reference.stdout;
        image = readFileSync(wasm);
    }, 60_000);
    afterAll(() => {
        if (directory) rmSync(directory, {recursive: true, force: true});
    });

    test("matches full-value IPv4/IPv6/mapped metadata, aliases and bounds with egress denied", async () => {
        const vm = await AgentOs.create({
            defaultSoftware: false,
            limits: {wasm: {capturedOutputLimitBytes: 64 * 1024}},
            permissions: {fs: "allow", childProcess: "allow", process: "allow", network: "deny"},
            rootFilesystem: {lowers: [createSnapshotExport([
                {path: "/bin", type: "directory", mode: "0755", uid: 0, gid: 0},
                {path: "/bin/udp-ancillary", type: "file", mode: "0755", uid: 0, gid: 0, content: image.toString("base64"), encoding: "base64"},
            ])]},
        });
        const failures: unknown[] = [];
        try {
            const result = await vm.process.execFile("/bin/udp-ancillary", ["--guest-identity"], {
                timeoutMs: 5_000, output: {capture: "all"},
            });
            assert(typeof result.stdout === "string", "stdout capture is missing");
            assert(typeof result.stderr === "string", "stderr capture is missing");
            expect(result.exitCode, result.stderr + result.stdout).toBe(0);
            expect(result.error).toBeUndefined();
            expect(result.stderr).toBe("");
            expect(result.stdout).toContain("udp_ancillary_guest_message_abi=yes");
            expect(result.stdout).toContain("udp_ancillary_guest_interface_identity=yes");
            expect(result.stdout).toContain("udp_ancillary_guest_interface_bound=yes");
            const common = result.stdout.split("\n").filter(line => !line.startsWith("udp_ancillary_guest_")).join("\n");
            expect(common).toBe(expected);
        } catch (error) { failures.push(error); }
        finally {
            try { await vm.dispose(); } catch (error) { failures.push(error); }
        }
        if (failures.length) throw new AggregateError(failures, "UDP ancillary contract or owned VM cleanup failed");
    }, 20_000);
});
