import {Buffer3} from "./buffer-runtime.js";
import {IpcTransport} from "./ipc-transport.js";
import {setImmediate,setTimeout2,clearTimeout2} from "./timers.js";

const ipcBuffer={from:Buffer3.from.bind(Buffer3),alloc:Buffer3.alloc.bind(Buffer3),byteLength:Buffer3.byteLength.bind(Buffer3)};
let transport;
const value = result => typeof result === "string" ? JSON.parse(result) : result;

export function ipcInfo(childId=null) {
  return value(_processIpcInfoRaw.applySync(void 0,[childId]));
}

export function openIpcChannel(childId,info,hooks) {
  transport ??= new IpcTransport({
    Buffer:ipcBuffer,
    defer:callback=>setImmediate(callback),
    delay:(callback,ms)=>setTimeout2(callback,ms),
    clearDelay:timer=>clearTimeout2(timer),
    read:(target,size)=>value(_processIpcReadRaw.applySync(void 0,[target,size])),
    write:(target,bytes)=>value(_processIpcWriteRaw.applySync(void 0,[target,bytes])),
    close:target=>_processIpcCloseRaw.applySync(void 0,[target]),
    poll:async(id,interests)=>value(await _processIpcPoll(id,interests)),
    update:(id,revision,interests)=>value(_processIpcUpdateRaw.applySync(void 0,[id,revision,interests])),
    refWrite:(key,close)=>{if(typeof _registerHandle==="function")_registerHandle(`ipc-write:${key}`,"IPC pending write",close);},
    unrefWrite:key=>{if(typeof _unregisterHandle==="function")_unregisterHandle(`ipc-write:${key}`);},
  });
  return transport.open(childId,info,hooks);
}

export function resetIpcTransport() {
  transport?.abandon(Object.assign(new Error("IPC execution retired"),{code:"ESTALE"}));
  transport=undefined;
}
