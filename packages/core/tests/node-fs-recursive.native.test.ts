import {expect,test} from 'vitest';
import {AgentOs} from '../src/index.js';
async function run(limits:Record<string,number>={},denied=false){
 const sidecar=await AgentOs.createSidecar();let vm:AgentOs|undefined;
 try{
  vm=await AgentOs.create({sidecar:{kind:'explicit',handle:sidecar},defaultSoftware:false,limits:{resources:limits},permissions:{network:'deny'}});
  await vm.filesystem.writeFile('/tmp/tree/top','x');await vm.filesystem.writeFile('/tmp/tree/a/b/deep','y');
  if(denied)await vm.configurePermissions({network:'deny',fs:{default:'allow',rules:[{mode:'deny',paths:['/tmp/tree/a'],operations:['readdir']}]}});
  let stdout='',stderr='';const source=`const fs=require('node:fs');(async()=>{const opts={recursive:true,withFileTypes:true};const rows=e=>e.filter(x=>x.isFile()).map(x=>x.parentPath.replace('/tmp/tree','')+'/'+x.name).sort();try{console.log(JSON.stringify({sync:rows(fs.readdirSync('/tmp/tree',opts)),async:rows(await fs.promises.readdir('/tmp/tree',opts)),names:fs.readdirSync('/tmp/tree',{recursive:true}).sort()}));}catch(e){console.log(JSON.stringify({code:e.code,message:e.message}));}})();`;
  const p=await vm.process.spawn('node',['-e',source],{timeoutMs:10000,onStdout:b=>stdout+=Buffer.from(b).toString(),onStderr:b=>stderr+=Buffer.from(b).toString()});
  expect((await vm.process.wait(p.pid)).exitCode,stderr).toBe(0);return JSON.parse(stdout);
 }finally{await vm?.dispose();await sidecar.dispose();}
}
test('recursive directory reads include nested names and correct Dirent parents in sync and promise APIs',async()=>{
 expect(await run()).toEqual({sync:['/a/b/deep','/top'],async:['/a/b/deep','/top'],names:['a','a/b','a/b/deep','top']});
},15000);
test('recursive directory reads retain the configured traversal quota',async()=>{
 expect((await run({maxRecursiveFsEntries:2})).message).toMatch(/maxRecursiveFsEntries|recursive.*entries|limit/i);
},15000);
test('recursive directory reads enforce descendant read denial',async()=>{
 expect((await run({},true)).code).toMatch(/EACCES|EPERM/);
},15000);
