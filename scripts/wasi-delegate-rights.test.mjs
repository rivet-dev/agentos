import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, statSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { runInNewContext } from "node:vm";

const source = readFileSync(
  new URL("../crates/executor-v8-runtime/assets/runners/wasi-module.js", import.meta.url),
  "utf8",
);
const requireBuiltin = createRequire(import.meta.url);

function createWasi({ managed = false, host = {}, preopens = {} } = {}) {
  const context = {
    Buffer, WebAssembly, process: { env: {} },
    __agentOSManagedKernelHost: managed,
    __agentOSWasiHost: { requireBuiltin, syncReadLimitBytes: 1024, ...host },
  };
  runInNewContext(source, context);
  const instance = new context.__agentOSWasiModule.WASI({ preopens });
  const memory = new WebAssembly.Memory({ initial: 1 });
  instance.instance = { exports: { memory } };
  return { api: instance.wasiImport, data: new DataView(memory.buffer), bytes: Buffer.from(memory.buffer) };
}

test("managed stdin uses the kernel for both fd 0 and its alias, preserving EOF and errno", () => {
  const calls = [];
  const results = [{ dataBase64: "QQ==" }, { done: true }, Object.assign(new Error("closed"), { code: "EBADF" })];
  const { api, data, bytes } = createWasi({
    managed: true,
    host: {
      lookupFdHandle: fd => fd === 7 ? { kind: "stdio", targetFd: 0 } : null,
      syncRpc: {
        callSync(name) {
          calls.push(name);
          const result = results.shift();
          if (result instanceof Error) throw result;
          return result;
        },
      },
    },
  });
  data.setUint32(0, 32, true);
  data.setUint32(4, 1, true);
  assert.equal(api.fd_read(0, 0, 1, 16), 0);
  assert.equal(data.getUint32(16, true), 1);
  assert.equal(bytes[32], 65);
  assert.equal(api.fd_read(7, 0, 1, 16), 0);
  assert.equal(data.getUint32(16, true), 0);
  assert.equal(api.fd_read(0, 0, 1, 16), 8);
  assert.deepEqual(calls, Array(3).fill("__kernel_stdin_read"));
});

test("a limited directory grants individual directory, create, link and file rights", t => {
  const root = mkdtempSync(join(tmpdir(), "agentos-wasi-rights-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const read = 1n << 1n;
  const write = 1n << 6n;
  const rights = write | (1n << 9n) | (1n << 10n) | (1n << 11n) | (1n << 12n) | (1n << 13n);
  const { api, data, bytes } = createWasi({
    preopens: { "/fixture": { hostPath: root, rightsBase: rights, rightsInheriting: read | write } },
  });
  bytes.write("dir", 32);
  assert.equal(api.path_create_directory(3, 32, 3), 0);
  assert.ok(statSync(join(root, "dir")).isDirectory());
  assert.equal(api.path_remove_directory(3, 32, 3), 76);
  bytes.write("a", 32);
  assert.equal(api.path_open(3, 0, 32, 1, 1, write, 0n, 0, 16), 0);
  const fd = data.getUint32(16, true);
  try {
    data.setUint32(0, 40, true);
    data.setUint32(4, 2, true);
    bytes.write("ok", 40);
    assert.equal(api.fd_write(fd, 0, 1, 24), 0);
    assert.equal(data.getUint32(24, true), 2);
    assert.equal(api.fd_read(fd, 0, 1, 24), 76);
  } finally {
    assert.equal(api.fd_close(fd), 0);
  }
  bytes.write("b", 36);
  assert.equal(api.path_link(3, 0, 32, 1, 3, 36, 1), 0);
  assert.equal(readFileSync(join(root, "b"), "utf8"), "ok");
  assert.equal(api.path_unlink_file(3, 36, 1), 76);
});

test("Preview 1 link source and target work with separately attenuated directory rights", t => {
  const root = mkdtempSync(join(tmpdir(), "agentos-wasi-link-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const { api, data, bytes } = createWasi({
    preopens: {
      "/source": { hostPath: root, rightsBase: (1n << 6n) | (1n << 10n) | (1n << 13n), rightsInheriting: 1n << 6n },
      "/link-source": { hostPath: root, rightsBase: 1n << 11n },
      "/link-target": { hostPath: root, rightsBase: 1n << 12n },
    },
  });
  bytes.write("old", 32);
  bytes.write("new", 40);
  assert.equal(api.path_open(3, 0, 32, 3, 1, 1n << 6n, 0n, 0, 16), 0);
  assert.equal(api.fd_close(data.getUint32(16, true)), 0);
  assert.equal(api.path_link(4, 0, 32, 3, 5, 40, 3), 0);
  assert.equal(statSync(join(root, "old")).ino, statSync(join(root, "new")).ino);
});
