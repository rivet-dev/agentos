export function forkIpcOwnershipProbe(
	mode: "direct" | "disconnect" | "nested" | "context",
): string {
	if (mode === "context")
		return `
import {fork} from 'node:child_process';
import {AsyncLocalStorage} from 'node:async_hooks';
import {mkdtempSync,writeFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
const storage=new AsyncLocalStorage(),contexts={message:[],send:[],disconnect:[]},errors=[];
const directory=mkdtempSync(tmpdir()+'/fork-context-'),path=directory+'/child.mjs';
writeFileSync(path,"process.on('message',()=>process.send({kind:'reply'},()=>{process.disconnect();process.exit(0);}));process.send({kind:'ready'});");
try {
 const child=storage.run('creator',()=>fork(path,[],{execArgv:[],stdio:['ignore','pipe','pipe','ipc']}));
 let stderr='',timedOut=false;
 child.stdout.resume();child.stderr.on('data',chunk=>{stderr+=chunk.toString();if(Buffer.byteLength(stderr)>4096)child.kill('SIGKILL');});
 child.on('error',error=>errors.push({code:error.code??null,message:error.message}));
 child.on('message',message=>{
  contexts.message.push(storage.getStore()??null);
  if(message.kind==='ready')storage.run('sender',()=>child.send({kind:'request'},error=>{
   contexts.send.push(storage.getStore()??null);
   if(error)errors.push({code:error.code??null,message:error.message});
  }));
 });
 child.on('disconnect',()=>contexts.disconnect.push(storage.getStore()??null));
 const closed=new Promise(resolve=>child.once('close',(code,signal)=>resolve({code,signal})));
 const timer=setTimeout(()=>{timedOut=true;child.kill('SIGKILL');},1200);
 let exit;try{exit=await closed;}finally{clearTimeout(timer);}
 console.log(JSON.stringify({contexts,errors,stderr,timedOut,exit}));
}finally{rmSync(directory,{recursive:true,force:true});}`;
	const prefix =
		"\x1eAGENTOS_IPC:" +
		Buffer.from(JSON.stringify({ ordinary: true })).toString("base64") +
		"\n";
	const child =
		mode === "direct"
			? `
import {read,closeSync} from 'node:fs';
const buffers=[];
process.stdout.write(${JSON.stringify(prefix)});
function pull(){read(0,Buffer.alloc(8),0,8,null,(error,size,buffer)=>{
 if(error){process.send({kind:'read-error',code:error.code,message:error.message});return;}
 if(size){buffers.push(Buffer.from(buffer.subarray(0,size)));pull();}
 else {closeSync(0);process.send({kind:'stdin-end',data:Buffer.concat(buffers).toString('base64')});}
});}
process.on('message',message=>{if(message.kind==='first')process.send({kind:'first-reply'});if(message.kind==='second')process.send({kind:'second-reply'},()=>process.exit(0));});
pull();process.send({kind:'ready'});
`
			: mode === "disconnect"
				? `
const buffers=[];
process.stdin.on('data',chunk=>buffers.push(Buffer.from(chunk)));
process.stdin.on('end',()=>{process.stdout.write(JSON.stringify({data:Buffer.concat(buffers).toString('base64')})+'\\n');process.exit(0);});
process.stdin.resume();
process.on('message',message=>{if(message.kind==='first')process.send({kind:'first-reply'},()=>process.disconnect());});
process.send({kind:'ready'});
`
				: `
import {fork} from 'node:child_process';
import {writeFileSync} from 'node:fs';
const path=process.argv[1]+'.grandchild.mjs';
writeFileSync(path,"process.on('message',message=>process.send({value:message.value},()=>process.exit(0)));process.send({kind:'grand-ready'});");
const child=fork(path,[],{execArgv:[],stdio:['ignore','pipe','pipe','ipc']});
child.on('error',error=>process.send({kind:'nested-error',code:error.code,message:error.message}));
child.on('message',message=>{if(message.kind==='grand-ready')process.send({kind:'ready'});else process.send({kind:'nested-reply',value:message.value},()=>process.exit(0));});
process.on('message',message=>child.send(message));
`;
	return `
import {fork} from 'node:child_process';import {mkdtempSync,writeFileSync,rmSync} from 'node:fs';import {tmpdir} from 'node:os';
const directory=mkdtempSync(tmpdir()+'/fork-owner-'),path=directory+'/child.mjs';writeFileSync(path,${JSON.stringify(child)});
const messages=[],errors=[];let stdout='',stderr='',timedOut=false,disconnected=false;
const describe=error=>({code:error.code??null,message:error.message});
try {
 const child=fork(path,[],{execArgv:[],stdio:['pipe','pipe','pipe','ipc']});
 child.stdout.on('data',chunk=>{stdout+=chunk.toString();if(Buffer.byteLength(stdout)>4096)child.kill('SIGKILL');});
 child.stderr.on('data',chunk=>{stderr+=chunk.toString();if(Buffer.byteLength(stderr)>4096)child.kill('SIGKILL');});
 child.on('error',error=>errors.push(describe(error)));child.stdin.on('error',error=>errors.push(describe(error)));
 const send=message=>child.send(message,error=>{if(error)errors.push(describe(error));});
 child.on('message',message=>{messages.push(message);
  if(message.kind==='ready')send(${JSON.stringify(mode)}==='nested'?{value:'é雪🟠'}:{kind:'first'});
  if(message.kind==='first-reply'&&${JSON.stringify(mode)}==='direct'){child.stdin.write('USER');child.stdin.end();}
  if(message.kind==='stdin-end')send({kind:'second'});
 });
 child.on('disconnect',()=>{disconnected=true;if(${JSON.stringify(mode)}==='disconnect'){child.stdin.write('USER');child.stdin.end();}});
 const closed=new Promise(resolve=>child.once('close',(code,signal)=>resolve({code,signal})));
 const timer=setTimeout(()=>{timedOut=true;child.kill('SIGKILL');},1200);
 let exit;try{exit=await closed;}finally{clearTimeout(timer);}
 console.log(JSON.stringify({messages,errors,stdout,stderr,timedOut,disconnected,exit}));
}finally{rmSync(directory,{recursive:true,force:true});}`;
}
