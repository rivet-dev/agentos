import{spawnSync}from'node:child_process';import{expect,test}from'vitest';import{AgentOs}from'../src/index.js';
async function guest(source:string,gc=false){const sidecar=await AgentOs.createSidecar();let vm:AgentOs|undefined;try{vm=await AgentOs.create({sidecar:{kind:'explicit',handle:sidecar},defaultSoftware:false,permissions:{network:'deny'}});let stdout='',stderr='';const child=await vm.process.spawn('node',[...(gc?['--expose-gc']:[]),'--input-type=module','-e',source],{timeoutMs:10000,onStdout:b=>stdout+=Buffer.from(b).toString(),onStderr:b=>stderr+=Buffer.from(b).toString()});expect((await vm.process.wait(child.pid)).exitCode,stderr).toBe(0);return JSON.parse(stdout);}finally{await vm?.dispose();await sidecar.dispose();}}
async function compare(source:string,gc=false){const host=spawnSync(process.execPath,[...(gc?['--expose-gc']:[]),'--input-type=module','-e',source],{encoding:'utf8',timeout:10000});expect(host.status,host.stderr).toBe(0);expect(await guest(source,gc)).toEqual(JSON.parse(host.stdout));}

test('AbortSignal.any propagates through weak dependencies without installing public listeners',async()=>{
 await compare(`import{getEventListeners}from'node:events';const controller=new AbortController(),children=Array.from({length:1500},()=>AbortSignal.any([controller.signal]));let seen=0;for(const signal of children)signal.addEventListener('abort',()=>seen++);const listeners=getEventListeners(controller.signal,'abort').length;controller.signal.addEventListener('abort',event=>event.stopImmediatePropagation());controller.abort('stop');console.log(JSON.stringify({listeners,seen,aborted:children.every(s=>s.aborted&&s.reason==='stop')}));`);
},15000);

test('dependent abort signals ignore manual events and mark nested signals before delivery',async()=>{
 await compare(`const root=new AbortController(),a=AbortSignal.any([root.signal]),b=AbortSignal.any([a]),c=AbortSignal.any([root.signal,a]);root.signal.dispatchEvent(new Event('abort'));const manual=[a.aborted,b.aborted,c.aborted],order=[];for(const[name,signal]of[['root',root.signal],['a',a],['b',b],['c',c]])signal.addEventListener('abort',()=>order.push([name,a.aborted,b.aborted,c.aborted]));root.abort(7);console.log(JSON.stringify({manual,order,reasons:[a.reason,b.reason,c.reason]}));`);
},15000);

test('unused dependent signals collect while listeners keep an observed dependent alive',async()=>{
 await compare(`const root=new AbortController();let observed=0;const reference=(()=>{const signal=AbortSignal.any([root.signal]);return new WeakRef(signal);})();(()=>{AbortSignal.any([root.signal]).addEventListener('abort',()=>observed++);})();for(let n=0;n<15;n++){await new Promise(r=>setTimeout(r,5));gc();}await new Promise(r=>setTimeout(r,5));const unusedCollected=reference.deref()===undefined;root.abort();console.log(JSON.stringify({unusedCollected,observed}));`,true);
},15000);

test('signal composition retains explicit EventTarget and input-count admission limits',async()=>{
 expect(await guest(`const target=new EventTarget();for(let n=0;n<1024;n++)target.addEventListener('sample',()=>{});let listeners,inputs;try{target.addEventListener('sample',()=>{});}catch(e){listeners=e.code;}const controller=new AbortController();try{AbortSignal.any(Array(1025).fill(controller.signal));}catch(e){inputs=e.code;}console.log(JSON.stringify({listeners,inputs,usable:AbortSignal.any([controller.signal]).aborted===false}));`)).toEqual({listeners:'ERR_EVENT_LISTENER_LIMIT',inputs:'ERR_ABORT_SIGNAL_LIMIT',usable:true});
},15000);
