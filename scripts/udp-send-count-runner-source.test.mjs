import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import {test} from 'node:test';

const source=readFileSync(process.env.AGENTOS_RUNNER_SOURCE??new URL('../crates/executor-v8-runtime/assets/runners/wasm-runner.mjs',import.meta.url),'utf8');
function extractFunction(name){const start=source.indexOf(`function ${name}(`);assert.ok(start>=0,name);return source.slice(start,source.indexOf('\n}\n',start)+3);}
function extractMethod(name){const start=source.indexOf(`  ${name}(`);assert.ok(start>=0,name);return source.slice(start,source.indexOf('\n  },',start)+5);}
function harness(reply,{kernel=false,error}={}){
 const memory=new WebAssembly.Memory({initial:1});
 new Uint8Array(memory.buffer).set([0,255,97],8);
 new DataView(memory.buffer).setUint32(64,0xfeedbeef,true);
 const calls=[];
 const context={WebAssembly,DataView,Uint8Array,Number,Buffer,instanceMemory:memory,
  WASI_ERRNO_SUCCESS:0,WASI_ERRNO_FAULT:21,WASI_ERRNO_BADF:8,
  unixConnectTimeoutMs:1000,HOST_NET_MSG_DONTWAIT:0x40,
  getHostNetSocket:()=>kernel?null:{managed:true,guestFd:7,targetFd:19},
  lookupFdHandle:()=>kernel?{kind:'kernel-fd',targetFd:23}:{kind:'host-socket'},
  callSyncRpc:(method,args)=>{calls.push({method,args});if(error)throw error;return reply;},
  mapHostProcessError:failure=>failure.code==='EACCES'?2:21,
  execError:(code,message)=>Object.assign(new Error(message),{code}),
 };
 vm.runInNewContext(['writeGuestUint32','guestRangeIsValid','guestRangesAreValid','readGuestBytes','managedHostNetTargetFd'].map(extractFunction).join('\n')+'\nglobalThis.api={'+extractMethod('net_send')+'};',context);
 return {send:(pointer=8,length=3,resultPointer=64)=>context.api.net_send(7,pointer,length,0,resultPointer),count:()=>new DataView(memory.buffer).getUint32(64,true),calls};
}

test('managed connected UDP returns the actual object byte count and preserves binary payload',()=>{
 const h=harness({bytes:3,localAddress:'127.0.0.1',localPort:1024});
 assert.equal(h.send(),0);assert.equal(h.count(),3);
 assert.equal(h.calls.length,1);assert.equal(h.calls[0].method,'process.hostnet_send');
 assert.equal(h.calls[0].args[0],19);assert.deepEqual([...h.calls[0].args[1]],[0,255,97]);
 assert.equal(h.calls[0].args[3],null);
});
test('zero-length connected UDP still submits a datagram and returns zero',()=>{const h=harness({bytes:0});assert.equal(h.send(8,0),0);assert.equal(h.count(),0);assert.equal(h.calls.length,1);});
test('managed TCP preserves numeric short-write counts',()=>{const h=harness(2);assert.equal(h.send(),0);assert.equal(h.count(),2);});
test('kernel fd send preserves numeric count and routing',()=>{const h=harness(3,{kernel:true});assert.equal(h.send(),0);assert.equal(h.count(),3);assert.equal(h.calls[0].method,'process.fd_sendmsg_rights');assert.equal(h.calls[0].args[0],23);});
test('host failure does not overwrite the output count',()=>{const h=harness(null,{error:{code:'EACCES'}});assert.equal(h.send(),2);assert.equal(h.count(),0xfeedbeef);});
test('invalid guest input or output bounds fail before host submission',()=>{
 for(const args of [[65535,3,64],[8,3,65535]]){const h=harness({bytes:3});assert.equal(h.send(...args),21);assert.equal(h.calls.length,0);assert.equal(h.count(),0xfeedbeef);}
});
