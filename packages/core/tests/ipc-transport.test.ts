import {expect,test} from "vitest";
import {IpcTransport,IPC_FRAME_PREFIX} from "../../build-tools/bridge-src/builtins/ipc-transport.js";

const frame=(message:unknown)=>IPC_FRAME_PREFIX+Buffer.from(JSON.stringify(message)).toString("base64")+"\n";

function fixture() {
  const tasks:Array<()=>void>=[],timers=new Set<()=>void>(),writes:Buffer[]=[],errors:Error[]=[],messages:unknown[]=[],closed:string[]=[],refs=new Set<string>();
  const inputs=new Map<string,Array<Buffer|"eof">>(),targets:any[]=[],polls:any[]=[],updates:any[]=[];
  let pending:any,writeLimit=65536,once: number|undefined;
  const key=(target:any)=>target.childId??"self";
  const signal=()=>{
    if(!pending)return;
    const ready=pending.interests.filter((i:any)=>inputs.get(key(i))?.length||(i.writable&&writeLimit>0)).map((i:any)=>({...i,events:1}));
    if(ready.length){const old=pending;pending=undefined;old.resolve({ready,revision:String(old.revision)});}
  };
  const driver={Buffer,
    defer:(task:()=>void)=>tasks.push(task),
    delay:(task:()=>void,_ms:number)=>{timers.add(task);return task;},clearDelay:(task:()=>void)=>timers.delete(task),
    refWrite:(key:string)=>refs.add(key),unrefWrite:(key:string)=>refs.delete(key),
    read:(target:any,size:number)=>{targets.push(target);const input=inputs.get(key(target));const chunk=input?.shift();if(chunk==="eof")return {done:true};if(!chunk)return null;const head=chunk.subarray(0,size);if(head.length<chunk.length)input!.unshift(chunk.subarray(head.length));return {dataBase64:head.toString("base64")};},
    write:(target:any,chunk:Uint8Array)=>{targets.push(target);const written=Math.min(once??writeLimit,chunk.length);once=undefined;if(written)writes.push(Buffer.from(chunk.subarray(0,written)));return {written};},
    close:(target:any)=>{targets.push(target);closed.push(key(target));if(pending){const old=pending;pending=undefined;old.resolve({ready:[{...target,events:32}],revision:String(old.revision)});}},
    poll:(id:string,interests:any[])=>{expect(pending).toBeUndefined();polls.push({id,interests});return new Promise(resolve=>{pending={id,interests,revision:0,resolve};signal();});},
    update:(id:string,revision:string,interests:any[])=>{
      if(!pending||pending.id!==id||String(pending.revision)!==revision)throw Object.assign(new Error("poll completed"),{code:"ESTALE"});
      updates.push({id,revision,interests});pending.interests=interests;pending.revision++;const result={revision:String(pending.revision)};signal();return result;
    },
  };
  const transport=new IpcTransport(driver);
  const open=(childId:string|null=null,generation="17")=>transport.open(childId,{generation,serialization:"json"},{drainMs:1000,message:(payload:string)=>messages.push(JSON.parse(Buffer.from(payload,"base64").toString())),error:(error:Error)=>errors.push(error),close:()=>{},});
  const flush=async()=>{for(let turn=0;turn<5000;turn++){await Promise.resolve();const task=tasks.shift();if(task){task();continue;}await Promise.resolve();if(!tasks.length)return;}throw new Error("finite transport control did not quiesce");};
  return {transport,open,flush,tasks,timers,writes,errors,messages,closed,refs,polls,updates,targets,
    feed:(id:string,bytes:Array<Buffer|"eof">)=>{inputs.set(id,bytes);signal();},
    writable:(limit:number)=>{writeLimit=limit;signal();},
    writeOnce:(limit:number)=>{once=limit;writeLimit=0;},
    expire:()=>{for(const task of [...timers]){timers.delete(task);task();}},
  };
}

test("idle IPC has one parked poll and no periodic deadline; send mutates its interests",async()=>{
  const f=fixture();f.open();await f.flush();
  expect(f.polls).toHaveLength(1);expect(f.timers.size).toBe(0);
  for(let turn=0;turn<10;turn++){f.expire();await f.flush();}
  expect(f.polls).toHaveLength(1);
  f.writable(0);
  let callbacks=0;f.transport.channels.get("self").send(frame({value:"é雪"}),()=>callbacks++);
  await f.flush();
  expect(f.updates.some(update=>update.interests.some((interest:any)=>interest.writable))).toBe(true);
  f.writable(65536);await f.flush();
  expect(Buffer.concat(f.writes).toString()).toBe(frame({value:"é雪"}));
  expect(callbacks).toBe(1);expect(f.refs.size).toBe(0);
  expect(f.targets.every(target=>target.generation==="17")).toBe(true);
});

test("partial writes preserve bytes and callback ownership",async()=>{
  const f=fixture();f.writable(3);const channel=f.open("child","21");let callbacks=0;
  const encoded=frame({value:"chunked 🟠"});channel.send(encoded,()=>callbacks++);await f.flush();
  expect(Buffer.concat(f.writes).toString()).toBe(encoded);expect(callbacks).toBe(1);
  expect(channel.retainedBytes).toBe(0);expect(f.errors).toEqual([]);
});

test("one-byte split frames preserve non-ASCII payloads",async()=>{
  const f=fixture();f.open();await f.flush();const message={value:"é雪🟠"};
  f.feed("self",[...Buffer.from(frame(message))].map(byte=>Buffer.from([byte])));await f.flush();
  expect(f.messages).toEqual([message]);expect(f.errors).toEqual([]);
});

test("closing one channel does not retire another channel's waiter",async()=>{
  const f=fixture();const first=f.open("one","1");const second=f.open("two","2");await f.flush();
  first.disconnect();await f.flush();let callbacks=0;second.send(frame("alive"),()=>callbacks++);await f.flush();
  expect(first.closed).toBe(true);expect(second.closed).toBe(false);expect(callbacks).toBe(1);expect(f.errors).toEqual([]);
});

test("explicit disconnect times out blocked accepted output and settles once",async()=>{
  const f=fixture();f.writable(0);const channel=f.open();const completions:any[]=[];
  channel.send(frame("blocked"),(error:any)=>completions.push(error));await f.flush();channel.disconnect();
  expect(f.timers.size).toBe(1);f.expire();await f.flush();f.expire();await f.flush();
  expect(completions).toHaveLength(1);expect(completions[0].code).toBe("ERR_AGENTOS_IPC_DRAIN_TIMEOUT");
  expect(channel.closed).toBe(true);expect(f.refs.size).toBe(0);expect(channel.retainedBytes).toBe(0);
});

test("encoded byte admission includes the retained prefix of a partial write",async()=>{
  const f=fixture();f.writeOnce(3);const channel=f.open();const completions:any[]=[];
  // A single finite boundary value, not an unbounded producer.
  channel.send("é".repeat(4*1024*1024),(error:any)=>completions.push(error));await f.flush();
  expect(channel.retainedBytes).toBe(8*1024*1024);expect(channel.bytes).toBe(8*1024*1024-3);
  expect(channel.send("x",(error:any)=>completions.push(error))).toBe(false);await f.flush();
  expect(completions).toHaveLength(1);expect(completions[0].code).toBe("ERR_RESOURCE_BUDGET_EXCEEDED");
  channel.abort(new Error("finite control ended"));await f.flush();expect(completions).toHaveLength(2);expect(f.refs.size).toBe(0);
});

test("truncated frames fail explicitly and retired execution callbacks do not cross reset",async()=>{
  const f=fixture();f.open();await f.flush();f.feed("self",[Buffer.from(IPC_FRAME_PREFIX+"partial"),"eof"]);await f.flush();
  expect(f.errors[0].code).toBe("ERR_IPC_MESSAGE_FAILED");expect(f.closed).toEqual(["self"]);
  const g=fixture();g.writable(0);g.open().send(frame("queued"),()=>{throw new Error("old callback crossed reset");});await g.flush();
  g.transport.abandon(new Error("execution retired"));await g.flush();expect(g.transport.channels.size).toBe(0);expect(g.refs.size).toBe(0);
});


test("oversized inbound framing stops before retaining bytes beyond the cap",async()=>{
  const f=fixture();const channel=f.open();await f.flush();
  f.feed("self",[Buffer.alloc(8*1024*1024+1,120)]);await f.flush();
  expect(f.errors).toHaveLength(1);expect(f.errors[0].code).toBe("ERR_RESOURCE_BUDGET_EXCEEDED");
  expect(channel.closed).toBe(true);expect(channel.frame.length).toBe(0);
});

test("a failed enqueue without a callback reports its typed failure",async()=>{
  const f=fixture();const channel=f.open();
  expect(channel.send("x".repeat(8*1024*1024+1))).toBe(false);await f.flush();
  expect(f.errors).toHaveLength(1);expect(f.errors[0].code).toBe("ERR_RESOURCE_BUDGET_EXCEEDED");
  expect(f.writes).toEqual([]);expect(channel.retainedBytes).toBe(0);
});

test("a throwing disconnect hook preserves failure and settles peer-exit ownership once",async()=>{
  const f=fixture();const channel=f.open();await f.flush();
  const failure=new Error("synthetic disconnect listener failure");let hooks=0,settlements=0;
  channel.hooks.close=()=>{hooks++;throw failure;};
  channel.afterPeerExit().then(()=>{settlements++;});
  f.feed("self",["eof"]);
  let caught:unknown;try{await f.flush();}catch(error){caught=error;}
  await f.flush();
  expect(caught).toBe(failure);expect(hooks).toBe(1);
  expect(f.closed).toEqual(["self"]);expect(settlements).toBe(1);
  expect(f.timers.size).toBe(0);expect(f.refs.size).toBe(0);
  channel.disconnect();channel.abort(new Error("duplicate close"));await f.flush();
  expect(hooks).toBe(1);expect(f.closed).toEqual(["self"]);expect(settlements).toBe(1);
});
