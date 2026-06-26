import type { BenchmarkOp } from "../lib/layers.js";

export const pipesFamily: BenchmarkOp[] = [
	{
		family: "pipes",
		name: "pass_through_small",
		nativeOp: "pipe_echo",
		fileLine: "crates/v8-runtime/src/host_call.rs:276",
		reproducer: "node PassThrough write/read small payload inside VM",
		program: `async () => {
  const { PassThrough } = await import("node:stream");
  await new Promise((resolve, reject) => {
    const stream = new PassThrough();
    let out = "";
    stream.on("data", (d) => out += d.toString("utf8"));
    stream.on("end", () => out === "hello" ? resolve() : reject(new Error(out)));
    stream.end("hello");
  });
}`,
	},
	{
		family: "pipes",
		name: "throughput_64k",
		nativeOp: "pipe_throughput",
		fileLine: "crates/v8-runtime/src/host_call.rs:276",
		reproducer: "node PassThrough write/read one 64KiB payload inside VM",
		program: `async () => {
  const { PassThrough } = await import("node:stream");
  const payload = Buffer.alloc(64 * 1024, 9);
  await new Promise((resolve, reject) => {
    const stream = new PassThrough();
    const chunks = [];
    stream.on("data", (d) => chunks.push(d));
    stream.on("end", () => {
      const got = Buffer.concat(chunks);
      got.length === payload.length ? resolve() : reject(new Error("short pipe"));
    });
    stream.end(payload);
  });
}`,
	},
	{
		family: "pipes",
		name: "backpressure_chunks",
		nativeOp: "pipe_backpressure",
		fileLine: "crates/v8-runtime/src/host_call.rs:276",
		reproducer: "node PassThrough with a tiny highWaterMark and 64 one-byte writes",
		program: `async () => {
  const { PassThrough } = await import("node:stream");
  await new Promise((resolve, reject) => {
    const stream = new PassThrough({ highWaterMark: 1 });
    let bytes = 0;
    stream.on("data", (d) => bytes += d.length);
    stream.on("end", () => bytes === 64 ? resolve() : reject(new Error(String(bytes))));
    for (let i = 0; i < 64; i++) stream.write(Buffer.from([i & 255]));
    stream.end();
  });
}`,
	},
];
