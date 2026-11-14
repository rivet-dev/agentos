import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("explicit collection options collect guest objects and preserve live references", async () => {
 const source = `
 const enabled = typeof globalThis.gc === 'function';
 let collected = false;
 const live = { value: 42 };
 if (enabled) {
   const registry = new FinalizationRegistry(() => { collected = true; });
   (() => { const discarded = {}; registry.register(discarded, 1); })();
   for (let attempt = 0; attempt < 20 && !collected; attempt++) {
     await new Promise(resolve => setTimeout(resolve, 10));
     gc();
   }
   await gc({execution:'async'});
 }
 console.log(JSON.stringify({enabled, collected, live:live.value}));`;
 const sidecar = await AgentOs.createSidecar();
 try {
  for (const mode of ['default','argument','environment'] as const) {
   const args = [...(mode === 'argument' ? ['--expose-gc'] : []), '--input-type=module', '-e', source];
   const env = {...process.env}; delete env.NODE_OPTIONS;
   if (mode === 'environment') env.NODE_OPTIONS = '--expose-gc';
   const host = spawnSync(process.execPath, args, {env, encoding:'utf8', timeout:5000,maxBuffer:16384});
   expect(host.status,host.stderr).toBe(0);
   const expected = JSON.parse(host.stdout);
   expect(expected).toEqual({enabled:mode !== 'default',collected:mode !== 'default',live:42});
   const vm = await AgentOs.create({sidecar:{kind:'explicit',handle:sidecar},defaultSoftware:false,permissions:{network:'deny'},environment:mode === 'environment' ? {NODE_OPTIONS:'--expose-gc'} : {}});
   try {
    let stdout='',stderr='';
    const child = await vm.process.spawn('node', args, {timeoutMs:5000,onStdout:bytes => {stdout+=Buffer.from(bytes).toString();},onStderr:bytes => {stderr+=Buffer.from(bytes).toString();}});
    expect((await vm.process.wait(child.pid)).exitCode,stderr).toBe(0);
    expect(JSON.parse(stdout),mode).toEqual(expected);
   } finally {await vm.dispose();}
  }
 } finally {await sidecar.dispose();}
},20000);
