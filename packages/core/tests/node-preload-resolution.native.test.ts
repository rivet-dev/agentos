import {expect,test} from 'vitest';
import {AgentOs} from '../src/index.js';
import {spawnSync} from 'node:child_process';
async function guest(source:string,preload=false){
 const sidecar=await AgentOs.createSidecar();let vm:AgentOs|undefined;
 try{
  vm=await AgentOs.create({sidecar:{kind:'explicit',handle:sidecar},defaultSoftware:false,permissions:{network:'deny'},...(preload?{environment:{NODE_OPTIONS:'--require "/tmp/preload one.cjs"'}}:{})});
  if(preload)await vm.filesystem.writeFile('/tmp/preload one.cjs','globalThis.preloadValue=42;');
  let stdout='',stderr='';const p=await vm.process.spawn('node',['-e',source],{timeoutMs:10000,onStdout:b=>stdout+=Buffer.from(b).toString(),onStderr:b=>stderr+=Buffer.from(b).toString()});expect((await vm.process.wait(p.pid)).exitCode,stderr).toBe(0);return stdout;
 }finally{await vm?.dispose();await sidecar.dispose();}
}
test('require and require.resolve consult an installed Module resolution hook',async()=>{
 const source=`const M=require('node:module');const original=M._resolveFilename;M._resolveFilename=function(request,...args){if(request==='synthetic-hook-target')throw Object.assign(Error('hook rejection'),{code:'HOOK_REJECT'});return Reflect.apply(original,this,[request,...args]);};const out=[];for(const fn of [()=>require('synthetic-hook-target'),()=>require.resolve('synthetic-hook-target')]){try{fn();}catch(e){out.push(e.code);}}console.log(JSON.stringify(out));`;
 const host=spawnSync(process.execPath,['-e',source],{encoding:'utf8',timeout:10000});expect(host.status,host.stderr).toBe(0);expect(await guest(source)).toBe(host.stdout);
},15000);
test('NODE_OPTIONS runs a quoted CommonJS preload before the entry module',async()=>{
 expect(await guest('console.log(globalThis.preloadValue)',true)).toBe('42\n');
},15000);
