import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

function serverProgram(depth: number): string {
  let source = `
const server=require('node:http').createServer((request,response)=>response.end('guest-listener'));
server.listen(0,'127.0.0.1',()=>{
 console.log(JSON.stringify({port:server.address().port}));
 setTimeout(()=>server.close(),8000);
});`;
  for (let level = 0; level < depth; level++) source = `
const child=require('node:child_process').spawn('node',['-e',${JSON.stringify(source)}],{stdio:['ignore','pipe','pipe']});
child.stdout.on('data',chunk=>process.stdout.write(chunk));
child.stderr.on('data',chunk=>process.stderr.write(chunk));
child.on('error',error=>{console.error(error.stack);process.exitCode=1;});`;
  return source;
}

for (const depth of [0, 1, 2]) test(`VM fetch reaches a kernel HTTP listener at child depth ${depth}`, async () => {
  const sidecar = await AgentOs.createSidecar();
  let vm: AgentOs | undefined;
  let streamId: string | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const failures: unknown[] = [];
  try {
    vm = await AgentOs.create({sidecar:{kind:"explicit",handle:sidecar},defaultSoftware:false,permissions:{network:"deny",childProcess:"allow"}});
    let output = "", stderr = "";
    let captureFailure: Error | undefined;
    let resolveReady!: (port: number) => void;
    let rejectReady!: (error: Error) => void;
    const ready = new Promise<number>((resolve, reject) => {resolveReady=resolve;rejectReady=reject;});
    void ready.catch(() => {});
    await vm.process.spawn("node", ["-e", serverProgram(depth)], {
      timeoutMs: 10000,
      onStdout(chunk) {
        if (captureFailure) return;
        if (Buffer.byteLength(output) + chunk.byteLength > 65536) {
          captureFailure = new Error("readiness output exceeded 64KiB");rejectReady(captureFailure);return;
        }
        output += Buffer.from(chunk).toString();
        for (const line of output.split("\n")) {
          try {
            const value = JSON.parse(line);
            if (Number.isInteger(value.port) && value.port > 0 && value.port <= 65535) resolveReady(value.port);
          } catch { /* A partial readiness line is completed by the next chunk. */ }
        }
      },
      onStderr(chunk) {
        if (captureFailure) return;
        if (Buffer.byteLength(stderr) + chunk.byteLength > 65536) {
          captureFailure = new Error("listener stderr exceeded 64KiB");rejectReady(captureFailure);return;
        }
        stderr += Buffer.from(chunk).toString();
      },
    });
    timer = setTimeout(() => rejectReady(new Error("guest listener readiness exceeded 5s")), 5000);
    const port = await ready;
    clearTimeout(timer);
    const request = () => new Request("http://guest.gameinc.io/");
    const response = await vm.fetch(port, request());
    expect(response.status, stderr).toBe(200);
    expect(await response.text()).toBe("guest-listener");
    const head = await vm.fetchStreamStart(port, request());
    streamId = head.streamId;
    expect(head.status).toBe(200);
    let body = "", done = false;
    for (let read = 0; read < 16 && !done; read++) {
      const chunk = await vm.fetchStreamRead(streamId, 4);
      body += Buffer.from(chunk.body).toString();
      done = chunk.done;
    }
    expect(done).toBe(true);
    expect(body).toBe("guest-listener");
    if (captureFailure) throw captureFailure;
    streamId = undefined;
  } catch (error) {failures.push(error);}
  finally {
    clearTimeout(timer);
    if (streamId && vm) try {await vm.fetchStreamCancel(streamId);} catch (error) {failures.push(error);}
    for (const cleanup of [() => vm?.dispose(), () => sidecar.dispose()]) try {await cleanup();} catch (error) {failures.push(error);}
  }
  if (failures.length) throw new AggregateError(failures, failures.map(error => String(error instanceof Error ? error.stack ?? error.message : error)).join("\n\n"));
}, 20000);
