import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {test} from 'node:test';
import vm from 'node:vm';
const source=await readFile(process.env.AGENTOS_RUNNER_SOURCE ?? new URL('../crates/executor-v8-runtime/assets/runners/wasm-runner.mjs',import.meta.url),'utf8');
const fn=name=>{const start=source.indexOf(`function ${name}(`);return start<0?'':source.slice(start,source.indexOf('\n}\n',start)+3);};
const method=name=>{const start=source.indexOf(`  ${name}(`);return source.slice(start,source.indexOf('\n  },',start)+5);};
function fixture({capacity=8,length=32,managed=true,type=2,socketPresent=true,validFd=false}={}) {
 const memory=new WebAssembly.Memory({initial:1});const bytes=new Uint8Array(memory.buffer);bytes.fill(0xa5);
 const payload=Buffer.from(Array.from({length},(_,i)=>i+1));let calls=0,flags;
 const event={type:'message',data:{base64:payload.subarray(0,managed?capacity:length).toString('base64')},fullLength:length,remoteAddress:'127.0.0.1',remotePort:1234};
 const socket=managed?{managed:true,guestFd:7,targetFd:7}:{managed:false,sockType:type===1?6:5,nonblock:true};
 const context={Buffer,WebAssembly,DataView,Uint8Array,Number,Date,instanceMemory:memory,
 WASI_ERRNO_NOTSOCK:57,WASI_ERRNO_FAULT:21,WASI_ERRNO_SUCCESS:0,WASI_ERRNO_BADF:8,WASI_ERRNO_INVAL:28,WASI_ERRNO_AGAIN:6,
 HOST_NET_MSG_TRUNC:32,HOST_NET_MSG_PEEK:2,HOST_NET_MSG_DONTWAIT:64,HOST_NET_SOCK_DGRAM:5,HOST_NET_SOCK_STREAM:6,POSIX_SOCK_DGRAM:2,POSIX_SOCK_STREAM:1,HOST_NET_SOCKET_TYPE_MASK:15,
 HOST_NET_SOL_SOCKET:1,HOST_NET_WASI_SOL_SOCKET:0x7fffffff,HOST_NET_SO_ERROR:4,
 getHostNetSocket:()=>socketPresent?socket:null,lookupFdHandle:()=>validFd?{kind:"kernel-fd"}:null,managedHostNetTargetFd:()=>7,managedHostNetOperationIsNonblocking:()=>true,
 callSyncRpc:(method,args)=>{calls++;if(method==='process.hostnet_get_option'){assert.deepEqual(Array.from(args),[7,'type']);return type;}assert.equal(method,'process.hostnet_recv');flags=args[2];return event;},
 ensureHostNetUdpSocket:()=>7,receiveHostNetDatagramEvent:(_socket,inputFlags)=>{calls++;flags=inputFlags;return event;},
 isHostNetWouldBlock:()=>false,formatHostNetAddressInfo:()=> '127.0.0.1:1234',
 readGuestUint32:ptr=>new DataView(memory.buffer).getUint32(ptr,true),mapHostProcessError:error=>{throw error;},
 };
 const helpers=['normalizeHostNetSocketType','hostNetSocketBaseType','hostNetDatagramBytes','hostNetDatagramFullLength','writeGuestUint32','guestRangeIsValid','guestRangesAreValid','writeGuestBytes'].map(fn).join('\n');
 vm.runInNewContext(helpers+'\nglobalThis.api={'+['net_recv','net_recvfrom','net_getsockopt'].map(method).join('\n')+'};',context);
 new DataView(memory.buffer).setUint32(104,32,true);
 return {api:context.api,memory,bytes,payload,calls:()=>calls,flags:()=>flags};
}
for(const managed of [true,false])for(const api of ['net_recv','net_recvfrom'])test(`actual ${api}, managed=${managed}: full length is independent of copied bytes`,()=>{
 for(const length of [0,32])for(const capacity of [0,8,32])for(const trunc of [0,32])for(const peek of [0,2]){
  const f=fixture({length,capacity,managed});const args=[7,16,capacity,64|trunc|peek,100];if(api==='net_recvfrom')args.push(128,104);
  assert.equal(f.api[api](...args),0);assert.equal(new DataView(f.memory.buffer).getUint32(100,true),trunc?length:Math.min(length,capacity));
  assert.deepEqual(Buffer.from(f.bytes.slice(16,16+Math.min(length,capacity))),f.payload.subarray(0,capacity));
  assert.equal(f.bytes[15],0xa5);assert.equal(f.bytes[16+Math.min(length,capacity)],0xa5);
  assert.equal(f.flags(),64|trunc|peek);
 }
});
test('actual receive preserves guest memory bounds before host consumption',()=>{
 for(const name of ['net_recv','net_recvfrom']){const f=fixture();const args=[7,65534,8,32,100];if(name==='net_recvfrom')args.push(128,104);assert.equal(f.api[name](...args),21);assert.equal(f.calls(),0);}
});
test('actual getsockopt exposes socket type needed by recvmsg fallback',()=>{
 for(const managed of [true,false])for(const type of [1,2]){const f=fixture({type,managed});new DataView(f.memory.buffer).setUint32(104,4,true);assert.equal(f.api.net_getsockopt(7,1,3,100,104),0);assert.equal(new DataView(f.memory.buffer).getInt32(100,true),type===1?6:5);}
});

test('socket type query distinguishes a valid nonsocket descriptor from an invalid fd',()=>{
 for(const validFd of [false,true]){const f=fixture({socketPresent:false,validFd});assert.equal(f.api.net_getsockopt(7,1,3,100,104),validFd?57:8);}
});

test('SO_TYPE copies only the requested prefix for lengths zero through five',()=>{
 for(const managed of [true,false])for(const level of [1,0x7fffffff])for(const type of [1,2])for(let capacity=0;capacity<=5;capacity++){
  const f=fixture({type,managed});const view=new DataView(f.memory.buffer);view.setUint32(104,capacity,true);
  assert.equal(f.api.net_getsockopt(7,level,3,80,104),0);
  const copied=Math.min(capacity,4);assert.equal(view.getUint32(104,true),copied);
  const expected=Buffer.alloc(4);expected.writeInt32LE(type===1?6:5);
  assert.deepEqual(Buffer.from(f.bytes.slice(80,80+copied)),expected.subarray(0,copied));
  assert.equal(f.bytes[79],0xa5);assert.equal(f.bytes[80+copied],0xa5);
 }
});
