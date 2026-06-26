import type { BenchmarkOp } from "../lib/layers.js";

export const netFamily: BenchmarkOp[] = [
	{
		family: "net",
		name: "tcp_connect_close",
		nativeOp: "tcp_connect",
		fileLine: "crates/kernel/src/socket_table.rs:382",
		reproducer: "node net.createServer(); net.connect(port).end() inside VM",
		program: `async () => {
  const net = await import("node:net");
  await new Promise((resolve, reject) => {
    const server = net.createServer((socket) => socket.end());
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const port = server.address().port;
      const socket = net.connect(port, "127.0.0.1");
      socket.on("error", reject);
      socket.on("close", () => server.close(resolve));
      socket.end();
    });
  });
}`,
	},
	{
		family: "net",
		name: "tcp_echo",
		nativeOp: "tcp_echo",
		fileLine: "crates/kernel/src/socket_table.rs:1413",
		reproducer: "localhost TCP echo one small payload inside VM",
		program: `async () => {
  const net = await import("node:net");
  await new Promise((resolve, reject) => {
    const server = net.createServer((socket) => socket.on("data", (d) => socket.end(d)));
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const port = server.address().port;
      const socket = net.connect(port, "127.0.0.1");
      let data = "";
      socket.on("data", (d) => data += d.toString("utf8"));
      socket.on("error", reject);
      socket.on("close", () => {
        server.close(() => data === "hello" ? resolve() : reject(new Error(data)));
      });
      socket.write("hello");
    });
  });
}`,
	},
	{
		family: "net",
		name: "tcp_concurrent_4",
		nativeOp: "tcp_concurrent",
		fileLine: "crates/kernel/src/socket_table.rs:382",
		reproducer: "four concurrent localhost TCP clients connect to one VM server",
		program: `async () => {
  const net = await import("node:net");
  await new Promise((resolve, reject) => {
    let accepted = 0;
    const server = net.createServer((socket) => {
      socket.on("data", () => socket.end());
      if (++accepted === 4) server.close(resolve);
    });
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const port = server.address().port;
      for (let i = 0; i < 4; i++) {
        const socket = net.connect(port, "127.0.0.1");
        socket.on("error", reject);
        socket.write("x");
      }
    });
  });
}`,
	},
	{
		family: "net",
		name: "tcp_throughput_64k",
		nativeOp: "tcp_throughput",
		fileLine: "crates/kernel/src/socket_table.rs:1413",
		reproducer: "localhost TCP echo of one 64KiB payload inside VM",
		program: `async () => {
  const net = await import("node:net");
  const payload = Buffer.alloc(64 * 1024, 7);
  await new Promise((resolve, reject) => {
    const server = net.createServer((socket) => socket.on("data", (d) => socket.end(d)));
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const port = server.address().port;
      const socket = net.connect(port, "127.0.0.1");
      const chunks = [];
      socket.on("data", (d) => chunks.push(d));
      socket.on("error", reject);
      socket.on("close", () => {
        const got = Buffer.concat(chunks);
        server.close(() => got.length === payload.length ? resolve() : reject(new Error("short echo")));
      });
      socket.write(payload);
    });
  });
}`,
	},
	{
		family: "net",
		name: "tcp_tiny_writes_16",
		nativeOp: "tcp_tiny_writes",
		fileLine: "crates/kernel/src/socket_table.rs:1335",
		reproducer: "localhost TCP echo using sixteen one-byte writes inside VM",
		program: `async () => {
  const net = await import("node:net");
  await new Promise((resolve, reject) => {
    const server = net.createServer((socket) => {
      const chunks = [];
      socket.on("data", (d) => {
        chunks.push(d);
        if (Buffer.concat(chunks).length >= 16) socket.end(Buffer.concat(chunks));
      });
    });
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const port = server.address().port;
      const socket = net.connect(port, "127.0.0.1");
      const chunks = [];
      socket.on("data", (d) => chunks.push(d));
      socket.on("error", reject);
      socket.on("close", () => {
        const got = Buffer.concat(chunks);
        server.close(() => got.length === 16 ? resolve() : reject(new Error("short tiny echo")));
      });
      for (let i = 0; i < 16; i++) socket.write("x");
    });
  });
}`,
	},
];
