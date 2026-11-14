import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

for (const method of ["GET", "CONNECT"]) {
 test("a "+method+" protocol handoff survives a delayed header-write callback",async()=>{
  const peers=new Set<import("node:net").Socket>();
  const server=createServer();
  server.on("connection",socket=>{peers.add(socket);socket.on("error",()=>{});socket.once("close",()=>peers.delete(socket));});
  const accept=(socket:import("node:stream").Duplex,status:string)=>{socket.write(status+"\r\n\r\n");socket.on("data",bytes=>socket.write(bytes));};
  server.on("upgrade",(_req,socket)=>accept(socket,"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: fixture"));
  server.on("connect",(_req,socket)=>accept(socket,"HTTP/1.1 200 Connection Established"));
  await new Promise<void>(resolve=>server.listen(0,"127.0.0.1",resolve));
  const address=server.address();if(!address||typeof address==="string")throw Error("No fixture address");
  const source=[
   "import http from'node:http';import net from'node:net';import{setTimeout as delay}from'node:timers/promises';",
   "const errors=[];let requestClosed=false;const agent=new http.Agent({keepAlive:false});",
   "agent.createConnection=()=>{const socket=net.connect(PORT,'127.0.0.1');const original=socket.write;let first=true;socket.write=function(chunk,encoding,callback){if(typeof encoding==='function'){callback=encoding;encoding=undefined;}if(first&&typeof callback==='function'){first=false;const finish=callback;callback=error=>setTimeout(()=>finish(error),80);}return original.call(this,chunk,encoding,callback);};return socket;};",
   "const method=METHOD;const request=http.request({host:'127.0.0.1',port:PORT,method,agent,headers:method==='GET'?{Connection:'Upgrade',Upgrade:'fixture'}:{}});request.on('error',e=>errors.push(e.message));request.once('close',()=>{requestClosed=true;});let socket;",
   "try{await new Promise((resolve,reject)=>{request.once(method==='GET'?'upgrade':'connect',(_res,s)=>{socket=s;resolve();});request.once('error',reject);request.end();});await delay(150);const response=await Promise.race([new Promise(resolve=>{socket.once('data',b=>resolve(b.toString()));socket.write('ping');}),delay(1000).then(()=>'<timeout>')]);console.log(JSON.stringify({errors,response,requestClosed,requestDestroyed:request.destroyed,socketAlive:!socket.destroyed}));}",
   "finally{socket?.destroy();agent.destroy();}"
  ].join("\n").replaceAll("PORT",String(address.port)).replace("METHOD",JSON.stringify(method));
  let vm:AgentOs|undefined;const sidecar=await AgentOs.createSidecar();
  try{
   const host=await new Promise<{code:number|null;stdout:string;stderr:string}>((resolve,reject)=>{
    const child=spawn(process.execPath,["--input-type=module","-e",source],{stdio:["ignore","pipe","pipe"]});
    let stdout="",stderr="";child.stdout.on("data",bytes=>{stdout+=bytes;});child.stderr.on("data",bytes=>{stderr+=bytes;});
    const timer=setTimeout(()=>child.kill("SIGKILL"),5000);
    child.once("error",error=>{clearTimeout(timer);reject(error);});child.once("exit",code=>{clearTimeout(timer);resolve({code,stdout,stderr});});
   });
   expect(host.code,host.stderr).toBe(0);
   const expected={errors:[],response:"ping",requestClosed:true,requestDestroyed:true,socketAlive:true};
   expect(JSON.parse(host.stdout)).toEqual(expected);
   vm=await AgentOs.create({sidecar:{kind:"explicit",handle:sidecar},defaultSoftware:false,permissions:{network:{default:"deny",rules:[{mode:"allow",patterns:["tcp://127.0.0.1:"+address.port]}]},hostFunction:"deny"},loopbackExemptPorts:[address.port]});
   let stdout="",stderr="";
   const child=await vm.process.spawn("node",["--input-type=module","-e",source],{timeoutMs:5000,onStdout:bytes=>{stdout+=Buffer.from(bytes).toString();},onStderr:bytes=>{stderr+=Buffer.from(bytes).toString();}});
   expect((await vm.process.wait(child.pid)).exitCode,stderr).toBe(0);expect(JSON.parse(stdout)).toEqual(expected);
  }finally{await vm?.dispose();await sidecar.dispose();for(const socket of peers)socket.destroy();await new Promise<void>(resolve=>server.close(()=>resolve()));}
 },15000);
}
