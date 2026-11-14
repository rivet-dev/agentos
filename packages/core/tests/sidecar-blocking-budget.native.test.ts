import {expect,test} from 'vitest';
import {AgentOs} from '../src/index.js';

test('explicit blocking capacity reaches native startup and permits a real guest',async()=>{
 const sidecar=await AgentOs.createSidecar({runtime:{blocking:{maxQueuedBytes:256*1024*1024}}});let vm:AgentOs|undefined;
 try{vm=await AgentOs.create({sidecar:{kind:'explicit',handle:sidecar},defaultSoftware:false,permissions:{network:'deny'}});let stdout='';const child=await vm.process.spawn('node',['-e',`console.log(require('node:crypto').createHash('sha256').update('guest').digest('hex'))`],{timeoutMs:5000,onStdout:b=>stdout+=Buffer.from(b).toString()});expect((await vm.process.wait(child.pid)).exitCode).toBe(0);expect(stdout.trim()).toBe('84983c60f7daadc1cb8698621f802c0d9f9a3c3c295c810748fb048115c186ec');}finally{await vm?.dispose();await sidecar.dispose();}
},12000);

test('blocking capacity rejects a snapshot larger than its budget without changing the default',async()=>{
 const sidecar=await AgentOs.createSidecar({runtime:{blocking:{maxQueuedBytes:1}}});let vm:AgentOs|undefined;
 try{await expect((async()=>{let stderr='';vm=await AgentOs.create({sidecar:{kind:'explicit',handle:sidecar},defaultSoftware:false,permissions:{network:'deny'}});const child=await vm.process.spawn('node',['-e','console.log(1)'],{timeoutMs:5000,onStderr:b=>stderr+=Buffer.from(b).toString()});const result=await vm.process.wait(child.pid);if(result.exitCode!==0)throw Error(stderr+JSON.stringify(result));})()).rejects.toThrow(/executorBytes|maxQueuedBytes/);}finally{await vm?.dispose();await sidecar.dispose();}
},12000);


test('shared sidecars reject a different blocking budget and invalid trusted input',async()=>{
 const pool='blocking-budget-'+crypto.randomUUID(),runtime={blocking:{maxQueuedBytes:128*1024*1024}};
 const owner=await AgentOs.getSharedSidecar({pool,runtime});
 try{expect(await AgentOs.getSharedSidecar({pool,runtime})).toBe(owner);await expect(AgentOs.getSharedSidecar({pool,runtime:{blocking:{maxQueuedBytes:256*1024*1024}}})).rejects.toThrow(/runtime|configuration/i);await expect(AgentOs.createSidecar({runtime:{blocking:{maxQueuedBytes:0}}})).rejects.toThrow(/positive/);}finally{await owner.dispose();}
},10000);
