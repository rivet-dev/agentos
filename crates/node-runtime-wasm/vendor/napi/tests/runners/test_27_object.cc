#include <string>

#include "test_env.h"
#include "upstream_js_test.h"

extern "C" napi_value Init(napi_env env, napi_value exports);

class Test27Object : public FixtureTestBase {};

TEST_F(Test27Object, PortedCoreFlow) {
  EnvScope s(runtime_.get());
  napi_value exports = nullptr;
  ASSERT_EQ(napi_create_object(s.env, &exports), napi_ok);
  ASSERT_NE(Init(s.env, exports), nullptr);

  napi_value global = nullptr;
  ASSERT_EQ(napi_get_global(s.env, &global), napi_ok);
  ASSERT_EQ(napi_set_named_property(s.env, global, "__to", exports), napi_ok);

  ASSERT_TRUE(RunScript(s, R"JS(
(() => { 'use strict';
const assert = {
  ok(v,m){ if(!v) throw new Error(m||'assert'); },
  strictEqual(a,b,m){ if(a!==b) throw new Error(m||`strictEqual:${a}!=${b}`); },
  deepStrictEqual(a,b,m){ if(JSON.stringify(a)!==JSON.stringify(b)) throw new Error(m||`deep:${JSON.stringify(a)}!=${JSON.stringify(b)}`); },
  throws(fn, re){ let t=false; try{ fn(); } catch(e){ t=true; if(re && !re.test(String(e))) throw e; } if(!t) throw new Error('expected throw'); }
};

const object = { hello:'world', array:[1,94,'str',12.321,{test:'obj in arr'}], newObject:{test:'obj in obj'} };
assert.strictEqual(__to.Get(object, 'hello'), 'world');
assert.strictEqual(__to.GetNamed(object, 'hello'), 'world');
assert.deepStrictEqual(__to.Get(object, 'array'), [1,94,'str',12.321,{test:'obj in arr'}]);
assert.deepStrictEqual(__to.Get(object, 'newObject'), {test:'obj in obj'});
assert.ok(__to.Has(object, 'hello'));
assert.ok(__to.HasNamed(object, 'hello'));
assert.ok(__to.Has(object, 'array'));
assert.ok(__to.Has(object, 'newObject'));

const newObject = __to.New();
assert.ok(__to.Has(newObject, 'test_number'));
assert.strictEqual(newObject.test_number, 987654321);
assert.strictEqual(newObject.test_string, 'test string');

function MyObject(){ this.foo = 42; this.bar = 43; }
MyObject.prototype.bar = 44;
MyObject.prototype.baz = 45;
const obj = new MyObject();
assert.strictEqual(__to.Get(obj, 'foo'), 42);
assert.strictEqual(__to.Get(obj, 'bar'), 43);
assert.strictEqual(__to.Get(obj, 'baz'), 45);
assert.strictEqual(__to.Get(obj, 'toString'), Object.prototype.toString);

[true,false,null,undefined,{},[],0,1,()=>{}].forEach((v)=> {
  assert.throws(() => __to.HasOwn({}, v), /^Error: A string or symbol was expected$/);
});

const symbol1 = Symbol();
const symbol2 = Symbol();
function Obj2(){ this.foo=42; this.bar=43; this[symbol1]=44; }
Obj2.prototype.bar=45;
Obj2.prototype.baz=46;
Obj2.prototype[symbol2]=47;
const obj2 = new Obj2();
assert.strictEqual(__to.HasOwn(obj2, 'foo'), true);
assert.strictEqual(__to.HasOwn(obj2, 'bar'), true);
assert.strictEqual(__to.HasOwn(obj2, symbol1), true);
assert.strictEqual(__to.HasOwn(obj2, 'baz'), false);
assert.strictEqual(__to.HasOwn(obj2, 'toString'), false);
assert.strictEqual(__to.HasOwn(obj2, symbol2), false);

const cube = {x:10,y:10,z:10};
assert.deepStrictEqual(__to.Inflate(cube), {x:11,y:11,z:11});
assert.deepStrictEqual(__to.Inflate(cube), {x:12,y:12,z:12});
assert.deepStrictEqual(__to.Inflate(cube), {x:13,y:13,z:13});
cube.t = 13;
assert.deepStrictEqual(__to.Inflate(cube), {x:14,y:14,z:14,t:14});

const sym1 = Symbol('1'); const sym2 = Symbol('2'); const sym3 = Symbol('3'); const sym4 = Symbol('4');
const o3 = { [sym1]:'@@iterator', [sym2]:sym3 };
assert.ok(__to.Has(o3, sym1)); assert.ok(__to.Has(o3, sym2));
assert.strictEqual(__to.Get(o3, sym1), '@@iterator'); assert.strictEqual(__to.Get(o3, sym2), sym3);
assert.ok(__to.Set(o3, 'string', 'value')); assert.ok(__to.SetNamed(o3, 'named_string', 'value')); assert.ok(__to.Set(o3, sym4, 123));
assert.ok(__to.Has(o3, 'string')); assert.ok(__to.HasNamed(o3, 'named_string')); assert.ok(__to.Has(o3, sym4));
assert.strictEqual(__to.Get(o3, 'string'), 'value'); assert.strictEqual(__to.Get(o3, sym4), 123);

const wrapper = {};
__to.Wrap(wrapper);
assert.ok(__to.Unwrap(wrapper));

const wrapper2 = {};
const protoA = { protoA: true };
Object.setPrototypeOf(wrapper2, protoA);
__to.Wrap(wrapper2);
assert.ok(__to.Unwrap(wrapper2));
assert.strictEqual(wrapper2.protoA, true);

const wrapper3 = {};
Object.setPrototypeOf(wrapper3, protoA);
__to.Wrap(wrapper3);
const protoB = { protoB: true };
Object.setPrototypeOf(protoB, Object.getPrototypeOf(wrapper3));
Object.setPrototypeOf(wrapper3, protoB);
assert.ok(__to.Unwrap(wrapper3));
assert.strictEqual(wrapper3.protoA, true);
assert.strictEqual(wrapper3.protoB, true);

const tt1 = __to.TypeTaggedInstance(0);
const tt2 = __to.TypeTaggedInstance(1);
const tt3 = __to.TypeTaggedInstance(2);
const tt4 = __to.TypeTaggedInstance(3);
const ex = __to.TypeTaggedExternal(2);
const plainEx = __to.PlainExternal();
assert.throws(() => __to.TypeTaggedInstance(39), /Invalid type index/);
assert.throws(() => __to.TypeTaggedExternal(39), /Invalid type index/);
assert.strictEqual(__to.CheckTypeTag(0, tt1), true);
assert.strictEqual(__to.CheckTypeTag(1, tt2), true);
assert.strictEqual(__to.CheckTypeTag(2, tt3), true);
assert.strictEqual(__to.CheckTypeTag(3, tt4), true);
assert.strictEqual(__to.CheckTypeTag(2, ex), true);
assert.strictEqual(__to.CheckTypeTag(0, tt2), false);
assert.strictEqual(__to.CheckTypeTag(1, tt1), false);
assert.strictEqual(__to.CheckTypeTag(0, tt3), false);
assert.strictEqual(__to.CheckTypeTag(1, tt4), false);
assert.strictEqual(__to.CheckTypeTag(2, tt4), false);
assert.strictEqual(__to.CheckTypeTag(3, tt3), false);
assert.strictEqual(__to.CheckTypeTag(4, tt3), false);
assert.strictEqual(__to.CheckTypeTag(0, ex), false);
assert.strictEqual(__to.CheckTypeTag(1, ex), false);
assert.strictEqual(__to.CheckTypeTag(3, ex), false);
assert.strictEqual(__to.CheckTypeTag(4, ex), false);
assert.strictEqual(__to.CheckTypeTag(0, {}), false);
assert.strictEqual(__to.CheckTypeTag(1, {}), false);
assert.strictEqual(__to.CheckTypeTag(0, plainEx), false);
assert.strictEqual(__to.CheckTypeTag(1, plainEx), false);
assert.strictEqual(__to.CheckTypeTag(2, plainEx), false);
assert.strictEqual(__to.CheckTypeTag(3, plainEx), false);
assert.strictEqual(__to.CheckTypeTag(4, plainEx), false);

const ds = Symbol();
const delObj = { foo:'bar', [ds]:'baz' };
assert.strictEqual(__to.Delete(delObj, 'foo'), true);
assert.strictEqual(__to.Delete(delObj, ds), true);
const ncObj = {};
Object.defineProperty(ncObj, 'foo', { configurable: false });
assert.strictEqual(__to.Delete(ncObj, 'foo'), false);
function F(){ this.foo='bar'; }
F.prototype.foo='baz';
const delProto = new F();
assert.strictEqual(__to.Delete(delProto, 'foo'), true);
assert.strictEqual(delProto.foo, 'baz');
assert.strictEqual(__to.Delete(delProto, 'foo'), true);
assert.strictEqual(delProto.foo, 'baz');

const tset = __to.TestSetProperty();
assert.deepStrictEqual(tset, { envIsNull:'Invalid argument', objectIsNull:'Invalid argument', keyIsNull:'Invalid argument', valueIsNull:'Invalid argument' });
const thas = __to.TestHasProperty();
assert.deepStrictEqual(thas, { envIsNull:'Invalid argument', objectIsNull:'Invalid argument', keyIsNull:'Invalid argument', resultIsNull:'Invalid argument' });
const tget = __to.TestGetProperty();
assert.deepStrictEqual(tget, { envIsNull:'Invalid argument', objectIsNull:'Invalid argument', keyIsNull:'Invalid argument', resultIsNull:'Invalid argument' });

const sealObj = {x:'a',y:'b',z:'c'};
__to.TestSeal(sealObj);
assert.strictEqual(Object.isSealed(sealObj), true);
assert.throws(() => { sealObj.w = 'd'; }, /object is not extensible/);
assert.throws(() => { delete sealObj.x; }, /Cannot delete property|could not delete property/);
sealObj.x = 'd';

const freezeObj = {x:10,y:10,z:10};
__to.TestFreeze(freezeObj);
assert.strictEqual(Object.isFrozen(freezeObj), true);
assert.throws(() => { freezeObj.x = 10; }, /read only property|read-only/);
assert.throws(() => { freezeObj.w = 15; }, /object is not extensible/);
assert.throws(() => { delete freezeObj.x; }, /Cannot delete property|could not delete property/);

const owp = __to.TestCreateObjectWithProperties();
assert.strictEqual(typeof owp, 'object');
assert.strictEqual(owp.name, 'Foo');
assert.strictEqual(owp.age, 42);
assert.strictEqual(owp.active, true);
const empty = __to.TestCreateObjectWithPropertiesEmpty();
assert.strictEqual(typeof empty, 'object');
assert.strictEqual(Object.keys(empty).length, 0);
const withProto = __to.TestCreateObjectWithCustomPrototype();
assert.strictEqual(typeof withProto, 'object');
assert.deepStrictEqual(Object.getOwnPropertyNames(withProto), ['value']);
assert.strictEqual(withProto.value, 42);
assert.strictEqual(typeof withProto.test, 'function');

const pnObj = { __proto__: { inherited: 1 } };
const fooSymbol = Symbol('foo');
pnObj.normal = 2;
pnObj[fooSymbol] = 3;
Object.defineProperty(pnObj, 'unenumerable', { value: 4, enumerable: false, writable: true, configurable: true });
Object.defineProperty(pnObj, 'writable', { value: 4, enumerable: true, writable: true, configurable: false });
Object.defineProperty(pnObj, 'configurable', { value: 4, enumerable: true, writable: false, configurable: true });
pnObj[5] = 5;
assert.deepStrictEqual(__to.GetPropertyNames(pnObj), ['5','normal','writable','configurable','inherited']);
assert.deepStrictEqual(__to.GetSymbolNames(pnObj), [fooSymbol]);
assert.deepStrictEqual(__to.GetEnumerableWritableNames(pnObj), ['5','normal','writable',fooSymbol,'inherited']);
assert.deepStrictEqual(__to.GetOwnWritableNames(pnObj), ['5','normal','unenumerable','writable',fooSymbol]);
assert.deepStrictEqual(__to.GetEnumerableConfigurableNames(pnObj), ['5','normal','configurable',fooSymbol,'inherited']);
assert.deepStrictEqual(__to.GetOwnConfigurableNames(pnObj), ['5','normal','unenumerable','configurable',fooSymbol]);
})();
)JS", "test_27_object"));
}
