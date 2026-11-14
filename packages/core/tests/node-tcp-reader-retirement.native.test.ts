import {spawnSync} from 'node:child_process';
import {createServer,type Socket} from 'node:net';
import {expect,test} from 'vitest';
import {AgentOs} from '../src/index.js';

test('closing paused native TCP readers retires their buffers before subsequent connections',async()=>{
 const peers=new Set<Socket>();
 const server=createServer(socket=>{peers.add(socket);socket.on('error',()=>{});socket.on('data',()=>{});socket.once('close',()=>peers.delete(socket));});
 await new Promise<void>(resolve=>server.listen(0,'127.0.0.1',resolve));
 const address=server.address();if(!address||typeof address==='string')throw Error('no host port');
 const source=`const net=require('node:net');(async()=>{for(let i=0;i<96;i++)await new Promise((resolve,reject)=>{const s=net.connect({host:'127.0.0.1',port:${address.port}});s.once('error',reject);s.once('connect',()=>{s.pause();s.destroy();});s.once('close',resolve);});console.log('retired 96');})().catch(e=>{console.error(e);process.exitCode=1;});`;
 let vm:AgentOs|undefined;let sidecar:Awaited<ReturnType<typeof AgentOs.createSidecar>>|undefined;
 try{
  const host=spawnSync(process.execPath,['-e',source],{encoding:'utf8',timeout:15000});expect(host.status,host.stderr).toBe(0);
  sidecar=await AgentOs.createSidecar();
  vm=await AgentOs.create({sidecar:{kind:'explicit',handle:sidecar},defaultSoftware:false,permissions:{network:{default:'deny',rules:[{mode:'allow',patterns:[`tcp://127.0.0.1:${address.port}`]}]}},loopbackExemptPorts:[address.port]});
  let stdout='',stderr='';const p=await vm.process.spawn('node',['-e',source],{timeoutMs:15000,onStdout:b=>stdout+=Buffer.from(b).toString(),onStderr:b=>stderr+=Buffer.from(b).toString()});
  expect((await vm.process.wait(p.pid)).exitCode,stderr).toBe(0);expect(stdout).toBe(host.stdout);
 }finally{
  await vm?.dispose();await sidecar?.dispose();for(const peer of peers)peer.destroy();
  await new Promise<void>(resolve=>server.close(()=>resolve()));
 }
},30000);
