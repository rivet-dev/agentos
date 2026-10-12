import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";
import { forkStdinProbe } from "./fixtures/fork-stdin-probes.js";

test("fork IPC remains usable when the child's ordinary stdin is ignored", async () => {
  const source = `
import {fork} from 'node:child_process';
import {mkdtempSync,writeFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
const directory=mkdtempSync(tmpdir()+'/fork-ipc-');
const path=directory+'/child.mjs';
writeFileSync(path,"process.on('message', message=>{process.send({echo:message.value});process.exit(0);});");
const rows=[];
try {
 for(const mode of ['ignore','pipe']) {
  const child=fork(path,[],{execArgv:[],stdio:[mode,'pipe','pipe','ipc']});
  let reply=null,errors=[];
  child.on('message',message=>{reply=message;});
  child.on('error',error=>errors.push(error.code??error.message));
  const timer=setTimeout(()=>child.kill('SIGKILL'),1200);
  const closed=new Promise(resolve=>child.once('close',(code,signal)=>resolve({code,signal})));
  child.send({value:'early-message'});
  const exit=await closed;clearTimeout(timer);
  rows.push({mode,reply,errors,exit});
 }
} finally {rmSync(directory,{recursive:true,force:true});}
console.log(JSON.stringify(rows));`;
  const host = spawnSync(process.execPath,["--input-type=module","-e",source],{encoding:"utf8",timeout:10_000});
  expect(host.status,host.stderr).toBe(0);
  const expected = JSON.parse(host.stdout);
  expect(expected.every((row: {reply:unknown}) => row.reply !== null)).toBe(true);
  const sidecar = await AgentOs.createSidecar();
  let vm: AgentOs | undefined;
  try {
    vm = await AgentOs.create({sidecar:{kind:"explicit",handle:sidecar},defaultSoftware:false,permissions:{network:"deny",childProcess:"allow"}});
    let stdout="",stderr="";
    const proc = await vm.process.spawn("node",["--input-type=module","-e",source],{timeoutMs:10_000,onStdout:c=>{stdout+=Buffer.from(c).toString();},onStderr:c=>{stderr+=Buffer.from(c).toString();}});
    expect((await vm.process.wait(proc.pid)).exitCode,stderr).toBe(0);
    expect(JSON.parse(stdout)).toEqual(expected);
  } finally {try {await vm?.dispose();} finally {await sidecar.dispose();}}
},20_000);

for(const mode of ["ignore","pipe"] as const) test(`fork public stdin ${mode} preserves ordinary EOF and IPC isolation`,async()=>{
  const source=forkStdinProbe(mode);
  const host=spawnSync(process.execPath,["--input-type=module","-e",source],{encoding:"utf8",timeout:10_000,maxBuffer:65536});
  expect(host.status,host.stderr).toBe(0);
  const expected=JSON.parse(host.stdout);
  expect(expected.every((row:{timedOut:boolean;exit:{code:number}})=>!row.timedOut&&row.exit.code===0)).toBe(true);
  const sidecar=await AgentOs.createSidecar();let vm:AgentOs|undefined;
  const failures:unknown[]=[];
  try {
    vm=await AgentOs.create({sidecar:{kind:"explicit",handle:sidecar},defaultSoftware:false,permissions:{network:"deny",childProcess:"allow"}});
    let stdout="",stderr="";
    const proc=await vm.process.spawn("node",["--input-type=module","-e",source],{timeoutMs:10_000,onStdout:c=>{stdout+=Buffer.from(c).toString();if(Buffer.byteLength(stdout)>65536)throw new Error("probe stdout limit exceeded");},onStderr:c=>{stderr+=Buffer.from(c).toString();if(Buffer.byteLength(stderr)>65536)throw new Error("probe stderr limit exceeded");}});
    const exit=await vm.process.wait(proc.pid);
    console.log(JSON.stringify({kind:"fork-stdin-observation",mode,node:expected,guest:{stdout,stderr,exit}}));
    expect(exit.exitCode,stderr).toBe(0);
    expect(JSON.parse(stdout)).toEqual(expected);
  } catch(error){failures.push(error);}
  finally {
    for(const cleanup of [()=>vm?.dispose(),()=>sidecar.dispose()])try{await cleanup();}catch(error){failures.push(error);}
  }
  if(failures.length)throw new AggregateError(failures,failures.map(error=>String(error instanceof Error?error.stack??error.message:error)).join("\n\n"));
},20_000);
