import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import vm from 'node:vm';
const nativeRequire=createRequire(import.meta.url);
import { transform } from './source-transform.mjs';
const sourceUrl=process.env.AGENTOS_LIFECYCLE_ROOT ? new URL(`file://${process.env.AGENTOS_LIFECYCLE_ROOT}/sidecar/rpc-client.ts`) : new URL('../src/sidecar/rpc-client.ts',import.meta.url);
const {code}=await transform(await readFile(sourceUrl,'utf8'),{loader:'ts',format:'cjs'});
function loadRpc() {
 const context={module:{exports:{}},console,Error,Promise,AbortController,setTimeout,clearTimeout,require(name){if(name.startsWith('node:'))return nativeRequire(name);return {};}};
 vm.runInNewContext(code,context);return context.module.exports;
}
function fixture(transport){let id=0;return new (loadRpc().AgentOsSidecarClient)({createOwnershipTransport:async()=>transport,createId:()=>String(++id)});
}
function deferred(){let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};}

test('injected unconfirmed VM teardown remains a terminal failure rather than false success on repeat',async()=>{
 let deletes=0;const failure=new Error('injected teardown outcome unconfirmed');
 const client=fixture({async createVm(){},async disposeVm(){deletes++;throw failure;},async dispose(){}});
 const session=await client.createOwnershipSession();await session.createVm();
 let first;try{await client.dispose();assert.fail('expected failure');}catch(error){first=error;}
 await assert.rejects(client.dispose(),error=>error===first);
 assert.equal(deletes,1,'non-idempotent VM deletion is not blindly replayed');
});
test('confirmed VM deletion followed by ownership-transport error does not replay VM deletion',async()=>{
 let deletes=0,closes=0;const failure=new Error('injected transport close error after confirmed VM deletion');
 const client=fixture({async createVm(){},async disposeVm(){deletes++;},async dispose(){closes++;throw failure;}});
 const session=await client.createOwnershipSession();await session.createVm();
 let first;try{await client.dispose();assert.fail('expected failure');}catch(error){first=error;}
 await assert.rejects(client.dispose(),error=>error===first);
 assert.equal(deletes,1);assert.equal(closes,1);
 assert.equal(client.listVms(session.sessionId)[0].state,'disposed');
});
test('concurrent ownership disposal waits for the first actual transport attempt',async()=>{
 const gate=deferred();let deletes=0;
 const client=fixture({async createVm(){},disposeVm(){deletes++;return gate.promise;},async dispose(){}});
 const session=await client.createOwnershipSession();await session.createVm();
 const first=client.dispose(),second=client.dispose();let settled=0;
 first.then(()=>settled++,()=>settled++);second.then(()=>settled++,()=>settled++);
 await new Promise(resolve=>setImmediate(resolve));assert.equal(settled,0);assert.equal(deletes,1);
 gate.resolve();await Promise.all([first,second]);
});


const agentUrl=process.env.AGENTOS_LIFECYCLE_ROOT ? new URL(`file://${process.env.AGENTOS_LIFECYCLE_ROOT}/agent-os.ts`) : new URL('../src/agent-os.ts',import.meta.url);
const {code:agentCode}=await transform((await readFile(agentUrl,'utf8'))+'\nexport {leaseAgentOsSidecarVm,createInProcessSidecarTransport,sidecarStates};',{loader:'ts',format:'cjs',define:{'import.meta.url':JSON.stringify(agentUrl.href)}});
function loadAgent(){
 const rpc=loadRpc();const context={module:{exports:{}},console,process,URL,Error,AggregateError,Promise,AbortController,setTimeout,clearTimeout,require(name){if(name.startsWith('node:'))return nativeRequire(name);if(name==='./sidecar/rpc-client.js')return rpc;return {};}};
 vm.runInNewContext(agentCode,context);return context.module.exports;
}
test('actual VM lease keeps final-owner link and the same failed disposal result',async()=>{
 const api=loadAgent();const sidecar=new api.AgentOsSidecar('test-owner',{kind:'explicit',sidecarId:'test-owner'});let deletes=0;
 const lease=await api.leaseAgentOsSidecarVm(sidecar,{async createVm(){return {async dispose(){deletes++;throw Error('injected admin teardown unconfirmed');}};}});
 let first;try{await lease.dispose();assert.fail('expected failure');}catch(error){first=error;}
 await assert.rejects(lease.dispose(),error=>error===first);assert.equal(deletes,1,'no teardown replay inside transport cleanup');
 assert.equal(sidecar.describe().activeVmCount,1);assert.equal(api.sidecarStates.get(sidecar).activeLeases.size,1);
});
test('actual public VM disposal retains owner and cannot fall back to a different kernel after failure',async()=>{
 const api=loadAgent();const sidecar=new api.AgentOsSidecar('test-owner',{kind:'explicit',sidecarId:'test-owner'});let leaseCalls=0,kernelCalls=0;
 const client={onEvent(){return ()=>{};},onClose(){return ()=>{};}};
 const instance=new api.AgentOs({async dispose(){kernelCalls++;}},sidecar,[],[],{},{},client,{},{});
 instance._cronManager={dispose(){}};const failure=new Error('injected VM lease failure');
 const lease={sidecar,async dispose(){leaseCalls++;throw failure;}};instance._sidecarLease=lease;
 await assert.rejects(instance.dispose(),error=>error===failure);
 await assert.rejects(instance.dispose(),error=>error===failure);
 assert.equal(leaseCalls,1);assert.equal(kernelCalls,0);assert.equal(instance._sidecarLease,lease);
});
test('actual kernel proxy preserves terminal deletion failure rather than returning success on repeat',async()=>{
 const {SidecarKernelProxy}=loadRpc();let deletes=0;const failure=new Error('injected native deletion unconfirmed');
 const proxy=Object.assign(Object.create(SidecarKernelProxy.prototype),{
  disposalPromise:null,disposed:false,disposing:false,trackedProcesses:new Map(),trackedProcessesById:new Map(),signalStates:new Map(),signalRefreshes:new Map(),localMounts:[],
  client:{async disposeVm(){deletes++;throw failure;}},session:{},vm:{},ownsClient:false,eventPumpAbortController:new AbortController(),eventPump:Promise.resolve(),mountReconfigurePromise:null,
 });
 await assert.rejects(proxy.dispose(),error=>error===failure);
 await assert.rejects(proxy.dispose(),error=>error===failure);assert.equal(deletes,1);
});

test('failed VM disposal preserves peer VM until explicit owner close then releases every lease hold',async()=>{
 const api=loadAgent();const sidecar=new api.AgentOsSidecar('test-owner',{kind:'explicit',sidecarId:'test-owner'});
 let peerDisposals=0,nativeDisposals=0;
 const failed=await api.leaseAgentOsSidecarVm(sidecar,{async createVm(){return {async dispose(){throw Error('injected unconfirmed VM teardown');}};}});
 await api.leaseAgentOsSidecarVm(sidecar,{async createVm(){return {async dispose(){peerDisposals++;}};}});
 const state=api.sidecarStates.get(sidecar);
 state.nativeProcess=Promise.resolve({client:{async dispose(){nativeDisposals++;}},session:{}});
 await assert.rejects(failed.dispose(),/unconfirmed VM teardown/);
 assert.equal(peerDisposals,0);assert.equal(nativeDisposals,0);assert.equal(sidecar.describe().activeVmCount,2);
 await assert.rejects(sidecar.dispose(),/unconfirmed VM teardown/);
 assert.equal(peerDisposals,1);assert.equal(nativeDisposals,1);assert.equal(sidecar.describe().state,'disposed');
 assert.equal(state.activeLeases.size,0);assert.equal(state.eventLoopHolds,0);
});
