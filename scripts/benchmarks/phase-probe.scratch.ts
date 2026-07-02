// Scratch phase attribution for unix_accept_latency + tcp_concurrent_4 residuals. NOT committed.
import { AgentOs } from "@rivet-dev/agentos-core";

const PROBE = `
const net = await import("node:net");
const os = await import("node:os");
const path = await import("node:path");
const now = () => Number(process.hrtime.bigint()) / 1e6;
const out = { unix: {}, tcpc: {} };

// --- unix accept phases (5 runs, keep per-phase p50-ish mid run) ---
const unixRuns = [];
for (let i = 0; i < 7; i++) {
  const sock = path.join(os.tmpdir(), "phase-unix-" + process.pid + "-" + i + ".sock");
  const t = { t0: now() };
  await new Promise((resolve, reject) => {
    const server = net.createServer((s) => { t.serverConn = now(); s.end(); });
    server.on("error", reject);
    server.listen(sock, () => {
      t.listening = now();
      const c = net.connect(sock);
      c.on("connect", () => { t.clientConnect = now(); });
      c.on("error", reject);
      c.on("close", () => {
        t.clientClose = now();
        server.close(() => { t.serverClosed = now(); resolve(); });
      });
      c.end();
    });
  });
  unixRuns.push({
    listen: +(t.listening - t.t0).toFixed(2),
    connect: +(t.clientConnect - t.listening).toFixed(2),
    accept: +((t.serverConn ?? t.clientClose) - t.listening).toFixed(2),
    close: +(t.clientClose - (t.serverConn ?? t.clientConnect)).toFixed(2),
    serverClose: +(t.serverClosed - t.clientClose).toFixed(2),
    total: +(t.serverClosed - t.t0).toFixed(2),
  });
}
out.unix.runs = unixRuns.slice(2);

// --- tcp concurrent 4 phases ---
const tcpcRuns = [];
for (let i = 0; i < 7; i++) {
  const t = { t0: now(), accepts: [], closes: [] };
  await new Promise((resolve, reject) => {
    let accepted = 0;
    const server = net.createServer((socket) => {
      t.accepts.push(+(now() - t.t0).toFixed(2));
      socket.on("data", () => socket.end());
      if (++accepted === 4) { /* wait for closes */ }
    });
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      t.listening = +(now() - t.t0).toFixed(2);
      let closed = 0;
      for (let k = 0; k < 4; k++) {
        const socket = net.connect(server.address().port, "127.0.0.1");
        socket.on("error", reject);
        socket.on("close", () => {
          t.closes.push(+(now() - t.t0).toFixed(2));
          if (++closed === 4) server.close(() => { t.done = +(now() - t.t0).toFixed(2); resolve(); });
        });
        socket.write("x");
      }
    });
  });
  tcpcRuns.push(t);
}
out.tcpc.runs = tcpcRuns.slice(2).map(r => ({ listening: r.listening, accepts: r.accepts, closes: r.closes, done: r.done }));
process.stdout.write(JSON.stringify(out));
`;

const vm = await AgentOs.create({});
const source = `(async () => { ${PROBE} })().catch(e => { console.error(e && e.stack || e); process.exit(1); });`;
await vm.writeFile("/tmp/phase-probe.mjs", source);
let stdout = "";
let stderr = "";
const proc = vm.spawn("node", ["/tmp/phase-probe.mjs"], {
  onStdout: (d: Uint8Array) => (stdout += Buffer.from(d).toString("utf8")),
  onStderr: (d: Uint8Array) => (stderr += Buffer.from(d).toString("utf8")),
});
const code = await vm.waitProcess(proc.pid);
console.log("guest exit", code, stdout.trim());
if (stderr.trim()) console.error("stderr:", stderr.slice(0, 600));
process.exit(0);
