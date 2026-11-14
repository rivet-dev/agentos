import { spawnSync } from "node:child_process";
import { deserialize, serialize } from "node:v8";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("node:v8 preserves graphs and exchanges native serialization with Node.js", async () => {
	const fixture = serialize({ date: new Date(1234), map: new Map([["key", 3n]]), bytes: Buffer.from([7, 8]) }).toString("base64");
	const source = `
import {serialize,deserialize} from 'node:v8';
import {runInNewContext} from 'node:vm';
import {createRequire} from 'node:module';
const cjs=createRequire(import.meta.url)('node:v8');
const backing=Buffer.from('private-prefix:visible:private-suffix');
const visible=backing.subarray(15,22);
const child={value:3};const input={left:child,right:child,map:new Map([['key',3n]]),set:new Set([4]),date:new Date(1234),bytes:Buffer.from([7,8]),view:new Uint16Array([500,600]),regexp:/a+/gi,negative:-0,missing:undefined};input.self=input;
const bytes=serialize(input), copy=deserialize(bytes);
const host=deserialize(Buffer.from(${JSON.stringify(fixture)},'base64'));
const foreign=deserialize(serialize(runInNewContext('({date:new Date(1234),view:new Uint16Array([500,600])})')));
const error=fn=>{try{fn();return null;}catch(e){return e.name;}};
console.log(JSON.stringify({
 summary:{sameExports:cjs.serialize===serialize&&cjs.deserialize===deserialize,viewPrivacy:!serialize(visible).includes(Buffer.from('private-')),cycle:copy.self===copy,alias:copy.left===copy.right,map:copy.map.get('key').toString(),set:copy.set.has(4),date:copy.date.getTime(),buffer:Buffer.isBuffer(copy.bytes),bytes:[...copy.bytes],view:[...copy.view],viewType:copy.view.constructor.name,regexp:copy.regexp.toString(),negative:Object.is(copy.negative,-0),missing:'missing' in copy&&copy.missing===undefined,host:[host.date.getTime(),host.map.get('key').toString(),Buffer.isBuffer(host.bytes),[...host.bytes]],foreign:[foreign.date.getTime(),[...foreign.view]],rejected:[error(()=>serialize(()=>0)),error(()=>deserialize(Buffer.from([1,2,3])))]},
 bytes:bytes.toString('base64')
}));`;
	const host = spawnSync(process.execPath, ["--input-type=module", "-e", source], { encoding: "utf8", timeout: 10000, maxBuffer: 65536 });
	expect(host.status, host.stderr).toBe(0);
	const expected = JSON.parse(host.stdout);
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({ sidecar: { kind: "explicit", handle: sidecar }, defaultSoftware: false, permissions: { network: "deny" } });
		let stdout = "", stderr = "";
		const proc = await vm.process.spawn("node", ["--input-type=module", "-e", source], {
			timeoutMs: 10000,
			onStdout: c => { stdout += Buffer.from(c).toString(); }, onStderr: c => { stderr += Buffer.from(c).toString(); },
		});
		expect((await vm.process.wait(proc.pid)).exitCode, stderr).toBe(0);
		const actual = JSON.parse(stdout);
		expect(actual.summary).toEqual(expected.summary);
		const decoded = deserialize(Buffer.from(actual.bytes, "base64"));
		expect(decoded.self).toBe(decoded);
		expect(decoded.left).toBe(decoded.right);
		expect(decoded.date.getTime()).toBe(1234);
		expect(Buffer.isBuffer(decoded.bytes)).toBe(true);
		expect(decoded.map.get("key")).toBe(3n);
	} finally { try { await vm?.dispose(); } finally { await sidecar.dispose(); } }
}, 20000);

test("node:v8 serialization reports the configured byte limit", async () => {
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({ sidecar: { kind: "explicit", handle: sidecar }, defaultSoftware: false,
			permissions: { network: "deny" }, limits: { reactor: { maxBridgeResponseBytes: 262144 } } });
		const source = `import {serialize,deserialize} from 'node:v8';const result=[];
for(const fn of [()=>serialize(new Uint8Array(262145)),()=>deserialize(new Uint8Array(262145))]) {
 try{fn();result.push(null);}catch(error){result.push([error.code,error.configurationPath]);}
}console.log(JSON.stringify(result));`;
		let stdout = "", stderr = "";
		const proc = await vm.process.spawn("node", ["--input-type=module", "-e", source], {
			timeoutMs: 3000, onStdout: c => { stdout += Buffer.from(c).toString(); }, onStderr: c => { stderr += Buffer.from(c).toString(); },
		});
		expect((await vm.process.wait(proc.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(Array.from({ length: 2 }, () => ["ERR_AGENTOS_BRIDGE_RESPONSE_LIMIT", "limits.reactor.maxBridgeResponseBytes"]));
	} finally { try { await vm?.dispose(); } finally { await sidecar.dispose(); } }
}, 10000);
