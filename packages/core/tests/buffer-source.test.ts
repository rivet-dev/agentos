import {expect,test} from "vitest";
import {timingSafeEqual} from "node:crypto";
import vm from "node:vm";
import {intrinsicArrayBufferBytes,intrinsicArrayBufferViewBytes} from "../../build-tools/bridge-src/builtins/buffer-source.js";

test("intrinsic view extraction ignores own metadata values and getters",()=>{
  for(const shape of ["Uint8Array","Buffer","DataView","shared"]){
    for(const property of ["buffer","byteOffset","byteLength"]){
      for(const getter of [false,true]){
        let reads=0;
        const make=(byte:number)=>{
          const backing=shape==="shared"?new SharedArrayBuffer(3):new ArrayBuffer(3);
          new Uint8Array(backing).set([7,byte,7]);
          const value=shape==="Buffer"?Buffer.from(backing,1,1):shape==="DataView"?new DataView(backing,1,1):new Uint8Array(backing,1,1);
          const replacement=property==="buffer"?new Uint8Array([7,7,7]).buffer:0;
          Object.defineProperty(value,property,getter?{get(){reads++;return replacement;}}:{value:replacement});
          return value;
        };
        const left=intrinsicArrayBufferViewBytes(make(1)),right=intrinsicArrayBufferViewBytes(make(2));
        expect(left).not.toBeNull();expect(right).not.toBeNull();
        expect(timingSafeEqual(left!,right!)).toBe(false);expect([...left!]).toEqual([1]);expect([...right!]).toEqual([2]);
        expect(reads).toBe(0);
      }
    }
  }
});

test("intrinsic extraction accepts genuine foreign brands and rejects spoofed receivers",()=>{
  const foreign=vm.runInNewContext("({buffer:new Uint8Array([1,2]).buffer,view:new DataView(new Uint8Array([7,1,2,7]).buffer,1,2)})");
  expect([...intrinsicArrayBufferBytes(foreign.buffer)!]).toEqual([1,2]);
  expect([...intrinsicArrayBufferViewBytes(foreign.view)!]).toEqual([1,2]);
  for(const value of [{[Symbol.toStringTag]:"ArrayBuffer",byteLength:2},Object.create(ArrayBuffer.prototype),new Proxy(new ArrayBuffer(2),{})]){
    expect(intrinsicArrayBufferBytes(value)).toBeNull();expect(intrinsicArrayBufferViewBytes(value)).toBeNull();
  }
  expect(intrinsicArrayBufferBytes(new SharedArrayBuffer(2))).toBeNull();
});
