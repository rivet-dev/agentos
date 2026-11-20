// Explicit native/WASM acceptance: build the probe with the owned sysroot first.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {mkdtempSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=fileURLToPath(new URL('../',import.meta.url));
const dir=mkdtempSync(join(tmpdir(),'agentos-udp-contract-'));
const nativePath=join(dir,'native');
const run=(command,args)=>{const r=spawnSync(command,args,{encoding:'utf8',timeout:10000,maxBuffer:65536});assert.equal(r.error,undefined);assert.equal(r.status,0,r.stderr);return r.stdout;};
try {
 const source=join(root,'toolchain/test-programs/udp_recvmsg_contract.c');
 run(process.env.CC??'cc',['-std=c11','-Wall','-Wextra','-Werror',source,'-o',nativePath]);
 const expected=run(nativePath,[]);
 const fallback=join(dir,'fallback');
 run(process.env.CC??'cc',['-std=c11','-D_GNU_SOURCE','-Drecvmsg=agentos_test_recvmsg','-Dsendmsg=agentos_test_sendmsg','-Dsocketpair=agentos_test_socketpair','-Wno-attributes','-ffunction-sections','-Wl,--gc-sections',join(root,'toolchain/std-patches/wasi-libc-overrides/unix_socket_ancillary.c'),join(root,'toolchain/test-programs/udp_recvmsg_host_stub.c'),source,'-o',fallback]);
 assert.equal(run(fallback,[]),expected,'actual sysroot fallback with injected ENOTSOCK route must match Linux');
 if(process.env.AGENTOS_UDP_PROBE_DIR){
  const {createKernel,createInMemoryFileSystem,createWasmVmRuntime,allowAll}=await import('../packages/core/dist/test-runtime.js');
  const kernel=createKernel({filesystem:createInMemoryFileSystem(),permissions:{...allowAll,network:'deny'},loopbackExemptPorts:[]});
  try {
   await kernel.mount(createWasmVmRuntime({commandDirs:[process.env.AGENTOS_UDP_PROBE_DIR]}));
   let stdout='',stderr='';
   const child=kernel.spawn('udp_recvmsg_contract',[],{timeout:10000,onStdout:bytes=>{stdout+=new TextDecoder().decode(bytes);},onStderr:bytes=>{stderr+=new TextDecoder().decode(bytes);}});
   child.closeStdin();assert.equal(await child.wait(),0,stderr);assert.equal(stdout,expected);
  }finally{await kernel.dispose();}
 }else console.log('WASM acceptance not run: set AGENTOS_UDP_PROBE_DIR to the rebuilt probe directory');
 console.log('UDP native and sysroot fallback contracts passed');
}finally{rmSync(dir,{recursive:true,force:true});}
