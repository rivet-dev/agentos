// Explicit acceptance after compiling the probe with the owned sysroot.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {mkdtempSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=fileURLToPath(new URL('../',import.meta.url));
const dir=mkdtempSync(join(tmpdir(),'agentos-udp-connect-'));
function run(command,args){const result=spawnSync(command,args,{encoding:'utf8',timeout:10000,maxBuffer:65536});assert.equal(result.error,undefined);assert.equal(result.status,0,result.stderr);return result.stdout;}
try{
 const binary=join(dir,'native');
 run(process.env.CC??'cc',['-std=c11','-Wall','-Wextra','-Werror',join(root,'toolchain/test-programs/udp_connect_contract.c'),'-o',binary]);
 const expected=run(binary,[]);
 if(process.env.AGENTOS_UDP_PROBE_DIR){
  const {createKernel,createInMemoryFileSystem,createWasmVmRuntime,allowAll}=await import('../packages/core/dist/test-runtime.js');
  const kernel=createKernel({filesystem:createInMemoryFileSystem(),permissions:allowAll,loopbackExemptPorts:[]});
  try{
   await kernel.mount(createWasmVmRuntime({commandDirs:[process.env.AGENTOS_UDP_PROBE_DIR]}));
   let stdout='',stderr='';
   const child=kernel.spawn('udp_connect_contract',[],{timeout:10000,onStdout:bytes=>{stdout+=new TextDecoder().decode(bytes);},onStderr:bytes=>{stderr+=new TextDecoder().decode(bytes);}});
   child.closeStdin();assert.equal(await child.wait(),0,stderr);assert.equal(stdout,expected);
  }finally{await kernel.dispose();}
 }else console.log('WASM acceptance not run: set AGENTOS_UDP_PROBE_DIR to the rebuilt probe directory');
 console.log('UDP connect Linux contract passed');
}finally{rmSync(dir,{recursive:true,force:true});}
