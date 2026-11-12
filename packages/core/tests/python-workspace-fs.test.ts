import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const source = readFileSync(
	new URL(
		"../../../crates/executor-v8-runtime/assets/runners/python-runner.mjs",
		import.meta.url,
	),
	"utf8",
);
const start = source.indexOf(
	"    mmap(stream,",
	source.indexOf("const workspaceFileStreamOps ="),
);
const end = source.indexOf("    msync(stream,", start);
if (start < 0 || end < start)
	throw new Error("Python workspace mmap implementation not found");

function mmapWith(hooks: {
	allocate: () => { ptr: number; allocated: boolean };
	read: (...args: any[]) => number;
	heap: Uint8Array;
	free: (ptr: number) => void;
	allocatedBytes: (length: number) => void;
}) {
	return new Function(
		"Uint8Array",
		"withFsErrors",
		"readSync",
		"memfsFileStreamOps",
		"pyodide",
		`return ({${source.slice(start, end)}}).mmap;`,
	)(
		function Bytes(length: number) {
			hooks.allocatedBytes(length);
			return new Uint8Array(length);
		},
		(operation: () => unknown) => operation(),
		hooks.read,
		{ mmap: hooks.allocate },
		{ _module: { HEAPU8: hooks.heap, _free: hooks.free } },
	);
}

const stream = { node: {}, agentOSKernelHandle: { fd: 7 } };

test("Python mmap rejects WASM allocation before allocating or reading a file copy", () => {
	const rejected = new Error("WASM allocation denied");
	const events: string[] = [];
	const mmap = mmapWith({
		allocate: () => {
			events.push("wasm");
			throw rejected;
		},
		read: () => {
			events.push("read");
			return 0;
		},
		heap: new Uint8Array(32),
		free: () => events.push("free"),
		allocatedBytes: () => events.push("copy"),
	});
	expect(() => mmap(stream, 16, 0, 1, 1)).toThrow(rejected);
	expect(events).toEqual(["wasm"]);
});

test("Python mmap releases its allocation when the opened descriptor read fails", () => {
	const rejected = new Error("descriptor read denied");
	const released: number[] = [];
	const mmap = mmapWith({
		allocate: () => ({ ptr: 8, allocated: true }),
		read: () => {
			throw rejected;
		},
		heap: new Uint8Array(32),
		free: (ptr) => released.push(ptr),
		allocatedBytes: () => {
			throw new Error("unexpected copy");
		},
	});
	expect(() => mmap(stream, 16, 0, 1, 1)).toThrow(rejected);
	expect(released).toEqual([8]);
});

test("Python mmap fills the admitted WASM span from the opened descriptor", () => {
	const heap = new Uint8Array(32);
	let reads = 0;
	const mmap = mmapWith({
		allocate: () => ({ ptr: 8, allocated: true }),
		read: (fd, buffer, offset, length, position) => {
			expect(fd).toBe(7);
			expect(buffer).toBe(heap);
			expect(position).toBe(3 + reads * 2);
			if (reads++ === 2) return 0;
			expect(length).toBeLessThanOrEqual(16);
			buffer.set([65, 66], offset);
			return 2;
		},
		heap,
		free: () => {
			throw new Error("successful mapping freed");
		},
		allocatedBytes: () => {
			throw new Error("unexpected copy");
		},
	});
	expect(mmap(stream, 16, 3, 1, 1)).toEqual({ ptr: 8, allocated: true });
	expect([...heap.subarray(8, 14)]).toEqual([65, 66, 65, 66, 0, 0]);
});

test("Python mapping imports retain descriptors across reuse, partial unmap and injected failures", () => {
	const begin = source.indexOf("function installPythonMmapImports(");
	const end = source.indexOf("function preparePythonMmapImportFactory(", begin);
	expect(begin).toBeGreaterThanOrEqual(0);
	expect(end).toBeGreaterThan(begin);
	const logs: string[] = [];
	const install = new Function(
		"writeStream",
		"formatError",
		"process",
		`${source.slice(begin, end)}; return installPythonMmapImports;`,
	)((_stream: unknown, message: string) => logs.push(message), String, {
		stderr: {},
	});
	new Function(
		"install",
		"assert",
		"logs",
		`function fixture(){
 class ErrnoError extends Error{constructor(errno){super(String(errno));this.errno=errno;}}
 const heap=new Uint8Array(262144),streams=new Map(),handles=new Map(),closeCalls=[];let next=10,ptr=4096;
 let rejectDup=false,writeError=false,mmapError=false,closeError='';
 const target=new Uint8Array(16384);handles.set(3,target);
 const operations={close(stream){const fd=stream.agentOSKernelHandle.fd;closeCalls.push(fd);if(closeError==='before')throw new ErrnoError(29);assert(handles.delete(fd),'owned kernel descriptor exists');delete stream.agentOSKernelHandle;if(closeError==='after')throw new ErrnoError(29);}};
 const original={fd:3,agentOSKernelHandle:{fd:3},target,stream_ops:operations};streams.set(3,original);
 const FS={ErrnoError,getStream:fd=>streams.get(fd),closeStream:fd=>streams.delete(fd),
  dupStream(stream){if(rejectDup)throw new ErrnoError(33);const kernel=next++;handles.set(kernel,stream.target);let fd=4;while(streams.has(fd))fd++;const dup={...stream,fd,agentOSKernelHandle:{fd:kernel}};streams.set(fd,dup);return dup;},
  close(stream){try{operations.close(stream);}finally{streams.delete(stream.fd);stream.fd=null;}},
  msync(stream,bytes,offset,length){if(writeError)throw new ErrnoError(29);if(!handles.has(stream.agentOSKernelHandle?.fd))throw new ErrnoError(8);stream.target.set(bytes.subarray(0,length),offset);},
 };
 const module={FS,HEAPU8:heap,HEAPU32:new Uint32Array(heap.buffer)};
 const imports={_mmap_js(len,prot,flags,fd,offset,allocated,address){if(mmapError)return -48;module.HEAPU32[address/4]=ptr;module.HEAPU32[allocated/4]=1;ptr+=65536;return 0;},
 _msync_js(){return -48;},_munmap_js(){return -48;}};
 install(imports,module);
 const map=(length=8)=>{const code=imports._mmap_js(length,2,1,3,0n,4,0);return {code,address:module.HEAPU32[0]};};
 return {FS,module,imports,map,streams,handles,original,closeCalls,set rejectDup(v){rejectDup=v;},set writeError(v){writeError=v;},set mmapError(v){mmapError=v;},set closeError(v){closeError=v;}};
}
{
 const f=fixture(),m=f.map();assert.equal(m.code,0);assert.equal(f.handles.size,2);
 assert.equal(f.streams.size,1,'mapping must not expose a retained Emscripten slot');
 f.FS.close(f.original);const replacement={fd:3,agentOSKernelHandle:{fd:3},target:new Uint8Array(32)};f.streams.set(3,replacement);f.handles.set(3,replacement.target);
 f.module.HEAPU8.set([65,66],m.address);assert.equal(f.imports._msync_js(m.address,8,2,1,3,0n),0);
 assert.deepEqual([...f.original.target.slice(0,2)],[65,66]);assert.equal(replacement.target[0],0);
 assert.equal(f.imports._munmap_js(m.address,8,2,1,3,0n),0);assert.equal(f.handles.size,1);assert.equal(f.streams.get(3),replacement);
}
{
 const f=fixture();f.mmapError=true;assert.equal(f.map().code,-48);assert.equal(f.handles.size,1);assert.equal(f.streams.size,1);
 f.mmapError=false;f.rejectDup=true;assert.equal(f.map().code,-33);assert.equal(f.handles.size,1);
}
{
 const f=fixture(),m=f.map();f.writeError=true;
 assert.equal(f.imports._msync_js(m.address,8,2,1,3,0n),-29);assert.equal(f.handles.size,2);
 assert.equal(f.imports._munmap_js(m.address,8,2,1,3,0n),-29);assert.equal(f.handles.size,2);
 f.writeError=false;assert.equal(f.imports._munmap_js(m.address,8,2,1,3,0n),0);assert.equal(f.handles.size,1);
}
{
 const f=fixture(),m=f.map(12288);f.rejectDup=true;
 assert.equal(f.imports._munmap_js(m.address+4096,4096,2,1,3,4096n),-33);assert.equal(f.handles.size,2);
 f.rejectDup=false;assert.equal(f.imports._munmap_js(m.address+4096,4096,2,1,3,4096n),0);assert.equal(f.handles.size,3);assert.equal(f.streams.size,1);
 assert.equal(f.imports._msync_js(m.address,12288,2,1,3,0n),-48);
 assert.equal(f.imports._munmap_js(m.address,4096,2,1,3,0n),0);assert.equal(f.handles.size,2);
 assert.equal(f.imports._munmap_js(m.address+8192,4096,2,1,3,8192n),0);assert.equal(f.handles.size,1);
}
for(const timing of ['before','after']){
 const f=fixture(),m=f.map(),closed=10,previousLogs=logs.length;f.closeError=timing;
 assert.equal(f.imports._munmap_js(m.address,8,2,1,3,0n),0);
 assert.equal(logs.length,previousLogs+1);assert.match(logs.at(-1),/ERR_AGENTOS_PYTHON_MAPPING_CLOSE/);
 assert.equal(f.handles.size,timing==='before'?2:1,'pre-retirement failure retains one charged kernel descriptor');
 const replacement=new Uint8Array(16);f.handles.set(closed,replacement);f.closeError='';
 assert.equal(f.imports._munmap_js(m.address,8,2,1,3,0n),-48);
 assert.equal(f.closeCalls.filter(fd=>fd===closed).length,1,'never retry retired numeric descriptor');
 assert.equal(f.handles.get(closed),replacement);
}
`,
	)(install, assert, logs);
});
