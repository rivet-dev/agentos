import {spawnSync} from 'node:child_process';
import {expect,test} from 'vitest';
import {AgentOs} from '../src/index.js';
async function compare(source:string){
 const host=spawnSync(process.execPath,['-e',source],{encoding:'utf8',timeout:15000});expect(host.status,host.stderr).toBe(0);
 const sidecar=await AgentOs.createSidecar();let vm:AgentOs|undefined;
 try{vm=await AgentOs.create({sidecar:{kind:'explicit',handle:sidecar},defaultSoftware:false,permissions:{network:'deny'},limits:{jsRuntime:{v8SerializationLimitBytes:64*1024*1024}}});let stdout='',stderr='';const p=await vm.process.spawn('node',['-e',source],{timeoutMs:15000,onStdout:b=>stdout+=Buffer.from(b).toString(),onStderr:b=>stderr+=Buffer.from(b).toString()});expect((await vm.process.wait(p.pid)).exitCode,stderr).toBe(0);expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));}
 finally{await vm?.dispose();await sidecar.dispose();}
}
test('local V8 serialization transfers twenty MiB inside the guest without increasing RPC frames',async()=>{
 await compare(`const{serialize,deserialize}=require('node:v8'),{createHash}=require('node:crypto');const value=Buffer.alloc(20*1024*1024,17),wire=serialize(value),copy=deserialize(wire);console.log(JSON.stringify({length:copy.length,digest:createHash('sha256').update(copy).digest('hex'),buffer:Buffer.isBuffer(copy)}));`);
},20000);
