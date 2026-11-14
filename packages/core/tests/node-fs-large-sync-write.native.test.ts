import {createHash} from 'node:crypto';
import {expect,test} from 'vitest';
import {AgentOs} from '../src/index.js';
test('a synchronous path write streams through bounded bridge requests with exact bytes',async()=>{
 const bytes=Buffer.alloc(20*1024*1024);for(let i=0;i<bytes.length;i++)bytes[i]=i%251;
 const digest=createHash('sha256').update(bytes).digest('hex');
 const sidecar=await AgentOs.createSidecar();let vm:AgentOs|undefined;
 try{
  vm=await AgentOs.create({sidecar:{kind:'explicit',handle:sidecar},defaultSoftware:false,permissions:{network:'deny'}});
  let stdout='',stderr='';const source=`const fs=require('node:fs'),{createHash}=require('node:crypto');const b=Buffer.alloc(20*1024*1024);for(let i=0;i<b.length;i++)b[i]=i%251;fs.writeFileSync('/tmp/large-sync.bin',b);try{fs.writeFileSync('/tmp/large-sync.bin',b,{flag:'wx'});throw Error('exclusive write accepted');}catch(e){if(e.code!=='EEXIST')throw e;}console.log(createHash('sha256').update(fs.readFileSync('/tmp/large-sync.bin')).digest('hex'));`;
  const p=await vm.process.spawn('node',['-e',source],{timeoutMs:25000,onStdout:b=>{stdout+=Buffer.from(b).toString();},onStderr:b=>{stderr+=Buffer.from(b).toString();}});
  expect((await vm.process.wait(p.pid)).exitCode,stderr).toBe(0);expect(stdout.trim()).toBe(digest);
 }finally{await vm?.dispose();await sidecar.dispose();}
},30000);
