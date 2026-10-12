// Tiny fault-injection witness: real transformed owning modules, fake VM admin;
// no VM, subprocess, build, or production mutation.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import vm from 'node:vm';
const root=process.env.AGENTOS_LIFECYCLE_ROOT ?? new URL('../src',import.meta.url).pathname;
const nativeRequire=createRequire(import.meta.url);
import { transform } from './source-transform.mjs';
async function load(path,extra='',overrides={}) {
 const {code}=await transform((await readFile(path,'utf8'))+extra,{loader:'ts',format:'cjs',define:{'import.meta.url':JSON.stringify(`file://${path}`)}});
 const context={module:{exports:{}},console,process,URL,Error,AggregateError,Promise,AbortController,Buffer,setTimeout,clearTimeout,require(name){if(name in overrides)return overrides[name];if(name.startsWith('node:'))return nativeRequire(name);return {};}};
 vm.runInNewContext(code,context);return context.module.exports;
}
const rpc=await load(`${root}/sidecar/rpc-client.ts`);
const api=await load(`${root}/agent-os.ts`,'\nexport {leaseAgentOsSidecarVm,createInProcessSidecarTransport,sidecarStates};',{'./sidecar/rpc-client.js':rpc});

for(const cleanupFails of [false,true]) {
 let resolveCreate,entered;const creation=new Promise(resolve=>resolveCreate=resolve),started=new Promise(resolve=>entered=resolve);
 let adminDisposals=0;
 const sidecar=new api.AgentOsSidecar('injected-owner',{kind:'explicit',sidecarId:'injected-owner'});
 const leasePending=api.leaseAgentOsSidecarVm(sidecar,{async createVm(){entered();await creation;return {async dispose(){adminDisposals++;if(cleanupFails)throw Error('injected admin cleanup failure');}};}});
 await started;
 await assert.rejects(sidecar.dispose(),{code:'ERR_AGENTOS_SIDECAR_CREATION_PENDING'});
 assert.equal(sidecar.describe().state,'disposing');assert.equal(sidecar.describe().activeVmCount,1);
 resolveCreate();
 await assert.rejects(leasePending,cleanupFails?/creation and cleanup failed/:/disposal began during VM creation/);
 assert.equal(adminDisposals,1);
 if(cleanupFails){
  assert.equal(sidecar.describe().activeVmCount,1);assert.equal(api.sidecarStates.get(sidecar).eventLoopHolds,1);
  await assert.rejects(sidecar.dispose(),/injected admin cleanup failure/);
  assert.equal(adminDisposals,1,'do not replay terminal admin disposal');
  api.sidecarStates.get(sidecar).nativeProcess=Promise.resolve({client:{async dispose(){}},session:{}});
  await assert.rejects(sidecar.dispose(),/injected admin cleanup failure/);
 }else await sidecar.dispose();
 assert.equal(sidecar.describe().state,'disposed');assert.equal(sidecar.describe().activeVmCount,0);
 assert.equal(api.sidecarStates.get(sidecar).eventLoopHolds,0);
 console.log(`PASS injected pending creation, cleanupFails=${cleanupFails}`);
}
for (const scope of ['client-session','session','client-vm','vm']) {
 let resolve,entered;const gate=new Promise(r=>resolve=r),started=new Promise(r=>entered=r);let cleanups=0;
 const client=new rpc.AgentOsSidecarClient({async createOwnershipTransport(){if(scope==='client-session'){entered();await gate;}return {async createVm(){if(scope!=='client-session'){entered();await gate;}},async disposeVm(){cleanups++;},async dispose(){cleanups++;}};}});
 let session,pending;
 if(scope==='client-session')pending=client.createOwnershipSession();
 else {session=await client.createOwnershipSession();pending=session.createVm();}
 await started;
 const close=()=>scope==='session'?session.dispose():scope==='vm'?client.disposeVm(session.sessionId,client.listVms(session.sessionId)[0].vmId):client.dispose();
 await assert.rejects(close(),{code:'ERR_AGENTOS_SIDECAR_CREATION_PENDING'});
 resolve();await assert.rejects(pending,/disposed|disposal began/);
 await close();assert(cleanups>0);await client.dispose();
 console.log(`PASS injected deferred ${scope} creation remains owned until retry`);
}
{
 // Inject the client boundary rejection: proves owning lease catch retains both
 // failures, without claiming a real native transport failed this way.
 const original=rpc.AgentOsSidecarClient.prototype.dispose;
 const createError=Error('injected creation failure'),cleanupError=Error('injected client cleanup failure');
 rpc.AgentOsSidecarClient.prototype.dispose=async()=>{throw cleanupError;};
 try {
  const owner=new api.AgentOsSidecar('failed-create',{kind:'explicit',sidecarId:'failed-create'});
  await assert.rejects(api.leaseAgentOsSidecarVm(owner,{async createVm(){throw createError;}}),error=>error instanceof AggregateError&&error.errors[0]===createError&&error.errors[1]===cleanupError);
  const state=api.sidecarStates.get(owner);assert.equal(state.activeLeases.size,1);assert.equal(state.eventLoopHolds,1);
  await assert.rejects(owner.dispose(),error=>error===cleanupError);
  state.nativeProcess=Promise.resolve({client:{async dispose(){}},session:{}});
  await assert.rejects(owner.dispose(),error=>error===cleanupError);
  assert.equal(state.eventLoopHolds,0);assert.equal(owner.describe().state,'disposed');
  console.log('PASS injected creation and client cleanup failures retained together');
 } finally {rpc.AgentOsSidecarClient.prototype.dispose=original;}
}
