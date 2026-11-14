import {spawnSync} from 'node:child_process';
import {expect,test} from 'vitest';
import {AgentOs} from '../src/index.js';

test('explicit child environments replace parent values for spawn, spawnSync, exec and fork',async()=>{
 const source=`import {spawn,spawnSync,execSync,fork} from 'node:child_process';import{writeFileSync,unlinkSync}from'node:fs';
 process.env.OPENORANGE_ENV_PARENT='parent';
 const probe="console.log(JSON.stringify({parent:process.env.OPENORANGE_ENV_PARENT??null,child:process.env.CHILD_VALUE??null}))";
 const values=[];
 for(const env of [undefined,{}, {CHILD_VALUE:'child'}, {CHILD_VALUE:undefined}]){
  const sync=spawnSync(process.execPath,['-e',probe],{env,encoding:'utf8'});if(sync.status!==0)throw new Error(sync.stderr);values.push(JSON.parse(sync.stdout));
  const child=spawn(process.execPath,['-e',probe],{env});let out='';child.stdout.on('data',b=>out+=b);await new Promise((r,j)=>{child.once('error',j);child.once('close',c=>c===0?r():j(new Error('exit '+c)));});values.push(JSON.parse(out));
 }
 const shell=execSync('printf \\%s \\"$OPENORANGE_ENV_PARENT\\"',{env:{},encoding:'utf8'});values.push(shell);
 const path='/tmp/env-replacement-'+process.pid+'.cjs';writeFileSync(path,probe);
 try {const child=fork(path,[],{env:{CHILD_VALUE:'fork'},silent:true});let out='';child.stdout.on('data',b=>out+=b);await new Promise((r,j)=>{child.once('error',j);child.once('close',c=>c===0?r():j(new Error('fork '+c)));});values.push(JSON.parse(out));}finally{unlinkSync(path);}
 console.log(JSON.stringify(values));`;
 const host=spawnSync(process.execPath,['--input-type=module','-e',source],{encoding:'utf8',timeout:10000,maxBuffer:16384});expect(host.status,host.stderr).toBe(0);
 const sidecar=await AgentOs.createSidecar();let vm:AgentOs|undefined;
 try {vm=await AgentOs.create({sidecar:{kind:'explicit',handle:sidecar},permissions:{network:'deny'}});let stdout='',stderr='';const child=await vm.process.spawn('node',['--input-type=module','-e',source],{timeoutMs:15000,onStdout:b=>{stdout+=Buffer.from(b).toString();},onStderr:b=>{stderr+=Buffer.from(b).toString();}});expect((await vm.process.wait(child.pid)).exitCode,stderr).toBe(0);expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));}
 finally{try{await vm?.dispose();}finally{await sidecar.dispose();}}
},25000);
