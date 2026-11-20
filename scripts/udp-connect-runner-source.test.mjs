import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import {test} from 'node:test';
const source=readFileSync(process.env.AGENTOS_RUNNER_SOURCE??new URL('../crates/executor-v8-runtime/assets/runners/wasm-runner.mjs',import.meta.url),'utf8');
const fn=name=>{const start=source.indexOf(`function ${name}(`);assert.ok(start>=0);return source.slice(start,source.indexOf('\n}\n',start)+3);};
const method=name=>{const start=source.indexOf(`  ${name}(`);assert.ok(start>=0);return source.slice(start,source.indexOf('\n  },',start)+5);};
function run(raw,error){let sent;const context={Number,String,JSON,Buffer,
 getHostNetSocket:()=>({managed:true,guestFd:7,targetFd:19}),readGuestString:()=>raw,
 managedHostNetTargetFd:socket=>socket.targetFd,unixConnectTimeoutMs:1000,
 WASI_ERRNO_SUCCESS:0,WASI_ERRNO_ISCONN:30,WASI_ERRNO_INVAL:28,
 callSyncRpc:(method,args)=>{sent={method,args};if(error)throw error;return {};},mapHostProcessError:e=>e.code==='EACCES'?2:28,
 };vm.runInNewContext(['parseHostNetAddress','parseHostNetUnixAddress','managedHostNetAddressValue'].map(fn).join('\n')+'\nglobalThis.api={'+method('net_connect')+'};',context);
 return {errno:context.api.net_connect(7,0,raw.length),sent:sent&&JSON.parse(JSON.stringify(sent))};}
test('actual managed connect projection forwards AF_UNSPEC',()=>{const r=run('unspecified');assert.equal(r.errno,0);assert.deepEqual(r.sent,{method:'process.hostnet_connect',args:[19,{type:'unspecified'},1000]});});
test('actual managed connect preserves INET peer address',()=>{const r=run('127.0.0.1:1234');assert.equal(r.errno,0);assert.deepEqual(r.sent.args[1],{type:'inet',host:'127.0.0.1',port:1234});});
test('actual managed connect preserves host permission errno',()=>{assert.equal(run('127.0.0.1:1234',{code:'EACCES'}).errno,2);});
