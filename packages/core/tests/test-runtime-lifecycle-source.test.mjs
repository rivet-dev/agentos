import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import vm from 'node:vm';
const nativeRequire=createRequire(import.meta.url);
import { transform } from './source-transform.mjs';
const sourceRoot=process.env.AGENTOS_TEST_RUNTIME_ROOT ? new URL(`file://${process.env.AGENTOS_TEST_RUNTIME_ROOT}/`) : new URL('../src/',import.meta.url);
const codes=new Map(await Promise.all(['kernel-proxy','test-runtime'].map(async name=> {
 const url=new URL(`${name}.ts`,sourceRoot);
 return [name,(await transform(await readFile(url,'utf8'),{loader:'ts',format:'cjs',define:{'import.meta.url':JSON.stringify(url.href)}})).code];
})));
function fixture() {
 const modules=new Map();
 function load(name) {
  if(modules.has(name)) return modules.get(name);
  const context={module:{exports:{}},console,process,URL,Error,AggregateError,Promise,AbortController,Buffer,Uint8Array,TextEncoder,TextDecoder,setTimeout,clearTimeout,performance,require(specifier){
   if(specifier.startsWith('node:'))return nativeRequire(specifier);
   if(specifier==='./kernel-proxy.js')return load('kernel-proxy');
   return {};
  }};
  vm.runInNewContext(codes.get(name),context);modules.set(name,context.module.exports);return context.module.exports;
 }
 const api=load('test-runtime');
 const kernel=api.createKernel({filesystem:new api.InMemoryFileSystem(),syncFilesystemOnDispose:false});
 const root=new api.InMemoryFileSystem();
 let restores=0,cleared=0;
 const client={configureVm:async()=>{},async dispose(){throw new Error('injected owner drain remains unconfirmed');},setVmSidecarRequestHandler(){cleared++;}};
 const proxy={dispose:async()=>{},registerCommandGuestPaths(){},createRootView(){return root;}};
 Object.assign(kernel,{readyPromise:Promise.resolve(),client,session:{},vm:{vmId:'vm-fixture'},proxy,rootFilesystem:root,liveFilesystemBinding:{restore(){restores++;}}});
 return {api,kernel,root,client,proxy,SidecarKernelProxy:load('kernel-proxy').SidecarKernelProxy,restores:()=>restores,cleared:()=>cleared};
}
function deferred(){let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};}

test('actual NativeKernel retains failed teardown ownership and repeats the same terminal error',async()=>{
 const f=fixture();const failure=new Error('injected unconfirmed teardown');let calls=0;
 f.proxy.dispose=async()=>{calls++;throw failure;};
 await assert.rejects(f.kernel.dispose(),error=>error===failure);
 await assert.rejects(f.kernel.dispose(),error=>error===failure);
 assert.equal(calls,1);assert.equal(f.kernel.proxy,f.proxy);assert.equal(f.kernel.client,f.client);
 assert.equal(f.kernel.vm.vmId,'vm-fixture');assert.equal(f.restores(),0);assert.equal(f.cleared(),0);
});

test('concurrent NativeKernel disposal waits for one confirmed teardown before clearing ownership',async()=>{
 const f=fixture();const gate=deferred();let calls=0,settled=0;
 f.proxy.dispose=()=>{calls++;return gate.promise;};
 const first=f.kernel.dispose(),second=f.kernel.dispose();
 first.then(()=>settled++,()=>settled++);second.then(()=>settled++,()=>settled++);
 await new Promise(resolve=>setImmediate(resolve));assert.equal(calls,1);assert.equal(settled,0);
 gate.resolve();await Promise.all([first,second]);assert.equal(f.kernel.proxy,null);assert.equal(f.kernel.vm,null);
 assert.equal(f.restores(),1);assert.equal(f.cleared(),1);await f.kernel.dispose();assert.equal(calls,1);
});

test('filesystem sync rejection stays visible after confirmed VM disposal and is stable on repeat',async()=>{
 const f=fixture();const failure=new Error('injected filesystem sync failure');let calls=0;
 f.kernel.options.syncFilesystemOnDispose=true;
 f.kernel.liveFilesystemBinding.syncFromLive=async()=>{throw failure;};
 f.proxy.dispose=async()=>{calls++;};
 await assert.rejects(f.kernel.dispose(),error=>error===failure);
 await assert.rejects(f.kernel.dispose(),error=>error===failure);
 assert.equal(calls,1);assert.equal(f.kernel.proxy,null);assert.equal(f.restores(),1);
});


test('actual fixture proxy shares one pending teardown and retains its settled rejection',async()=>{
 const {SidecarKernelProxy}=fixture();const gate=deferred();let calls=0,closes=0;
 const client={disposeVm(){calls++;return gate.promise;},async dispose(){closes++;},waitForEvent(_filter,_unused,{signal}){return new Promise((_resolve,reject)=>signal.addEventListener('abort',()=>reject(new Error('aborted')),{once:true}));}};
 const proxy=new SidecarKernelProxy({client,session:{},vm:{vmId:'vm-fixture'},env:{},cwd:'/workspace',localMounts:[],commandGuestPaths:new Map()});
 const first=proxy.dispose(),second=proxy.dispose();
 const failure=new Error('injected unconfirmed proxy teardown');
 const outcomes=Promise.allSettled([first,second]);
 await new Promise(resolve=>setImmediate(resolve));assert.equal(calls,1);
 gate.reject(failure);const results=await outcomes;
 for(const result of results){assert.equal(result.status,'rejected');assert.equal(result.reason,failure);}
 await assert.rejects(proxy.dispose(),error=>error===failure);
 assert.equal(calls,1);assert.equal(closes,1);assert.equal(first,second);
});

test('NativeKernel preserves typed deletion failure after confirmed final owning-client drain',async()=>{
 const f=fixture();let deletes=0,closes=0;
 const failure=Object.assign(new Error('injected typed teardown failure'),{code:'timeout',operation:'vm.dispose'});
 const client={disposeVm:async()=>{deletes++;throw failure;},async dispose(){closes++;},setVmSidecarRequestHandler(){},waitForEvent(_filter,_unused,{signal}){return new Promise((_resolve,reject)=>signal.addEventListener('abort',()=>reject(new Error('aborted')),{once:true}));}};
 const proxy=new f.SidecarKernelProxy({client,session:{},vm:{vmId:'vm-fixture'},env:{},cwd:'/workspace',localMounts:[],commandGuestPaths:new Map()});
 f.kernel.proxy=proxy;f.kernel.client=client;
 await assert.rejects(f.kernel.dispose(),error=>error===failure);
 await assert.rejects(f.kernel.dispose(),error=>error===failure);
 await assert.rejects(proxy.dispose(),error=>error===failure);
 assert.equal(deletes,1);assert.equal(closes,2);assert.equal(f.kernel.proxy,null);assert.equal(f.kernel.client,null);
 assert.equal(f.restores(),1);
});

test('explicit repeat disposal retries final owned-child drain without replaying VM deletion',async()=>{
 const f=fixture();let deletes=0,closes=0;
 const failure=Object.assign(new Error('injected VM deletion failure'),{code:'timeout',operation:'vm.dispose'});
 const client={async disposeVm(){deletes++;throw failure;},async dispose(){closes++;if(closes<2)throw new Error('injected child exit unconfirmed');},setVmSidecarRequestHandler(){},waitForEvent(_filter,_unused,{signal}){return new Promise((_resolve,reject)=>signal.addEventListener('abort',()=>reject(new Error('aborted')),{once:true}));}};
 const proxy=new f.SidecarKernelProxy({client,session:{},vm:{vmId:'vm-fixture'},env:{},cwd:'/workspace',localMounts:[],commandGuestPaths:new Map()});
 f.kernel.proxy=proxy;f.kernel.client=client;
 await assert.rejects(f.kernel.dispose(),error=>error===failure);
 assert.equal(closes,1);assert.equal(f.kernel.client,client);assert.equal(f.restores(),0);
 await assert.rejects(f.kernel.dispose(),error=>error===failure);
 assert.equal(deletes,1);assert.equal(closes,2);assert.equal(f.kernel.client,null);assert.equal(f.kernel.vm,null);assert.equal(f.restores(),1);
 await assert.rejects(f.kernel.dispose(),error=>error===failure);assert.equal(deletes,1);assert.equal(closes,2);
});

test('concurrent final child-drain retries share the pending attempt and preserve the original VM error',async()=>{
 const f=fixture();let deletes=0,closes=0;const gate=deferred();
 const failure=new Error('injected non-idempotent VM deletion failure');
 const client={async disposeVm(){deletes++;throw failure;},async dispose(){closes++;if(closes===1)throw new Error('injected initial child exit unconfirmed');if(closes===2)await gate.promise;},setVmSidecarRequestHandler(){},waitForEvent(_filter,_unused,{signal}){return new Promise((_resolve,reject)=>signal.addEventListener('abort',()=>reject(new Error('aborted')),{once:true}));}};
 const proxy=new f.SidecarKernelProxy({client,session:{},vm:{vmId:'vm-fixture'},env:{},cwd:'/workspace',localMounts:[],commandGuestPaths:new Map()});
 f.kernel.proxy=proxy;f.kernel.client=client;
 await assert.rejects(f.kernel.dispose(),error=>error===failure);
 const first=f.kernel.dispose(),second=f.kernel.dispose();const outcomes=Promise.allSettled([first,second]);
 await new Promise(resolve=>setImmediate(resolve));assert.equal(closes,2);
 gate.reject(new Error('injected repeated child exit unconfirmed'));
 const results=await outcomes;for(const result of results){assert.equal(result.status,'rejected');assert.equal(result.reason,failure);}
 assert.equal(deletes,1);assert.equal(f.kernel.client,client);assert.equal(f.restores(),0);
 await assert.rejects(f.kernel.dispose(),error=>error===failure);assert.equal(closes,3);assert.equal(deletes,1);assert.equal(f.kernel.client,null);
});

test('repeated failed VM disposal never drains an externally owned peer sidecar',async()=>{
 const f=fixture();let closes=0,calls=0;const failure=new Error('injected shared VM deletion failure');
 f.client.dispose=async()=>{closes++;};f.kernel.options.sidecar=f.client;
 f.proxy.dispose=async()=>{calls++;throw failure;};
 await assert.rejects(f.kernel.dispose(),error=>error===failure);await assert.rejects(f.kernel.dispose(),error=>error===failure);
 assert.equal(closes,0);assert.equal(calls,1);assert.equal(f.kernel.client,f.client);
});

test('confirmed VM deletion still retains an unconfirmed owning child until explicit final drain',async()=>{
 const f=fixture();let deletes=0,closes=0;
 const failure=Object.assign(new Error('injected child exit remains unconfirmed'),{code:'ERR_AGENTOS_SIDECAR_EXIT_UNCONFIRMED'});
 const client={async disposeVm(){deletes++;},async dispose(){closes++;if(closes===1)throw failure;},setVmSidecarRequestHandler(){},waitForEvent(_filter,_unused,{signal}){return new Promise((_resolve,reject)=>signal.addEventListener('abort',()=>reject(new Error('aborted')),{once:true}));}};
 const proxy=new f.SidecarKernelProxy({client,session:{},vm:{vmId:'vm-fixture'},env:{},cwd:'/workspace',localMounts:[],commandGuestPaths:new Map()});
 f.kernel.proxy=proxy;f.kernel.client=client;
 await assert.rejects(f.kernel.dispose(),error=>error===failure);assert.equal(f.kernel.client,client);assert.equal(closes,1);assert.equal(f.restores(),0);
 await assert.rejects(f.kernel.dispose(),error=>error===failure);assert.equal(f.kernel.client,null);assert.equal(closes,2);assert.equal(deletes,1);assert.equal(f.restores(),1);
});


test('disposal fences mount admission waiting on existing initialization',async()=>{
 const f=fixture(),gate=deferred();f.kernel.readyPromise=gate.promise;let admitted=0;
 const mount=f.kernel.mount({kind:'node',commands:[],async init(){admitted++;}});
 const result=assert.rejects(mount,error=>error.code==='ERR_AGENTOS_KERNEL_DISPOSED');
 const disposal=f.kernel.dispose();gate.resolve();
 await Promise.all([result,disposal]);assert.equal(admitted,0);assert.equal(f.restores(),1);
});
