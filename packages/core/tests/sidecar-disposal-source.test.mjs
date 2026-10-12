import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import vm from 'node:vm';
const nativeRequire=createRequire(import.meta.url);
import { transform } from './source-transform.mjs';
const sourceUrl=process.env.AGENTOS_LIFECYCLE_SOURCE ? new URL(process.env.AGENTOS_LIFECYCLE_SOURCE) : new URL('../src/agent-os.ts',import.meta.url);
const source=await readFile(sourceUrl,'utf8');
const {code}=await transform(source+'\nexport {sidecarStates,sharedSidecars,ensureSharedSidecarNativeProcess};',{loader:'ts',format:'cjs',define:{'import.meta.url':JSON.stringify(sourceUrl.href)}});
function fixture(client) {
 const sourceProcess={...process,env:{...process.env,AGENTOS_SIDECAR_BIN:'/injected-never-executed'},on(){return this;}};
 const context={module:{exports:{}},console,process:sourceProcess,URL,Error,AggregateError,Promise,AbortController,Buffer,setTimeout,clearTimeout,require(name){
  if(name==='./sidecar/rpc-client.js')return {SidecarProcess:{spawn(){return client;}}};
  if(name==='./sidecar/binary.js')return {resolvePublishedSidecarBinary(){return '/injected-never-executed';}};
  if(name.startsWith('node:'))return nativeRequire(name);
  return {};
 }};
 vm.runInNewContext(code,context);
 const api=context.module.exports;
 const sidecar=new api.AgentOsSidecar('test-sidecar',{kind:'explicit',sidecarId:'test-sidecar'},'test-pool');
 api.sharedSidecars.set('test-pool',sidecar);
 const state=api.sidecarStates.get(sidecar);
 return {api,sidecar,state};
}
function deferred(){let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};}

test('injected native client disposal failure is visible and owner remains reachable for retry',async()=>{
 const {sidecar,state,api}=fixture();const failure=new Error('injected transport disposal failure');let attempts=0;
 const child={kill(){throw Error('no actual child belongs to this source fixture');}};
 const pending=Promise.resolve({client:{async dispose(){if(++attempts===1)throw failure;}},session:{}});
 state.nativeProcess=pending;state.sharedChild=child;
 await assert.rejects(sidecar.dispose(),error=>error===failure);
 assert.equal(state.nativeProcess,pending);assert.equal(state.sharedChild,child);
 assert.equal(api.sharedSidecars.get('test-pool'),sidecar);assert.equal(sidecar.describe().state,'disposing');
 await sidecar.dispose();assert.equal(attempts,2);assert.equal(state.nativeProcess,undefined);
 assert.equal(state.sharedChild,undefined);assert.equal(sidecar.describe().state,'disposed');
});
test('concurrent disposal callers await the same native cleanup outcome',async()=>{
 const {sidecar,state}=fixture();const gate=deferred();let calls=0;
 state.nativeProcess=Promise.resolve({client:{dispose(){calls++;return gate.promise;}},session:{}});
 const first=sidecar.dispose(),second=sidecar.dispose();
 let settled=0;first.then(()=>settled++,()=>settled++);second.then(()=>settled++,()=>settled++);
 await new Promise(resolve=>setImmediate(resolve));assert.equal(calls,1);assert.equal(settled,0);
 gate.resolve();await Promise.all([first,second]);assert.equal(sidecar.describe().state,'disposed');
});
test('failed VM lease is retained when final native teardown also fails',async()=>{
 const {sidecar,state}=fixture();let leaseCalls=0,nativeCalls=0;
 const lease={async dispose(){if(++leaseCalls===1)throw new Error('injected VM cleanup failure');}};
 state.activeLeases.add(lease);state.description.activeVmCount=1;
 state.nativeProcess=Promise.resolve({client:{async dispose(){if(++nativeCalls===1)throw new Error('injected native cleanup failure');}},session:{}});
 await assert.rejects(sidecar.dispose(),error=>error instanceof AggregateError && error.errors.some(item=>item.message==='injected VM cleanup failure'));
 assert.equal(state.activeLeases.has(lease),true);assert.equal(sidecar.describe().activeVmCount,1);assert.equal(nativeCalls,1);
 await sidecar.dispose();assert.equal(leaseCalls,2);assert.equal(nativeCalls,2);assert.equal(state.activeLeases.size,0);
});
test('pool-wide cleanup preserves failed owner rather than clearing the pool before await',async()=>{
 const {sidecar,state,api}=fixture();let attempts=0;
 state.nativeProcess=Promise.resolve({client:{async dispose(){if(++attempts===1)throw Error('injected cleanup failure');}},session:{}});
 await assert.rejects(api.__disposeAllSharedSidecarsForTesting(),/injected cleanup failure/);
 assert.equal(api.sharedSidecars.get('test-pool'),sidecar);
 await api.__disposeAllSharedSidecarsForTesting();assert.equal(attempts,2);assert.equal(api.sharedSidecars.size,0);
});

test('injected nonzero-exit error after confirmed exit is reported but dead pool owner is removed',async()=>{
 const {sidecar,state,api}=fixture();const failure=new Error('sidecar exited with code 17');let calls=0;
 state.sharedChild={exitCode:17,signalCode:null};
 state.nativeProcess=Promise.resolve({client:{async dispose(){calls++;throw failure;}},session:{}});
 await assert.rejects(sidecar.dispose(),error=>error===failure);
 assert.equal(state.nativeProcess,undefined);assert.equal(state.sharedChild,undefined);
 assert.equal(sidecar.describe().state,'disposed');assert.equal(api.sharedSidecars.has('test-pool'),false);
 await assert.rejects(sidecar.dispose(),error=>error===failure);assert.equal(calls,1);
});
test('concurrent callers receive the same unconfirmed disposal failure and can retry',async()=>{
 const {sidecar,state}=fixture();const gate=deferred();const failure=new Error('injected interrupted disposal');let calls=0;
 state.sharedChild={exitCode:null,signalCode:null};
 state.nativeProcess=Promise.resolve({client:{dispose(){calls++;return calls===1?gate.promise:Promise.resolve();}},session:{}});
 const first=sidecar.dispose(),second=sidecar.dispose();
 const outcomes=Promise.allSettled([first,second]);
 await new Promise(resolve=>setImmediate(resolve));gate.reject(failure);
 const results=await outcomes;assert.equal(calls,1);
 assert(results.every(result=>result.status==='rejected'&&result.reason===failure));
 assert.equal(sidecar.describe().state,'disposing');await sidecar.dispose();assert.equal(calls,2);
});


test('failed VM cleanup is reported while successful native teardown releases all leases',async()=>{
 const {sidecar,state,api}=fixture();const failure=new Error('injected quarantined VM failure');
 const lease={async dispose(){state.activeLeases.delete(lease);throw failure;}};
 state.activeLeases.add(lease);state.description.activeVmCount=1;
 state.nativeProcess=Promise.resolve({client:{async dispose(){}},session:{}});
 await assert.rejects(sidecar.dispose(),error=>error===failure);
 assert.equal(state.activeLeases.size,0);assert.equal(sidecar.describe().activeVmCount,0);
 assert.equal(sidecar.describe().state,'disposed');assert.equal(api.sharedSidecars.size,0);
});

for(const bootstrap of [false,true]) test(`actual ${bootstrap?'bootstrap failure':'transport close'} retains native cleanup owner`,async()=>{
 const failure=new Error('injected authentication failure');let close,calls=0;
 const child={exitCode:null,signalCode:null,kill(){return true;}};
 const client={protocolClient:{child},async authenticateAndOpenSession(){if(bootstrap)throw failure;return {};},onClose(handler){close=handler;},async dispose(){calls++;if(calls===1)throw Error('injected exit unconfirmed');child.exitCode=0;}};
 const {api,sidecar,state}=fixture(client);
 if(bootstrap)await assert.rejects(api.ensureSharedSidecarNativeProcess(sidecar),error=>error===failure);
 else {await api.ensureSharedSidecarNativeProcess(sidecar);close(Error('injected transport loss'));}
 assert.equal(sidecar.describe().state,'disposing');assert.equal(state.nativeClient,client);
 await assert.rejects(sidecar.dispose(),/injected exit unconfirmed/);
 assert.equal(state.nativeClient,client);assert.equal(state.sharedChild,child);
 await sidecar.dispose();assert.equal(calls,2);assert.equal(state.nativeClient,undefined);
 assert.equal(sidecar.describe().state,'disposed');
});
