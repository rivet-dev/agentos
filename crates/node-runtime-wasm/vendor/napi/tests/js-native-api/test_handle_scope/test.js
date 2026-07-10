'use strict';
const common = require('../../common');
const assert = require('assert');

// Testing handle scope api calls
const testHandleScope =
    require(`./build/${common.buildType}/test_handle_scope`);

testHandleScope.NewScope();

assert.ok(testHandleScope.NewScopeEscape() instanceof Object);

testHandleScope.NewScopeEscapeTwice();

assert.strictEqual(testHandleScope.CloseHandleScopeOutOfOrder(), true);

assert.throws(
  () => {
    testHandleScope.NewScopeWithException(() => { throw new RangeError(); });
  },
  RangeError);
