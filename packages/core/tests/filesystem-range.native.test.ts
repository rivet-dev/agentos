import {expect,test} from "vitest";
import {AgentOs} from "../src/index.js";
test("filesystem range reads cap transfer bytes and retain kernel pread policy",async()=>{
 const sidecar=await AgentOs.createSidecar();let vm:AgentOs|undefined;
 try{vm=await AgentOs.create({sidecar:{kind:"explicit",handle:sidecar},defaultSoftware:false,permissions:{network:"deny"},limits:{resources:{maxPreadBytes:8}}});await vm.filesystem.writeFile("/tmp/data",Buffer.from("0123456789abcdef"));expect(Buffer.from(await vm.filesystem.readFileRange("/tmp/data",3,5)).toString()).toBe("34567");expect((await vm.filesystem.readFileRange("/tmp/data",16,8)).length).toBe(0);await expect(vm.filesystem.readFileRange("/tmp/data",0,9)).rejects.toThrow(/pread|maxPreadBytes|limit/i);await expect(vm.filesystem.readFileRange("/tmp/data",-1,1)).rejects.toThrow(/non-negative safe integers/);
 await vm.filesystem.writeFile("/tmp/larger",Buffer.alloc(1024*1024,82));const proc=await vm.process.spawn("node",["-e","const fs=require('node:fs');fs.unlinkSync('/tmp/data');fs.symlinkSync('/tmp/larger','/tmp/data');"],{timeoutMs:5000});expect((await vm.process.wait(proc.pid)).exitCode).toBe(0);const replacement=await vm.filesystem.readFileRange("/tmp/data",0,8);expect([...replacement]).toEqual(Array(8).fill(82));
 }finally{try{await vm?.dispose();}finally{await sidecar.dispose();}}
},10000);
