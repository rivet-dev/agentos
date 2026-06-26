import type { BenchmarkOp } from "../lib/layers.js";

export const fsFamily: BenchmarkOp[] = [
	{
		family: "fs",
		name: "open_close_churn",
		nativeOp: "fs_open_close",
		fileLine: "crates/kernel/src/kernel.rs:1950",
		reproducer: "fs.openSync + fs.closeSync on a small fixture inside VM",
		program: `async () => {
  const fs = await import("node:fs");
  const path = "/tmp/fuzz-perf-open-close.txt";
  if (!fs.existsSync(path)) fs.writeFileSync(path, "hi");
  const fd = fs.openSync(path, "r");
  fs.closeSync(fd);
}`,
	},
	{
		family: "fs",
		name: "stat_storm",
		nativeOp: "fs_stat",
		fileLine: "crates/kernel/src/kernel.rs:1950",
		reproducer: "node fs.statSync('/tmp/fuzz-perf-stat.txt') inside VM",
		program: `async (i) => {
  const fs = await import("node:fs");
  const path = "/tmp/fuzz-perf-stat.txt";
  if (!fs.existsSync(path)) fs.writeFileSync(path, "hi");
  fs.statSync(path);
}`,
	},
	{
		family: "fs",
		name: "small_write",
		nativeOp: "fs_write",
		fileLine: "crates/kernel/src/kernel.rs:1930",
		reproducer: "node fs.writeFileSync('/tmp/fuzz-perf-write.txt', payload)",
		program: `async (i) => {
  const fs = await import("node:fs");
  fs.writeFileSync("/tmp/fuzz-perf-write.txt", "hello-" + i);
}`,
	},
	{
		family: "fs",
		name: "big_read",
		nativeOp: "fs_read",
		fileLine: "crates/kernel/src/mount_table.rs:814",
		reproducer: "node fs.readFileSync('/tmp/fuzz-perf-read.bin')",
		program: `async () => {
  const fs = await import("node:fs");
  const path = "/tmp/fuzz-perf-read.bin";
  if (!fs.existsSync(path)) fs.writeFileSync(path, Buffer.alloc(64 * 1024, 7));
  const data = fs.readFileSync(path);
  if (data.length !== 64 * 1024) throw new Error("bad read");
}`,
	},
	{
		family: "fs",
		name: "mkdir_rmdir",
		nativeOp: "fs_mkdir_rmdir",
		fileLine: "crates/kernel/src/mount_table.rs:814",
		reproducer: "fs.mkdirSync + fs.rmdirSync on a fresh VM path",
		program: `async (i) => {
  const fs = await import("node:fs");
  const path = "/tmp/fuzz-perf-dir-" + i + "-" + process.pid;
  fs.mkdirSync(path);
  fs.rmSync(path, { recursive: true });
}`,
	},
	{
		family: "fs",
		name: "rename_file",
		nativeOp: "fs_rename",
		fileLine: "crates/kernel/src/mount_table.rs:814",
		reproducer: "write one file, rename it, then unlink",
		program: `async (i) => {
  const fs = await import("node:fs");
  const from = "/tmp/fuzz-perf-rename-" + i + ".a";
  const to = "/tmp/fuzz-perf-rename-" + i + ".b";
  fs.writeFileSync(from, "hi");
  fs.renameSync(from, to);
  fs.unlinkSync(to);
}`,
	},
	{
		family: "fs",
		name: "readdir_large",
		nativeOp: "fs_readdir",
		fileLine: "crates/kernel/src/mount_table.rs:814",
		reproducer: "readdirSync over a 32-entry VM directory",
		setup: `async () => {
  const fs = await import("node:fs");
  const dir = "/tmp/fuzz-perf-readdir";
  if (!fs.existsSync(dir)) fs.mkdirSync(dir);
  for (let i = 0; i < 32; i++) {
    const path = dir + "/" + i + ".txt";
    if (!fs.existsSync(path)) fs.writeFileSync(path, "hi");
  }
}`,
		program: `async () => {
  const fs = await import("node:fs");
  const dir = "/tmp/fuzz-perf-readdir";
  const entries = fs.readdirSync(dir);
  if (entries.length < 32) throw new Error("short readdir");
}`,
	},
	{
		family: "fs",
		name: "fsync_small",
		nativeOp: "fs_fsync",
		fileLine: "crates/kernel/src/kernel.rs:1930",
		reproducer: "fs.writeSync then fs.fsyncSync on a small file",
		program: `async () => {
  const fs = await import("node:fs");
  const fd = fs.openSync("/tmp/fuzz-perf-fsync.txt", "w");
  fs.writeSync(fd, "hello");
  fs.fsyncSync(fd);
  fs.closeSync(fd);
}`,
	},
];
