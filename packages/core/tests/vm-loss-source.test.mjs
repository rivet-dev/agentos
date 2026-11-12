import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import { transform } from './source-transform.mjs';
const repoSource=process.env.AGENTOS_CORE_SOURCE ?? new URL('../src',import.meta.url).pathname;
const {code:errorCode}=await transform(await readFile(`${repoSource}/sidecar-errors.ts`,'utf8'),{loader:'ts',format:'cjs'});
const errors={module:{exports:{}},Error};vm.runInNewContext(errorCode,errors);
const {SidecarProcessExited}=errors.module.exports;
const {code}=await transform(await readFile(new URL('../src/sidecar-process.ts',import.meta.url),'utf8'),{loader:'ts',format:'cjs'});
function fixture(){
 const context={module:{exports:{}},Error,Promise,require(name){if(name==='./sidecar-errors.js')return errors.module.exports;return {};}};
 vm.runInNewContext(code,context);let closed,requests=0;
 const process=context.module.exports.SidecarProcess.fromClient({onClose(handler){closed=handler;return()=>{};},setSidecarRequestHandler(){},async sendRequest(){requests++;return {payload:{type:'vm_disposed'}};}});
 return {process,close:error=>closed(error),requests:()=>requests};
}
const session={connectionId:'connection-test',sessionId:'session-test'},guest={vmId:'vm-test'};
test('injected generic transport loss is not proof of VM termination',async()=>{
 const {process,close,requests}=fixture();const error=new Error('injected connection lost');close(error);
 await assert.rejects(process.disposeVm(session,guest),reason=>reason===error);assert.equal(requests(),0);
});
test('typed disconnect without exit status is not proof of VM termination',async()=>{
 const {process,close}=fixture();const error=new SidecarProcessExited({exitCode:null,signal:null,stderr:''});close(error);
 await assert.rejects(process.disposeVm(session,guest),reason=>reason===error);
});
test('confirmed death of this transport generation permits local VM cleanup without replay',async()=>{
 for(const status of [{exitCode:17,signal:null},{exitCode:null,signal:'SIGKILL'}]) {
  const {process,close,requests}=fixture();close(new SidecarProcessExited({...status,stderr:''}));
  await process.disposeVm(session,guest);assert.equal(requests(),0);
 }
});
