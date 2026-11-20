// Actual owning module with injected native transport/child effects. No VM spawn.
import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import {createRequire} from 'node:module';
import vm from 'node:vm';
const nativeRequire=createRequire(import.meta.url);
import { transform } from './source-transform.mjs';
const source=process.env.AGENTOS_REVIEW_SOURCE ?? new URL('../src/agent-os.ts',import.meta.url).pathname;
const {code}=await transform((await readFile(source,'utf8'))+'\nexport {ensureSharedSidecarNativeProcess,resolveAgentOsSidecar,sidecarStates,sharedSidecars};',{loader:'ts',format:'cjs',define:{'import.meta.url':JSON.stringify(`file://${source}`)}});
function deferred(){let resolve;const promise=new Promise(r=>resolve=r);return {promise,resolve};}
async function fixture({runtime,bootstrapFails=false}={}) {
 let close,disposals=0,kills=0,failDisposal,gate;const logs=[];
 const failure=Error('injected native transport failure');
 const child={exitCode:null,signalCode:null,kill(){kills++;return true;}};
 const client={protocolClient:{child},async authenticateAndOpenSession(){if(bootstrapFails)throw failure;return {};},onClose(handler){close=handler;},async dispose(){disposals++;if(gate)await gate;if(failDisposal)throw failDisposal;}};
 const sourceProcess={...process,env:{...process.env,AGENTOS_SIDECAR_BIN:'/injected-never-executed'},on(){return this;}};
 const context={module:{exports:{}},console:{...console,error(...args){logs.push(args);}},process:sourceProcess,URL,Error,AggregateError,Promise,AbortController,Buffer,setTimeout,clearTimeout,require(name){
  if(name==='./sidecar/rpc-client.js')return {SidecarProcess:{spawn(){return client;}}};
  if(name==='./sidecar/binary.js')return {resolvePublishedSidecarBinary(){return '/injected-never-executed';}};
  if(name.startsWith('node:'))return nativeRequire(name);return {};
 }};
 vm.runInNewContext(code,context);const api=context.module.exports;
 const options={pool:'test-recovery',...(runtime?{runtime}:{})};
 const owner=await api.AgentOs.getSharedSidecar(options);
 if(bootstrapFails)await assert.rejects(api.ensureSharedSidecarNativeProcess(owner),error=>error===failure);
 else await api.ensureSharedSidecarNativeProcess(owner);
 return {api,owner,options,child,failure,logs,close:()=>close(failure),counts:()=>disposals,kills:()=>kills,setFailure(error){failDisposal=error;},setGate(promise){gate=promise;}};
}
test('confirmed dead failed shared owner is drained then replaced and old error preserved',async()=>{
 const f=await fixture();f.child.signalCode='SIGKILL';f.close();
 const error=Error('injected disposal error after confirmed exit');f.setFailure(error);
 const next=await f.api.AgentOs.getSharedSidecar(f.options);
 assert.notEqual(next,f.owner);assert.equal(next.describe().state,'ready');assert.equal(f.counts(),1);
 assert.equal(f.api.sidecarStates.get(f.owner).nativeProcess,undefined);
 await assert.rejects(f.owner.dispose(),reason=>reason===error);
 assert.equal(f.logs.length,1);assert(f.logs[0].includes(f.failure));assert(f.logs[0].includes(error));
});
test('unconfirmed failed shared owner stays reachable and later acquisition retries cleanup',async()=>{
 const f=await fixture();f.close();const error=Error('injected unconfirmed exit');f.setFailure(error);
 await assert.rejects(f.api.AgentOs.getSharedSidecar(f.options),reason=>reason===error);
 assert.equal(f.api.sharedSidecars.get(f.options.pool),f.owner);assert.equal(f.logs.length,0);
 f.child.signalCode='SIGKILL';
 const next=await f.api.AgentOs.getSharedSidecar(f.options);assert.notEqual(next,f.owner);assert.equal(f.counts(),2);
});
test('concurrent implicit acquisitions join cleanup and return one replacement',async()=>{
 const f=await fixture();f.child.exitCode=0;f.close();const gate=deferred();f.setGate(gate.promise);
 const first=f.api.AgentOs.getSharedSidecar(f.options),second=f.api.resolveAgentOsSidecar({kind:'shared',...f.options});
 await new Promise(resolve=>setImmediate(resolve));assert.equal(f.counts(),1);
 gate.resolve();const [a,b]=await Promise.all([first,second]);assert.equal(a,b);assert.notEqual(a,f.owner);assert.equal(f.logs.length,1);
});
test('bootstrap failure uses retained client cleanup before shared replacement',async()=>{
 const f=await fixture({bootstrapFails:true});f.child.signalCode='SIGKILL';
 const next=await f.api.AgentOs.getSharedSidecar(f.options);assert.notEqual(next,f.owner);assert.equal(f.counts(),1);assert.equal(f.logs.length,1);
});
test('explicit handle selection and intentional disposal do not initiate pool recovery',async()=>{
 const f=await fixture();f.child.signalCode='SIGKILL';f.close();
 assert.equal(await f.api.resolveAgentOsSidecar({kind:'explicit',handle:f.owner}),f.owner);assert.equal(f.counts(),0);
 const other=await fixture();const gate=deferred();other.setGate(gate.promise);
 const disposing=other.owner.dispose();await new Promise(resolve=>setImmediate(resolve));
 assert.equal(await other.api.AgentOs.getSharedSidecar(other.options),other.owner);
 gate.resolve();await disposing;assert.equal(other.logs.length,0);
});
test('runtime mismatch is rejected before cleanup and after a concurrent replacement',async()=>{
 const f=await fixture({runtime:{executor:{maxActiveVms:2}}});f.child.exitCode=0;f.close();
 await assert.rejects(f.api.AgentOs.getSharedSidecar({...f.options,runtime:{executor:{maxActiveVms:3}}}),/different runtime settings/);
 assert.equal(f.counts(),0);
 const gate=deferred();f.setGate(gate.promise);
 const first=f.api.AgentOs.getSharedSidecar({pool:f.options.pool});
 const second=f.api.AgentOs.getSharedSidecar(f.options);
 await new Promise(resolve=>setImmediate(resolve));gate.resolve();await first;
 await assert.rejects(second,/different runtime settings/);
});
test('pending creation blocks failed-owner replacement without dropping ownership',async()=>{
 const f=await fixture();f.close();const state=f.api.sidecarStates.get(f.owner);
 state.activeLeases.add({creating:true,async dispose(){throw Error('creation was not settled');}});
 await assert.rejects(f.api.AgentOs.getSharedSidecar(f.options),{code:'ERR_AGENTOS_SIDECAR_CREATION_PENDING'});
 assert.equal(f.counts(),0);assert.equal(f.api.sharedSidecars.get(f.options.pool),f.owner);
});
test('failed VM lease is released only after recovery confirms native owner cleanup',async()=>{
 const f=await fixture();f.close();const state=f.api.sidecarStates.get(f.owner);let releases=0;
 const vmError=Error('injected VM cleanup failure'),nativeError=Error('injected unconfirmed native cleanup');
 state.activeLeases.add({async dispose(){throw vmError;},releaseAfterOwnerExit(){releases++;}});
 state.description.activeVmCount=1;f.setFailure(nativeError);
 await assert.rejects(f.api.AgentOs.getSharedSidecar(f.options),error=>error instanceof AggregateError&&error.errors.includes(vmError)&&error.errors.includes(nativeError));
 assert.equal(releases,0);assert.equal(state.activeLeases.size,1);
 f.child.signalCode='SIGKILL';const next=await f.api.AgentOs.getSharedSidecar(f.options);
 assert.notEqual(next,f.owner);assert.equal(releases,1);assert.equal(state.activeLeases.size,0);assert.equal(f.logs.length,1);
});


test('unexpected close after deferred disposal rejection still terminates the retained native owner',async()=>{
 const f=await fixture();const state=f.api.sidecarStates.get(f.owner);
 const creating={creating:true,async dispose(){}};state.activeLeases.add(creating);
 await assert.rejects(f.owner.dispose(),{code:'ERR_AGENTOS_SIDECAR_CREATION_PENDING'});
 assert.equal(state.disposePromise,undefined);
 assert.equal(f.counts(),0,'native disposal has not started');
 f.close();
 assert.equal(f.kills(),1,'disposing state alone must not suppress termination of an unexpected transport failure');
 assert.equal(state.nativeFailure.error,f.failure);
 assert.equal(f.api.sharedSidecars.get(f.options.pool),f.owner);
 state.activeLeases.clear();await f.owner.dispose();
});
