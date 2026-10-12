export function forkStdinProbe(mode:"ignore"|"pipe"):string {
  const child=mode==="ignore" ? `
import {read} from 'node:fs';
const buffers=[];let ended=false;
process.stdin.on('data',chunk=>{if(buffers.reduce((n,b)=>n+b.length,0)<256)buffers.push(Buffer.from(chunk).subarray(0,256));});
process.stdin.on('end',()=>{ended=true;});
process.stdin.on('error',error=>process.send({kind:'stdin-error',name:error.name,code:error.code??null,message:error.message}));
process.stdin.resume();
process.on('message',message=>{if(message.kind==='ping')setTimeout(()=>{process.send({kind:'result',data:Buffer.concat(buffers).toString('base64'),ended});process.exit(0);},30);});
if(process.argv[2]==='read') {
 read(0,Buffer.alloc(1),0,1,null,(error,bytesRead)=>{process.send({kind:'read',bytesRead,error:error?{name:error.name,code:error.code??null,message:error.message}:null});process.send({kind:'ready'});});
} else process.send({kind:'ready'});
` : `
const buffers=[];
process.stdin.on('data',chunk=>{if(buffers.reduce((n,b)=>n+b.length,0)<256)buffers.push(Buffer.from(chunk).subarray(0,256));});
process.stdin.on('end',()=>process.send({kind:'stdin-end',data:Buffer.concat(buffers).toString('base64')}));
process.stdin.on('error',error=>process.send({kind:'stdin-error',name:error.name,code:error.code??null,message:error.message}));
process.stdin.resume();
process.on('message',message=>{
 if(message.kind==='first')setImmediate(()=>process.send({kind:'first-reply'}));
 if(message.kind==='second'){process.send({kind:'second-reply'});process.exit(0);}
});
process.send({kind:'ready'});
`;
  return `
import {fork} from 'node:child_process';
import {mkdtempSync,writeFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
const directory=mkdtempSync(tmpdir()+'/fork-stdin-'),path=directory+'/child.mjs';
writeFileSync(path,${JSON.stringify(child)});
const rows=[];
try {
 for(const subcase of ${JSON.stringify(mode==="ignore"?["public","read"]:["pipe"])}) {
  const child=fork(path,subcase==='read'?['read']:[],{execArgv:[],stdio:[${JSON.stringify(mode)},'pipe','pipe','ipc']});
  const messages=[],errors=[];let timedOut=false,stderr='';
  child.stderr.on('data',chunk=>{stderr=(stderr+chunk.toString()).slice(0,1024);});
  const describe=error=>({name:error.name,code:error.code??null,message:error.message});
  child.on('error',error=>errors.push(describe(error)));
  child.stdin?.on('error',error=>errors.push(describe(error)));
  const send=message=>{try{child.send(message,error=>{if(error)errors.push(describe(error));});}catch(error){errors.push(describe(error));}};
  child.on('message',message=>{
   messages.push(message);
   if(message.kind==='ready') {
    if(${JSON.stringify(mode)}==='ignore')send({kind:'ping'});
    else {send({kind:'first'});child.stdin.write('USER');}
   }
   if(message.kind==='first-reply')child.stdin.end();
   if(message.kind==='stdin-end')send({kind:'second'});
  });
  const closed=new Promise(resolve=>child.once('close',(code,signal)=>resolve({code,signal})));
  const timer=setTimeout(()=>{timedOut=true;child.kill('SIGKILL');},1200);
  let exit;try{exit=await closed;}finally{clearTimeout(timer);}
  rows.push({subcase,stdin:child.stdin===null?'null':'stream',messages,errors,timedOut,stderr,exit});
 }
} finally {rmSync(directory,{recursive:true,force:true});}
console.log(JSON.stringify(rows));`;
}
