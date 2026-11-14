import {spawnSync} from 'node:child_process';
import {expect,test} from 'vitest';
import {AgentOs} from '../src/index.js';

test('filesystem readable streams use canonical stream identity, decoding and backpressure',async()=>{
 const source=`const fs=require('node:fs'),{Readable,Writable}=require('node:stream'),{pipeline}=require('node:stream/promises');
 (async()=>{const directory=fs.mkdtempSync('/tmp/readable-identity-'),file=directory+'/input';fs.writeFileSync(file,'αβγ-abcdef');
 try{const reader=fs.createReadStream(file,{highWaterMark:3,encoding:'utf8'}),parts=[];
 const identity=[reader instanceof Readable,reader instanceof fs.ReadStream,reader.constructor===fs.ReadStream];
 const sink=new Writable({highWaterMark:1,write(piece,encoding,callback){parts.push(Buffer.from(piece));setTimeout(callback,1);}});
 await pipeline(reader,sink);
 const handle=await fs.promises.open(file,'r'),ranged=handle.createReadStream({start:6,end:9,autoClose:false,highWaterMark:2});let bytes=[];for await(const piece of ranged)bytes.push(...piece);
 const retained=handle.fd>=0;await handle.close();
 console.log(JSON.stringify({identity,text:Buffer.concat(parts).toString(),closed:reader.closed,handleIdentity:ranged instanceof Readable,bytes,retained}));
 }finally{fs.rmSync(directory,{recursive:true,force:true});}})();`;
 const host=spawnSync(process.execPath,['-e',source],{encoding:'utf8',timeout:5000,maxBuffer:16384});expect(host.status,host.stderr).toBe(0);
 const sidecar=await AgentOs.createSidecar();let vm:AgentOs|undefined;
 try{vm=await AgentOs.create({sidecar:{kind:'explicit',handle:sidecar},defaultSoftware:false,permissions:{network:'deny'}});let stdout='',stderr='';const child=await vm.process.spawn('node',['-e',source],{timeoutMs:5000,onStdout:b=>stdout+=Buffer.from(b).toString(),onStderr:b=>stderr+=Buffer.from(b).toString()});expect((await vm.process.wait(child.pid)).exitCode,stderr).toBe(0);expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));}
 finally{await vm?.dispose();await sidecar.dispose();}
},10000);
