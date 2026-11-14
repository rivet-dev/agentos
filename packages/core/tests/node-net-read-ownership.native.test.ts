import {createServer} from "node:net";
import {expect,test} from "vitest";
import {AgentOs} from "../src/index.js";

test("the socket read pump leaves the next bytes available when a synchronous reader pauses it",async()=>{
 const peers=new Set<import("node:net").Socket>();
 const server=createServer(socket=>{
  peers.add(socket);socket.on("error",()=>{});socket.on("close",()=>peers.delete(socket));
  socket.write(Buffer.from([65]));
  socket.once("data",()=>{
   const body=Buffer.alloc(256*1024);
   for(let i=0;i<body.length;i+=4)body.writeUInt32LE(i/4,i);
   socket.write(body);
  });
 });
 await new Promise<void>(resolve=>server.listen(0,"127.0.0.1",resolve));const address=server.address();
 if(!address||typeof address==="string")throw Error("missing fixture address");
 const sidecar=await AgentOs.createSidecar();let vm:AgentOs|undefined;
 try{
  vm=await AgentOs.create({sidecar:{kind:"explicit",handle:sidecar},defaultSoftware:false,permissions:{network:{default:"deny",rules:[{mode:"allow",patterns:[`tcp://127.0.0.1:${address.port}`]}]},hostFunction:"deny"},loopbackExemptPorts:[address.port]});
  let stdout="",stderr="";
  const child=await vm.process.spawn("node",["--input-type=module","-e",`
import assert from'node:assert/strict';import{connect}from'node:net';
const socket=connect(${address.port},'127.0.0.1');let first=true;
socket.on('data',data=>{if(!first)return;first=false;assert.deepEqual([...data],[65]);_netSocketWriteSyncRaw.applySync(undefined,[socket._socketId,Buffer.from('continue')]);
// Let the fixture fill TCP while this I/O callback still owns the guest stack.
Atomics.wait(new Int32Array(new SharedArrayBuffer(4)),0,0,50);
setImmediate(()=>{socket.pause();socket._applicationReadDemand=false;const data=_netSocketReadRaw.applySync(undefined,[socket._socketId,4]);const b=ArrayBuffer.isView(data)?Buffer.from(data.buffer,data.byteOffset,data.byteLength):typeof data==='string'?Buffer.from(data,'base64'):Buffer.from(data.base64,'base64');assert.equal(b.readUInt32LE(0),0,'read pump consumed bytes before yielding');socket.destroy();console.log('ordered');});});
`],{timeoutMs:5000,onStdout:b=>{stdout+=Buffer.from(b).toString();},onStderr:b=>{stderr+=Buffer.from(b).toString();}});
  expect((await vm.process.wait(child.pid)).exitCode,stderr).toBe(0);expect(stdout.trim()).toBe("ordered");
 }finally{try{await vm?.dispose();}finally{await sidecar.dispose();for(const peer of peers)peer.destroy();await new Promise<void>(resolve=>server.close(()=>resolve()));}}
},10000);
