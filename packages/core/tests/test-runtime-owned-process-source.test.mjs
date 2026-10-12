import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import vm from 'node:vm';
const nativeRequire=createRequire(import.meta.url);
import { transform } from './source-transform.mjs';
const sourceRoot=process.env.AGENTOS_TEST_RUNTIME_ROOT ? new URL(`file://${process.env.AGENTOS_TEST_RUNTIME_ROOT}/`) : new URL('../src/',import.meta.url);
const codes=new Map(await Promise.all(['kernel-proxy','test-runtime','sidecar-process','stdio-client','process','sidecar-errors','protocol-schema'].map(async name=> {
 const url=new URL(`${name}.ts`,sourceRoot);
 return [name,(await transform(await readFile(url,'utf8'),{loader:'ts',format:'cjs',define:{'import.meta.url':JSON.stringify(url.href)}})).code];
})));
function fixture({failurePhase,allowExit=true,exitCode=null,signal='SIGKILL',shared=false}={}) {
 const modules=new Map();
 function load(name){
  if(modules.has(name))return modules.get(name);
  const module={exports:{}};modules.set(name,module.exports);
  const context={module,console,process:{...process,env:{...process.env,AGENTOS_SIDECAR_BIN:'/injected-never-executed'}},URL,Error,AggregateError,Promise,AbortController,Buffer,Uint8Array,TextEncoder,TextDecoder,setTimeout,clearTimeout,performance,require(specifier){
   if(specifier.startsWith('node:'))return nativeRequire(specifier);
   if(specifier==='./binary.js')return {resolvePublishedSidecarBinary(){return '/injected-never-executed';}};
   const local=specifier.replace(/^\.\//,'').replace(/\.js$/,'');
   if(codes.has(local))return load(local);
   return {};
  }};
  vm.runInNewContext(codes.get(name),context);modules.set(name,module.exports);return module.exports;
 }
 const api=load('test-runtime'),{SidecarProcess}=load('sidecar-process');
 const child=new EventEmitter();Object.assign(child,{exitCode:null,signalCode:null,stdin:new PassThrough(),stdout:new PassThrough(),stderr:new PassThrough()});
 let kills=0,shutdowns=0,deletes=0,restores=0,spawns=0,closeListener;
 const initializationError=new Error(`injected ${failurePhase} initialization failure`),shutdownError=new Error('injected shutdown write failed'),vmError=new Error('injected VM deletion failed');
 const state={allowExit,failVm:false,disconnected:false};
 child.kill=()=>{kills++;if(state.allowExit)setImmediate(()=>{child.exitCode=exitCode;child.signalCode=signal;child.emit('exit',exitCode,signal);});};
 const childOwner=load('process').StdioSidecarProcess.fromChild(child,null);
 const protocol={
  async shutdown(){shutdowns++;throw shutdownError;},failPermanently(error){closeListener?.(error);},dispose(){},onClose(handler){closeListener=handler;return ()=>{};},onEvent(){return ()=>{};},setSidecarRequestHandler(){},
  async sendRequest({payload}){
   if(payload.type===failurePhase)throw initializationError;
   if(payload.type==='authenticate')return {payload:{type:'authenticated',connection_id:'conn-fixture'}};
   if(payload.type==='open_session')return {payload:{type:'session_opened',session_id:'session-fixture'}};
   if(payload.type==='create_vm')return {payload:{type:'vm_created',vm_id:'vm-fixture'}};
   if(payload.type==='configure_vm')return {payload:{type:'vm_configured',applied_mounts:[],applied_software:[],projected_commands:[]}};
   if(payload.type==='dispose_vm'){deletes++;if(state.failVm)throw vmError;return {payload:{type:'vm_disposed'}};}
   throw new Error('unexpected injected protocol request '+payload.type);
  },
  waitForEvent(matcher,_timeout,options){
   if(matcher.type==='vm_lifecycle'){if(failurePhase==='vm_ready')return Promise.reject(initializationError);return Promise.resolve({payload:{type:'vm_lifecycle'}});}
   return new Promise((_resolve,reject)=>options.signal.addEventListener('abort',()=>reject(new Error('aborted')),{once:true}));
  },
 };
 const transport=Object.assign(Object.create(load('stdio-client').StdioSidecarProtocolClient.prototype),{child,sidecarProcess:childOwner,protocolClient:protocol,gracefulExitMs:5,forceExitMs:2,disposedErrorMessage:'disposed'});
 const client=SidecarProcess.fromClient(transport);SidecarProcess.spawn=()=>{spawns++;return client;};
 const filesystem=new api.InMemoryFileSystem();
 const kernel=api.createKernel({filesystem,syncFilesystemOnDispose:false,...(failurePhase==='configure_vm'?{permissions:{network:'deny'}}:{}),...(shared?{sidecar:client}:{})});
 const restore=kernel.liveFilesystemBinding.restore.bind(kernel.liveFilesystemBinding);kernel.liveFilesystemBinding.restore=()=>{restores++;restore();};
 function attachProxy(){kernel.readyPromise=Promise.resolve();kernel.client=client;kernel.session={connectionId:'conn-fixture',sessionId:'session-fixture'};kernel.vm={vmId:'vm-fixture'};kernel.proxy=new (load('kernel-proxy').SidecarKernelProxy)({client,session:kernel.session,vm:kernel.vm,disposeClient:!shared,env:{},cwd:'/workspace',localMounts:[],commandGuestPaths:new Map()});}
 return {api,kernel,client,child,transport,state,attachProxy,initializationError,shutdownError,vmError,counts:()=>({kills,shutdowns,deletes,restores,spawns}),disconnect(){closeListener?.(new Error('injected transport disconnected'));}};
}

for(const [exitCode,signal] of [[null,'SIGKILL'],[17,null]])test(`actual transport rejection after confirmed child exit ${exitCode}/${signal} releases NativeKernel ownership`,async()=>{
 const f=fixture({exitCode,signal});f.attachProxy();f.state.failVm=true;
 await assert.rejects(f.kernel.dispose(),error=>error===f.vmError);
 assert.equal(f.child.exitCode,exitCode);assert.equal(f.child.signalCode,signal);
 assert.equal(f.kernel.client,null);assert.equal(f.kernel.proxy,null);assert.equal(f.counts().restores,1);
 await assert.rejects(f.kernel.dispose(),error=>error===f.vmError);assert.equal(f.counts().deletes,1);assert.equal(f.counts().shutdowns,1);
});

test('actual transport failed drain stays retryable and releases on confirmed signal exit despite rejected shutdown',async()=>{
 const f=fixture({allowExit:false});f.attachProxy();f.state.failVm=true;
 await assert.rejects(f.kernel.dispose(),error=>error===f.vmError);assert.equal(f.kernel.client,f.client);assert.equal(f.child.signalCode,null);
 f.state.allowExit=true;
 await assert.rejects(f.kernel.dispose(),error=>error===f.vmError);
 assert.equal(f.child.signalCode,'SIGKILL');assert.equal(f.kernel.client,null);assert.equal(f.counts().deletes,1);assert.equal(f.counts().shutdowns,2);assert.equal(f.counts().restores,1);
});

test('transport disconnect without child exit cannot release NativeKernel ownership',async()=>{
 const f=fixture({allowExit:false});f.attachProxy();f.disconnect();
 await assert.rejects(f.kernel.dispose());assert.equal(f.kernel.client,f.client);assert.equal(f.child.exitCode,null);assert.equal(f.child.signalCode,null);assert.equal(f.counts().restores,0);
});

for(const failurePhase of ['authenticate','create_vm','vm_ready','configure_vm'])test(`actual initialization ${failurePhase} failure retains its acquired owner and drains it on disposal`,async()=>{
 const f=fixture({failurePhase});
 await assert.rejects(f.kernel.mount(f.api.createNodeRuntime()),error=>error===f.initializationError);
 assert.equal(f.kernel.client,f.client,'spawned owner must be retained before every asynchronous initialization boundary');
 await assert.rejects(f.kernel.dispose(),error=>error===f.initializationError);
 assert.equal(f.child.signalCode,'SIGKILL');assert.equal(f.kernel.client,null);assert.equal(f.counts().shutdowns,1);assert.equal(f.counts().restores,1);
 assert.equal(f.counts().deletes,['vm_ready','configure_vm'].includes(failurePhase)?1:0);
 await assert.rejects(f.kernel.dispose(),error=>error===f.initializationError);assert.equal(f.counts().shutdowns,1);
});

test('failed shared initialization disposes only the known VM and retains no claim on its external child',async()=>{
 const f=fixture({failurePhase:'vm_ready',shared:true});
 await assert.rejects(f.kernel.mount(f.api.createNodeRuntime()),error=>error===f.initializationError);
 await assert.rejects(f.kernel.dispose(),error=>error===f.initializationError);
 assert.equal(f.counts().deletes,1);assert.equal(f.counts().shutdowns,0);assert.equal(f.child.signalCode,null);assert.equal(f.kernel.client,null);
});

test('actual confirmed child shutdown error remains the terminal result after successful VM deletion',async()=>{
 const f=fixture();f.attachProxy();
 await assert.rejects(f.kernel.dispose(),error=>error===f.shutdownError);
 assert.equal(f.child.signalCode,'SIGKILL');assert.equal(f.kernel.client,null);assert.equal(f.counts().deletes,1);assert.equal(f.counts().restores,1);
 await assert.rejects(f.kernel.dispose(),error=>error===f.shutdownError);assert.equal(f.counts().shutdowns,1);
});

test('failed authentication cleanup retains its live owner and later drains it despite repeated shutdown rejection',async()=>{
 const f=fixture({failurePhase:'authenticate',allowExit:false});
 await assert.rejects(f.kernel.mount(f.api.createNodeRuntime()),error=>error===f.initializationError);
 await assert.rejects(f.kernel.dispose(),error=>error===f.initializationError);
 assert.equal(f.kernel.client,f.client);assert.equal(f.counts().restores,0);
 f.state.allowExit=true;
 await assert.rejects(f.kernel.dispose(),error=>error===f.initializationError);
 assert.equal(f.child.signalCode,'SIGKILL');assert.equal(f.kernel.client,null);assert.equal(f.counts().deletes,0);assert.equal(f.counts().shutdowns,2);assert.equal(f.counts().restores,1);
});

test('failed shared initialization retains a failed known VM owner without replay or external child shutdown',async()=>{
 const f=fixture({failurePhase:'vm_ready',shared:true});f.state.failVm=true;
 await assert.rejects(f.kernel.mount(f.api.createNodeRuntime()),error=>error===f.initializationError);
 await assert.rejects(f.kernel.dispose(),error=>error===f.initializationError);
 await assert.rejects(f.kernel.dispose(),error=>error===f.initializationError);
 assert.equal(f.kernel.client,f.client);assert.equal(f.counts().deletes,1);assert.equal(f.counts().shutdowns,0);assert.equal(f.counts().restores,0);
});


for(const completed of [false,true])test(`terminal kernel disposal fences initialization, completed=${completed}`,async()=>{
 const f=fixture({failurePhase:'authenticate'});
 const disposal=f.kernel.dispose();
 if(completed)await disposal;
 let rejection;
 try {await f.kernel.mount(f.api.createNodeRuntime());}catch(error){rejection=error;}
 await disposal;
 const acquired=f.kernel.client;
 const counts=f.counts();
 // Clean the injected child even when running this regression against the old code.
 if(acquired)await f.client.dispose().catch(()=>{});
 assert.equal(rejection?.code,'ERR_AGENTOS_KERNEL_DISPOSED');
 assert.equal(counts.spawns,0,'terminal kernel must not spawn a new sidecar');
 assert.equal(acquired,null,'terminal kernel must not acquire an owner');
 assert.equal(f.kernel.readyPromise,null);
 assert.equal(f.kernel.dispose(),disposal,'settled disposal result remains cached');
});
