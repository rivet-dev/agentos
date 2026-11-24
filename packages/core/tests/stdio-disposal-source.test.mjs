import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import { transform } from './source-transform.mjs';
const {code}=await transform(await readFile(process.env.AGENTOS_STDIO_SOURCE ?? new URL('../src/stdio-client.ts',import.meta.url),'utf8'),{loader:'ts',format:'cjs'});
function fixture(exits, shutdownError) {
 const context={module:{exports:{}},Error,Promise,require(name){if(name==='./sidecar-process.js')return {registerSidecarProcessSpawnFactory(){}};return {};}};
 vm.runInNewContext(code,context);
 const descriptor=()=>({destroyed:false,end(){},destroy(){this.destroyed=true;}});
 let waits=0,kills=0,disposed=0;
 const child={exitCode:null,signalCode:null,stdin:descriptor(),stdout:descriptor(),stderr:descriptor(),kill(){kills++;}};
 const instance=Object.assign(Object.create(context.module.exports.StdioSidecarProtocolClient.prototype),{
  child,gracefulExitMs:5,forceExitMs:2,disposedErrorMessage:'disposed',
  protocolClient:{async shutdown(){if(shutdownError)throw shutdownError;},failPermanently(){},dispose(){disposed++;}},
  sidecarProcess:{control:null,stderrText:()=>'',async waitForExit(){const result=exits[waits++];child.exitCode=result.code;child.signalCode=result.signal;return result.code;}},
 });
 return {instance,child,counts:()=>({waits,kills,disposed})};
}
test('actual stdio cleanup cannot report success after both injected exit waits time out',async()=>{
 const {instance,counts}=fixture([{code:null,signal:null},{code:null,signal:null}]);
 await assert.rejects(instance.dispose(),{code:'ERR_AGENTOS_SIDECAR_EXIT_UNCONFIRMED'});
 assert.deepEqual(counts(),{waits:2,kills:1,disposed:0});
});
test('actual stdio cleanup accepts confirmed signal exit even though waitForExit returns null',async()=>{
 const {instance,counts}=fixture([{code:null,signal:null},{code:null,signal:'SIGKILL'}]);
 await instance.dispose();assert.deepEqual(counts(),{waits:2,kills:1,disposed:1});
});
test('actual stdio cleanup reports nonzero exit observed during forced-wait phase',async()=>{
 const {instance,counts}=fixture([{code:null,signal:null},{code:17,signal:null}]);
 await assert.rejects(instance.dispose(),/sidecar exited with code 17/);
 assert.deepEqual(counts(),{waits:2,kills:1,disposed:1});
});

for (const code of [null,17]) test(`injected shutdown rejection remains cause when forced exit is ${code}`,async()=>{
 const failure=new Error('injected shutdown failure');
 const {instance}=fixture([{code:null,signal:null},{code,signal:null}],failure);
 await assert.rejects(instance.dispose(),error=>error.cause===failure);
});

test('shutdown write cannot prevent the graceful exit deadline from starting',async()=>{
 const {instance,child,counts}=fixture([{code:null,signal:null},{code:null,signal:'SIGKILL'}]);
 let rejectWrite;
 instance.protocolClient.shutdown=()=>new Promise((_,reject)=>{rejectWrite=reject;});
 const disposal=instance.dispose();
 const result=await Promise.race([
  disposal.then(()=>({unexpectedSuccess:true}),error=>({error})),
  new Promise(resolve=>setTimeout(()=>resolve({blocked:true}),30)),
 ]);
 // Settle the injected write even on failure, without hiding the observed hang.
 rejectWrite(new Error('late injected shutdown write failure'));
 await disposal.catch(()=>{});
 assert.equal(result.blocked,undefined,'disposal waited indefinitely for the write before starting its exit deadline');
 assert.equal(result.error?.code,'ERR_AGENTOS_SIDECAR_SHUTDOWN_TIMEOUT');
 assert.equal(child.signalCode,'SIGKILL');
 assert.deepEqual(counts(),{waits:2,kills:1,disposed:1});
});

test('confirmed clean exit before the shutdown write callback is not a timeout',async()=>{
 const {instance,counts}=fixture([{code:0,signal:null}]);
 let rejectWrite;
 instance.protocolClient.shutdown=()=>new Promise((_,reject)=>{rejectWrite=reject;});
 const disposal=instance.dispose();
 const completed=await Promise.race([
  disposal.then(()=>true),
  new Promise(resolve=>setTimeout(()=>resolve(false),30)),
 ]);
 rejectWrite(new Error('late write callback after clean exit'));
 await disposal;
 assert.equal(completed,true,'confirmed child exit must not wait for a stalled write callback');
 assert.deepEqual(counts(),{waits:1,kills:0,disposed:1});
});
