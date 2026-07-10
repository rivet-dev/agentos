'use strict';

const common = require('../../common');
const assert = require('assert');

const test_symbol = require(`./build/${common.buildType}/13_symbol`);

{
  assert.strictEqual(typeof Symbol.dispose, 'symbol');
  assert.strictEqual(Symbol.dispose.description, 'Symbol.dispose');
  assert.strictEqual(typeof Symbol.asyncDispose, 'symbol');
  assert.strictEqual(Symbol.asyncDispose.description, 'Symbol.asyncDispose');
}

{
  const s = test_symbol.New('test');
  assert.strictEqual(s.toString(), 'Symbol(test)');
}

{
  const myObj = {};
  const fooSym = test_symbol.New('foo');
  const otherSym = test_symbol.New('bar');
  myObj.foo = 'bar';
  myObj[fooSym] = 'baz';
  myObj[otherSym] = 'bing';
  assert.strictEqual(myObj.foo, 'bar');
  assert.strictEqual(myObj[fooSym], 'baz');
  assert.strictEqual(myObj[otherSym], 'bing');
}

{
  const fooSym = test_symbol.New('foo');
  const myObj = {};
  myObj.foo = 'bar';
  myObj[fooSym] = 'baz';

  assert.deepStrictEqual(Object.keys(myObj), ['foo']);
  assert.deepStrictEqual(Object.getOwnPropertyNames(myObj), ['foo']);
  assert.deepStrictEqual(Object.getOwnPropertySymbols(myObj), [fooSym]);
}

{
  assert.notStrictEqual(test_symbol.New(), test_symbol.New());
  assert.notStrictEqual(test_symbol.New('foo'), test_symbol.New('foo'));
  assert.notStrictEqual(test_symbol.New('foo'), test_symbol.New('bar'));

  const foo1 = test_symbol.New('foo');
  const foo2 = test_symbol.New('foo');
  const object = { [foo1]: 1, [foo2]: 2 };

  assert.strictEqual(object[foo1], 1);
  assert.strictEqual(object[foo2], 2);
}
