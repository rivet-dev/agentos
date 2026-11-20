import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

async function compare(source: string) {
	const host = spawnSync(process.execPath, ["-e", source], {
		encoding: "utf8",
		timeout: 10000,
	});
	expect(host.status, host.stderr).toBe(0);
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
		let stdout = "",
			stderr = "";
		const p = await vm.process.spawn("node", ["-e", source], {
			timeoutMs: 10000,
			onStdout: (b) => (stdout += Buffer.from(b).toString()),
			onStderr: (b) => (stderr += Buffer.from(b).toString()),
		});
		expect((await vm.process.wait(p.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}
test("DOMException legacy names, codes and constants match Node", async () => {
	await compare(
		`const names=['IndexSizeError','DOMStringSizeError','HierarchyRequestError','WrongDocumentError','InvalidCharacterError','NoDataAllowedError','NoModificationAllowedError','NotFoundError','NotSupportedError','InUseAttributeError','InvalidStateError','SyntaxError','InvalidModificationError','NamespaceError','InvalidAccessError','ValidationError','TypeMismatchError','SecurityError','NetworkError','AbortError','URLMismatchError','QuotaExceededError','TimeoutError','InvalidNodeTypeError','DataCloneError','unknown'];const values=names.map(name=>{const e=new DOMException('message',name);return[e.name,e.message,e.code,Object.prototype.toString.call(e)];});let clone;try{structuredClone(Symbol('x'));}catch(e){clone=[e.name,e.code,e instanceof DOMException];}console.log(JSON.stringify({values,clone,constants:[DOMException.NOT_FOUND_ERR,DOMException.prototype.DATA_CLONE_ERR,DOMException.DOMSTRING_SIZE_ERR,DOMException.NO_DATA_ALLOWED_ERR,DOMException.VALIDATION_ERR]}));`,
	);
}, 15000);
test("events.getEventListeners reads EventTarget and AbortSignal listeners without exposing bootstrap hooks", async () => {
	await compare(
		`const {getEventListeners,EventEmitter}=require('node:events');const a=()=>{},b=()=>{},object={handleEvent(){}};const target=new EventTarget();const names=t=>getEventListeners(t,'ping').map(fn=>fn===a?'a':fn===b?'b':'object');target.addEventListener('ping',a);target.addEventListener('ping',a);target.addEventListener('ping',b,{once:true});target.addEventListener('ping',object);const before=names(target);getEventListeners(target,'ping').length=0;target.dispatchEvent(new Event('ping'));const after=names(target);target.removeEventListener('ping',a);const removed=names(target);const controller=new AbortController();controller.signal.addEventListener('abort',a);controller.signal.addEventListener('abort',b,{once:true});const abortBefore=getEventListeners(controller.signal,'abort').length;controller.abort();const abortAfter=getEventListeners(controller.signal,'abort').length;const emitter=new EventEmitter();emitter.on('ping',a);console.log(JSON.stringify({before,after,removed,abortBefore,abortAfter,emitter:getEventListeners(emitter,'ping')[0]===a,privateHook:typeof globalThis.__agentOsGetEventTargetListeners}));`,
	);
}, 15000);
