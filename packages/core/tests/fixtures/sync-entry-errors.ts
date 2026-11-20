export const syncEntryCases = [
	{
		name: "CJS handler observes unwound context while prior timer retains its frame",
		module: false,
		source: `const { AsyncLocalStorage } = require('node:async_hooks'); const als = new AsyncLocalStorage(); process.on('uncaughtException', (error, origin) => console.log('HANDLED:' + als.getStore() + ':' + origin)); als.run('entry-frame', () => { setTimeout(() => console.log('TIMER:' + als.getStore()), 25); throw Error('entry-error'); });`,
		exitCode: 0,
		stdout: "HANDLED:undefined:uncaughtException\nTIMER:entry-frame\n",
		errorText: "",
	},
	{
		name: "consuming CJS handler retains explicit process exitCode",
		module: false,
		source: `process.on('uncaughtException', () => { process.exitCode = 23; console.log('HANDLED'); }); throw Error('entry-error');`,
		exitCode: 23,
		stdout: "HANDLED\n",
		errorText: "",
	},
	{
		name: "CJS preserves original object and origin",
		module: false,
		source: `const original = { marker: 'original' }; process.on('uncaughtException', (error, origin) => console.log('HANDLED:' + (error === original) + ':' + origin)); throw original; console.log('UNREACHABLE');`,
		exitCode: 0,
		stdout: "HANDLED:true:uncaughtException\n",
		errorText: "",
	},
	{
		name: "CJS preserves thrown null",
		module: false,
		source: `process.on('uncaughtException', (error, origin) => console.log('HANDLED:' + (error === null) + ':' + origin)); throw null;`,
		exitCode: 0,
		stdout: "HANDLED:true:uncaughtException\n",
		errorText: "",
	},
	{
		name: "CJS preserves thrown undefined",
		module: false,
		source: `process.on('uncaughtException', (error, origin) => console.log('HANDLED:' + (error === undefined) + ':' + origin)); throw undefined;`,
		exitCode: 0,
		stdout: "HANDLED:true:uncaughtException\n",
		errorText: "",
	},
	{
		name: "consumed CJS error drains a pre-existing referenced timer",
		module: false,
		source: `setTimeout(() => console.log('TIMER'), 25); process.on('uncaughtException', (error, origin) => console.log('HANDLED:' + error.message + ':' + origin)); throw Error('entry-error');`,
		exitCode: 0,
		stdout: "HANDLED:entry-error:uncaughtException\nTIMER\n",
		errorText: "",
	},
	{
		name: "throwing CJS error handler exits seven without draining timers",
		module: false,
		source: `setTimeout(() => console.log('UNREACHABLE_TIMER'), 25); process.on('uncaughtException', () => { console.log('HANDLER'); throw Error('handler-fatal'); }); throw Error('entry-error');`,
		exitCode: 7,
		stdout: "HANDLER\n",
		errorText: "handler-fatal",
	},
	{
		name: "CJS error handler explicit exit retains nine",
		module: false,
		source: `setTimeout(() => console.log('UNREACHABLE_TIMER'), 25); process.on('uncaughtException', () => { console.log('HANDLER'); process.exit(9); }); throw Error('entry-error');`,
		exitCode: 9,
		stdout: "HANDLER\n",
		errorText: "",
	},
	{
		name: "unhandled CJS error stays terminal",
		module: false,
		source: `setTimeout(() => console.log('UNREACHABLE_TIMER'), 25); throw Error('entry-unhandled');`,
		exitCode: 1,
		stdout: "",
		errorText: "entry-unhandled",
	},
	{
		name: "local catch bypasses process error handler",
		module: false,
		source: `process.on('uncaughtException', () => console.log('UNREACHABLE_HANDLER')); try { throw Error('local'); } catch (error) { console.log('LOCAL:' + error.message); }`,
		exitCode: 0,
		stdout: "LOCAL:local\n",
		errorText: "",
	},
	{
		name: "explicit process exit bypasses process error handler",
		module: false,
		source: `process.on('uncaughtException', () => console.log('UNREACHABLE_HANDLER')); process.exit(9);`,
		exitCode: 9,
		stdout: "",
		errorText: "",
	},
	{
		name: "ESM entry error retains module rejection origin",
		module: true,
		source: `const original = { marker: 'module' }; process.on('uncaughtException', (error, origin) => console.log('HANDLED:' + (error === original) + ':' + origin)); throw original;`,
		exitCode: 0,
		stdout: "HANDLED:true:unhandledRejection\n",
		errorText: "",
	},
	{
		name: "ESM top-level await error retains module rejection origin",
		module: true,
		source: `const original = Error('module-await'); process.on('uncaughtException', (error, origin) => console.log('HANDLED:' + (error === original) + ':' + origin)); await Promise.reject(original);`,
		exitCode: 0,
		stdout: "HANDLED:true:unhandledRejection\n",
		errorText: "",
	},
	{
		name: "syntax failure does not run entrypoint handlers",
		module: false,
		source: `process.on('uncaughtException', () => console.log('UNREACHABLE_HANDLER')); const = ;`,
		exitCode: 1,
		stdout: "",
		errorText: "SyntaxError",
	},
	{
		name: "exit parity listener throw keeps explicit status and diagnostic",
		module: false,
		source:
			"const fs = require('node:fs');\nprocess.on('exit', code => {\n  fs.writeSync(1, 'EXIT:' + code + '\\n');\n  throw new Error('SYNTHETIC_EXIT_LISTENER_THROW');\n});\nprocess.exit(5);\nfs.writeSync(1, 'AFTER_OUTER\\n');\n",
		exitCode: 5,
		stdout: "EXIT:5\n",
		errorText: "SYNTHETIC_EXIT_LISTENER_THROW",
	},
	{
		name: "exit parity listener nested explicit exit overrides code without reentry",
		module: false,
		source:
			"const fs = require('node:fs');\nlet calls = 0;\nprocess.on('exit', code => {\n  fs.writeSync(1, 'EXIT:' + code + '\\n');\n  if (calls++ === 0) process.exit(9);\n  fs.writeSync(1, 'AFTER_NESTED\\n');\n});\nprocess.exit(7);\nfs.writeSync(1, 'AFTER_OUTER\\n');\n",
		exitCode: 9,
		stdout: "EXIT:7\n",
		errorText: "",
	},
	{
		name: "exit parity explicit exit cannot be caught or followed by continuation",
		module: false,
		source:
			"const fs = require('node:fs');\ntry {\n  process.exit(9);\n} catch (error) {\n  fs.writeSync(1, 'CAUGHT_EXIT\\n');\n}\nfs.writeSync(1, 'AFTER_EXIT\\n');\n",
		exitCode: 9,
		stdout: "",
		errorText: "",
	},
	{
		name: "exit parity immediate exit abandons queued sibling microtask",
		module: false,
		source:
			"const fs = require('node:fs');\nsetImmediate(() => {\n  queueMicrotask(() => fs.writeSync(1, 'SIBLING_MICROTASK\\n'));\n  fs.writeSync(1, 'EXIT_NOW\\n');\n  process.exit(9);\n  fs.writeSync(1, 'AFTER_EXIT\\n');\n});\n",
		exitCode: 9,
		stdout: "EXIT_NOW\n",
		errorText: "",
	},
	{
		name: "exit parity genuine thrown listener error remains catchable",
		module: false,
		source:
			"const fs = require('node:fs');\nprocess.once('exit', () => {\n  throw new Error('SYNTHETIC_CATCHABLE_EXIT_LISTENER_ERROR');\n});\ntry {\n  process.exit(5);\n} catch (error) {\n  fs.writeSync(1, 'CAUGHT:' + error.message + '\\n');\n}\nfs.writeSync(1, 'CONTINUATION\\n');\n",
		exitCode: 5,
		stdout: "CAUGHT:SYNTHETIC_CATCHABLE_EXIT_LISTENER_ERROR\nCONTINUATION\n",
		errorText: "",
	},
	{
		name: "natural exit parity code",
		module: false,
		source:
			"const fs = require('node:fs');\nprocess.on('exit', code => fs.writeSync(1, 'EXIT:' + code + '\\n'));\nprocess.exitCode = 5;\n",
		exitCode: 5,
		stdout: "EXIT:5\n",
		errorText: "",
	},
	{
		name: "natural exit parity persistent throw",
		module: false,
		source:
			"const fs = require('node:fs');\nprocess.on('exit', () => {\n  throw new Error('SYNTHETIC_CATCHABLE_EXIT_LISTENER_ERROR');\n});\ntry {\n  process.exit(5);\n} catch (error) {\n  fs.writeSync(1, 'CAUGHT:' + error.message + '\\n');\n}\nfs.writeSync(1, 'CONTINUATION\\n');\n",
		exitCode: 5,
		stdout: "CAUGHT:SYNTHETIC_CATCHABLE_EXIT_LISTENER_ERROR\nCONTINUATION\n",
		errorText: "SYNTHETIC_CATCHABLE_EXIT_LISTENER_ERROR",
	},
	{
		name: "exit caught timer",
		module: false,
		source:
			"const fs = require('node:fs');\nprocess.once('exit', () => {\n  throw new Error('SYNTHETIC_CAUGHT_EXIT_TIMER');\n});\ntry {\n  process.exit(5);\n} catch (error) {\n  fs.writeSync(1, 'CAUGHT\\n');\n}\nprocess.nextTick(() => fs.writeSync(1, 'UNREACHABLE_CAUGHT_NEXT_TICK\\n'));\nsetTimeout(() => fs.writeSync(1, 'TIMER\\n'), 10);\n",
		exitCode: 5,
		stdout: "CAUGHT\nTIMER\n",
		errorText: "",
	},
	{
		name: "exit diagnostic message prefix",
		module: false,
		source:
			"process.once('exit', () => { throw new Error('process.exit(9)'); });\nprocess.exit(5);\n",
		exitCode: 5,
		stdout: "",
		errorText: "process.exit(9)",
	},
	{
		name: "exit diagnostic constructor name",
		module: false,
		source:
			"class ProcessExitError extends Error {}\nprocess.once('exit', () => { throw new ProcessExitError('SYNTHETIC_EXIT_DIAGNOSTIC_NAME'); });\nprocess.exit(5);\n",
		exitCode: 5,
		stdout: "",
		errorText: "SYNTHETIC_EXIT_DIAGNOSTIC_NAME",
	},
	{
		name: "exit status natural default throw",
		module: false,
		source:
			"const fs = require('node:fs');\nprocess.on('exit', code => {\n  fs.writeSync(1, 'EXIT:' + code + '\\n');\n  throw new Error('SYNTHETIC_NATURAL_DEFAULT_EXIT');\n});\n",
		exitCode: 1,
		stdout: "EXIT:0\n",
		errorText: "SYNTHETIC_NATURAL_DEFAULT_EXIT",
	},
	{
		name: "exit status noarg throw",
		module: false,
		source:
			"const fs = require('node:fs');\nprocess.on('exit', code => {\n  fs.writeSync(1, 'EXIT:' + code + '\\n');\n  throw new Error('SYNTHETIC_NOARG_EXIT');\n});\nprocess.exit();\n",
		exitCode: 1,
		stdout: "EXIT:0\n",
		errorText: "SYNTHETIC_NOARG_EXIT",
	},
	{
		name: "exit status explicit zero throw",
		module: false,
		source:
			"const fs = require('node:fs');\nprocess.on('exit', code => {\n  fs.writeSync(1, 'EXIT:' + code + '\\n');\n  throw new Error('SYNTHETIC_ZERO_EXIT');\n});\nprocess.exit(0);\n",
		exitCode: 0,
		stdout: "EXIT:0\n",
		errorText: "SYNTHETIC_ZERO_EXIT",
	},
	{
		name: "exit status selected queue",
		module: false,
		source:
			"const fs = require('node:fs');\nprocess.exitCode = 5;\nprocess.on('exit', code => {\n  fs.writeSync(1, 'EXIT:' + code + '\\n');\n  process.exitCode = 9;\n  queueMicrotask(() => fs.writeSync(1, 'NATURAL_MICROTASK\\n'));\n  Promise.resolve().then(() => fs.writeSync(1, 'NATURAL_PROMISE\\n'));\n  process.nextTick(() => fs.writeSync(1, 'UNREACHABLE_NEXT_TICK\\n'));\n  setTimeout(() => fs.writeSync(1, 'UNREACHABLE_TIMER\\n'), 10);\n});\n",
		exitCode: 9,
		stdout: "EXIT:5\nNATURAL_MICROTASK\nNATURAL_PROMISE\n",
		errorText: "",
	},
	{
		name: "exit diagnostic display name",
		module: false,
		source:
			"process.once('exit', () => { const error = new Error('SYNTHETIC_EXIT_DIAGNOSTIC_DISPLAY_NAME'); error.name = 'ProcessExitError'; throw error; });\nprocess.exit(5);\n",
		exitCode: 5,
		stdout: "",
		errorText: "SYNTHETIC_EXIT_DIAGNOSTIC_DISPLAY_NAME",
	},
	{
		name: "exit fence timer",
		module: false,
		source:
			"const fs = require('node:fs');\nsetTimeout(() => {\n  queueMicrotask(() => fs.writeSync(1, 'SIBLING_TIMER\\n'));\n  fs.writeSync(1, 'EXIT_TIMER\\n');\n  process.exit(9);\n  fs.writeSync(1, 'BAD_AFTER\\n');\n}, 0);\n",
		exitCode: 9,
		stdout: "EXIT_TIMER\n",
		errorText: "",
	},
	{
		name: "exit fence signal",
		module: false,
		source:
			"const fs = require('node:fs');\nprocess.once('SIGUSR2', () => {\n  queueMicrotask(() => fs.writeSync(1, 'SIBLING_SIGNAL\\n'));\n  fs.writeSync(1, 'EXIT_SIGNAL\\n');\n  process.exit(9);\n  fs.writeSync(1, 'BAD_AFTER\\n');\n});\nsetInterval(() => {}, 1000);\nprocess.kill(process.pid, 'SIGUSR2');\n",
		exitCode: 9,
		stdout: "EXIT_SIGNAL\n",
		errorText: "",
	},
	{
		name: "exit fence stream",
		module: false,
		source:
			"const fs = require('node:fs');\nconst { spawn } = require('node:child_process');\nconst child = spawn(process.execPath, ['-e', \"require('node:fs').writeSync(1, 'READY')\"], { stdio: ['ignore', 'pipe', 'ignore'] });\nchild.on('error', (error) => { throw error; });\nchild.once('exit', (code, signal) => {\n  if (code !== 0 || signal !== null) throw new Error('SYNTHETIC_CHILD_EXIT:' + code + ':' + signal);\n  child.stdout.once('data', () => {\n    queueMicrotask(() => fs.writeSync(1, 'SIBLING_STREAM\\n'));\n    fs.writeSync(1, 'EXIT_STREAM\\n');\n    process.exit(9);\n    fs.writeSync(1, 'BAD_AFTER\\n');\n  });\n});\n",
		exitCode: 9,
		stdout: "EXIT_STREAM\n",
		errorText: "",
	},
	{
		name: "exit fence tcp",
		module: false,
		source:
			"const fs = require('node:fs');\nconst net = require('node:net');\nconst server = net.createServer((socket) => {\n  socket.on('error', (error) => { throw error; });\n  socket.once('data', () => {\n    queueMicrotask(() => fs.writeSync(1, 'SIBLING_TCP\\n'));\n    fs.writeSync(1, 'EXIT_TCP\\n');\n    process.exit(9);\n    fs.writeSync(1, 'BAD_AFTER\\n');\n  });\n});\nserver.on('error', (error) => { throw error; });\nserver.listen(0, '127.0.0.1', () => {\n  const client = net.connect(server.address().port, '127.0.0.1', () => client.end('READY'));\n  client.on('error', (error) => { throw error; });\n});\n",
		exitCode: 9,
		stdout: "EXIT_TCP\n",
		errorText: "",
	},
	{
		name: "node vm escaped global properties",
		module: false,
		source:
			"const vm = require('node:vm');\nconst rows = [];\nfunction observe(label, read) {\n  try { rows.push([label, 'ok', read()]); }\n  catch (error) { rows.push([label, 'throw', String(error && error.name), String(error && error.message)]); }\n}\nconst sandbox = {};\nconst escaped = vm.runInNewContext('globalThis.answer = 41; globalThis', sandbox);\nobserve('sandbox', () => sandbox.answer);\nobserve('escaped-global', () => escaped.answer);\nobserve('ordinary-object', () => vm.runInNewContext('({ answer: 7 })').answer);\nobserve('child-closure', () => vm.runInNewContext('(() => 7)')());\nconsole.log(JSON.stringify(rows));\n",
		exitCode: 0,
		stdout:
			'[["sandbox","ok",41],["escaped-global","ok",41],["ordinary-object","ok",7],["child-closure","ok",7]]\n',
		errorText: "",
	},
	{
		name: "node vm escaped global read write identity",
		module: false,
		source:
			"const vm = require('node:vm');\nconst rows = [];\nfunction observe(label, read) {\n  try { rows.push([label, 'ok', read()]); }\n  catch (error) { rows.push([label, 'throw', String(error && error.name), String(error && error.message)]); }\n}\nconst shared = { value: 3 };\nconst sandbox = { shared };\nconst context = vm.createContext(sandbox);\nconst escaped = vm.runInContext('globalThis.answer=41;globalThis', context);\nobserve('identity', () => vm.runInContext('globalThis', context) === escaped);\nobserve('read', () => escaped.answer);\nobserve('write', () => { escaped.answer = 42; return escaped.answer; });\nobserve('sandbox-after-write', () => sandbox.answer);\nobserve('child-reread', () => vm.runInContext('answer', context));\nobserve('same-after-reread', () => vm.runInContext('globalThis', context) === escaped);\nobserve('child-write', () => vm.runInContext('answer=43;answer', context));\nobserve('parent-reread', () => escaped.answer);\nobserve('object-identity', () => vm.runInContext('shared', context) === shared);\nobserve('separate-context', () => vm.runInNewContext('globalThis') !== escaped);\nconsole.log(JSON.stringify(rows));\n",
		exitCode: 0,
		stdout:
			'[["identity","ok",true],["read","ok",41],["write","ok",42],["sandbox-after-write","ok",42],["child-reread","ok",42],["same-after-reread","ok",true],["child-write","ok",43],["parent-reread","ok",43],["object-identity","ok",true],["separate-context","ok",true]]\n',
		errorText: "",
	},
	{
		name: "node vm live descriptors deletes and keys",
		module: false,
		source:
			"const vm = require('node:vm');\nconst key = Symbol('vm-key');\nconst sandbox = { 2: 'two', z: 3 };\nsandbox[key] = 4;\nObject.defineProperty(sandbox, 'hidden', { value: 5, writable: false, enumerable: false, configurable: false });\nconst context = vm.createContext(sandbox);\nconst escaped = vm.runInContext('globalThis', context);\nconst keys = Reflect.ownKeys(escaped).filter(k => k === '2' || k === 'z' || k === 'hidden' || k === key).map(k => typeof k === 'symbol' ? 'symbol' : k);\nconst descriptor = Object.getOwnPropertyDescriptor(escaped, 'hidden');\nlet strictError = 'NO_THROW';\ntry { vm.runInContext(\"'use strict';hidden=9\", context); } catch (error) { strictError = error.name; }\nconst hiddenDelete = delete escaped.hidden;\nconst hiddenDefine = Reflect.defineProperty(escaped, 'hidden', { value: 9 });\nconst hiddenSet = Reflect.set(escaped, 'hidden', 9);\nescaped.visible = 6;\nconst visible = sandbox.visible;\nconst visibleDelete = delete escaped.visible;\nconst visibleGone = !Object.prototype.hasOwnProperty.call(sandbox, 'visible');\ndelete sandbox.z;\nconst zGone = escaped.z === undefined;\nObject.defineProperty(escaped, 'defined', { value: 8, writable: true, enumerable: true, configurable: true });\nconst defined = Object.getOwnPropertyDescriptor(sandbox, 'defined');\nescaped[key] = 9;\nescaped[2] = 'changed';\nconsole.log(JSON.stringify({ keys, hidden: [descriptor.value, descriptor.writable, descriptor.enumerable, descriptor.configurable], strictError, hiddenDelete, hiddenDefine, hiddenSet, visible, visibleDelete, visibleGone, zGone, defined: [defined.value, defined.writable, defined.enumerable, defined.configurable], symbol: sandbox[key], index: sandbox[2] }));\n",
		exitCode: 0,
		stdout:
			'{"keys":["2","z","hidden","symbol"],"hidden":[5,false,false,false],"strictError":"TypeError","hiddenDelete":false,"hiddenDefine":false,"hiddenSet":false,"visible":6,"visibleDelete":true,"visibleGone":true,"zGone":true,"defined":[8,true,true,true],"symbol":9,"index":"changed"}\n',
		errorText: "",
	},
	{
		name: "node vm live accessors and independence",
		module: false,
		source:
			"const vm = require('node:vm');\nconst trace = [];\nlet value = 1;\nconst sandbox = {};\nsandbox.self = sandbox;\nObject.defineProperty(sandbox, 'access', { configurable: true, enumerable: true,\n  get() { trace.push('get:' + (this === sandbox)); return value; },\n  set(next) { trace.push('set:' + (this === sandbox) + ':' + next); value = next; }\n});\nconst context = vm.createContext(sandbox);\nconst noEagerGet = trace.length === 0;\nconst escaped = vm.runInContext('globalThis', context);\nconst parentRead = escaped.access;\nescaped.access = 4;\nconst childRead = vm.runInContext('access=5;access', context);\nconst selfIdentity = escaped.self === escaped && vm.runInContext('self===globalThis', context);\nconst independent = vm.runInNewContext('typeof access');\nconst builtins = vm.runInContext(\"typeof process==='undefined' && typeof Buffer==='undefined' && Array===globalThis.Array\", context);\nlet missingName = 'NO_THROW';\ntry { vm.runInContext(\"'use strict';missingSyntheticName=1\", context); } catch (error) { missingName = error.name; }\nconsole.log(JSON.stringify({ noEagerGet, parentRead, childRead, value, trace, selfIdentity, independent, builtins, missingName }));\n",
		exitCode: 0,
		stdout:
			'{"noEagerGet":true,"parentRead":1,"childRead":5,"value":5,"trace":["get:true","set:true:4","set:true:5","get:true"],"selfIdentity":true,"independent":"undefined","builtins":true,"missingName":"ReferenceError"}\n',
		errorText: "",
	},
	{
		name: "node vm partial descriptor diagnostics",
		module: false,
		source:
			"const vm = require('node:vm');\nconst trace = [];\nfunction getOne() { return 1; }\nfunction getTwo() { return 2; }\nfunction setOne(value) { trace.push('setOne:' + value); }\nfunction setTwo(value) { trace.push('setTwo:' + value); }\nconst sandbox = {};\nfor (const [name, value] of [['enumOnly', 17], ['writeOnly', 18], ['emptyOnly', 19]]) {\n  Object.defineProperty(sandbox, name, { value, writable: true, enumerable: true, configurable: true });\n}\nfor (const name of ['getOnly', 'setOnly']) {\n  Object.defineProperty(sandbox, name, { get: getOne, set: setOne, enumerable: true, configurable: true });\n}\nconst context = vm.createContext(sandbox);\nconst escaped = vm.runInContext('globalThis', context);\nfunction descriptor(object, name) {\n  const value = Object.getOwnPropertyDescriptor(object, name);\n  return { present: value !== undefined, hasValue: Object.prototype.hasOwnProperty.call(value, 'value'),\n    valueType: typeof value.value, value: value.value ?? null, writable: value.writable ?? null,\n    enumerable: value.enumerable, configurable: value.configurable,\n    getOne: value.get === getOne, getTwo: value.get === getTwo,\n    setOne: value.set === setOne, setTwo: value.set === setTwo };\n}\nfunction update(name, value) {\n  let caught = false, error;\n  try { Object.defineProperty(escaped, name, value); } catch (actual) { caught = true; error = actual; }\n  return { name, caught, errorName: error?.name ?? null, errorMessage: error?.message ?? null,\n    sandbox: descriptor(sandbox, name), global: descriptor(escaped, name) };\n}\nconst rows = [update('enumOnly', { enumerable: false }), update('writeOnly', { writable: false }),\n  update('emptyOnly', {}), update('getOnly', { get: getTwo }), update('setOnly', { set: setTwo })];\nlet getRead, setRead;\ntry { getRead = { type: typeof escaped.getOnly, value: escaped.getOnly ?? null }; } catch (error) { getRead = { error: error.name }; }\ntry { escaped.getOnly = 3; } catch (error) { trace.push('getOnly-write-error:' + error.name); }\ntry { escaped.setOnly = 4; setRead = { type: typeof escaped.setOnly, value: escaped.setOnly ?? null }; } catch (error) { setRead = { error: error.name }; }\nconsole.log(JSON.stringify({ rows, getRead, setRead, trace }));\n",
		exitCode: 0,
		stdout:
			'{"rows":[{"name":"enumOnly","caught":false,"errorName":null,"errorMessage":null,"sandbox":{"present":true,"hasValue":true,"valueType":"number","value":17,"writable":true,"enumerable":false,"configurable":true,"getOne":false,"getTwo":false,"setOne":false,"setTwo":false},"global":{"present":true,"hasValue":true,"valueType":"number","value":17,"writable":true,"enumerable":false,"configurable":true,"getOne":false,"getTwo":false,"setOne":false,"setTwo":false}},{"name":"writeOnly","caught":false,"errorName":null,"errorMessage":null,"sandbox":{"present":true,"hasValue":true,"valueType":"undefined","value":null,"writable":false,"enumerable":true,"configurable":true,"getOne":false,"getTwo":false,"setOne":false,"setTwo":false},"global":{"present":true,"hasValue":true,"valueType":"undefined","value":null,"writable":false,"enumerable":true,"configurable":true,"getOne":false,"getTwo":false,"setOne":false,"setTwo":false}},{"name":"emptyOnly","caught":false,"errorName":null,"errorMessage":null,"sandbox":{"present":true,"hasValue":true,"valueType":"undefined","value":null,"writable":true,"enumerable":true,"configurable":true,"getOne":false,"getTwo":false,"setOne":false,"setTwo":false},"global":{"present":true,"hasValue":true,"valueType":"undefined","value":null,"writable":true,"enumerable":true,"configurable":true,"getOne":false,"getTwo":false,"setOne":false,"setTwo":false}},{"name":"getOnly","caught":false,"errorName":null,"errorMessage":null,"sandbox":{"present":true,"hasValue":false,"valueType":"undefined","value":null,"writable":null,"enumerable":true,"configurable":true,"getOne":false,"getTwo":true,"setOne":false,"setTwo":false},"global":{"present":true,"hasValue":false,"valueType":"undefined","value":null,"writable":null,"enumerable":true,"configurable":true,"getOne":false,"getTwo":true,"setOne":false,"setTwo":false}},{"name":"setOnly","caught":false,"errorName":null,"errorMessage":null,"sandbox":{"present":true,"hasValue":false,"valueType":"undefined","value":null,"writable":null,"enumerable":true,"configurable":true,"getOne":false,"getTwo":false,"setOne":false,"setTwo":true},"global":{"present":true,"hasValue":false,"valueType":"undefined","value":null,"writable":null,"enumerable":true,"configurable":true,"getOne":false,"getTwo":false,"setOne":false,"setTwo":true}}],"getRead":{"type":"number","value":2},"setRead":{"type":"undefined","value":null},"trace":["setTwo:4"]}\n',
		errorText: "",
	},
	{
		name: "node vm ScriptOrigin offsets",
		module: false,
		source:
			"const vm = require('node:vm');\nconst cases = [\n  { name: 'positive', lineOffset: 3, columnOffset: 0, source: '\\n\\nthrow new Error(\"ORIGIN_MARKER\")' },\n  { name: 'zero', lineOffset: 0, columnOffset: 0, source: '\\n\\nthrow new Error(\"ORIGIN_MARKER\")' },\n  { name: 'negative', lineOffset: -1, columnOffset: 0, source: '\\n\\nthrow new Error(\"ORIGIN_MARKER\")' },\n  { name: 'column-zero', lineOffset: 0, columnOffset: 0, source: 'throw new Error(\"ORIGIN_MARKER\")' },\n  { name: 'column-two', lineOffset: 0, columnOffset: 2, source: 'throw new Error(\"ORIGIN_MARKER\")' },\n];\nconst rows = [];\nfor (const row of cases) {\n  const context = vm.createContext({});\n  let caught = false, name, message, frame = null;\n  try { vm.runInContext(row.source, context, { filename: 'vm-origin-' + row.name + '.js', lineOffset: row.lineOffset, columnOffset: row.columnOffset }); }\n  catch (error) {\n    caught = true; name = error.name; message = error.message;\n    const match = String(error.stack).match(/vm-origin-[a-z-]+\\.js:(-?\\d+):(-?\\d+)/);\n    if (match) frame = { line: +match[1], column: +match[2] };\n  }\n  rows.push({ case: row.name, caught, name, message, frame, usable: vm.runInContext('6*7', context) === 42 });\n}\nconsole.log(JSON.stringify(rows));\n",
		exitCode: 0,
		stdout:
			'[{"case":"positive","caught":true,"name":"Error","message":"ORIGIN_MARKER","frame":{"line":6,"column":7},"usable":true},{"case":"zero","caught":true,"name":"Error","message":"ORIGIN_MARKER","frame":{"line":3,"column":7},"usable":true},{"case":"negative","caught":true,"name":"Error","message":"ORIGIN_MARKER","frame":{"line":2,"column":7},"usable":true},{"case":"column-zero","caught":true,"name":"Error","message":"ORIGIN_MARKER","frame":{"line":1,"column":7},"usable":true},{"case":"column-two","caught":true,"name":"Error","message":"ORIGIN_MARKER","frame":{"line":1,"column":9},"usable":true}]\n',
		errorText: "",
	},
	{
		name: "node vm proto query",
		module: false,
		source:
			"const vm = require('node:vm');\nconst sandbox = Object.create({ parentOnly: 7, 2: 8 });\nconst context = vm.createContext(sandbox);\nconst global = vm.runInContext('globalThis', context);\nconsole.log(JSON.stringify([\n  global.parentOnly, Reflect.has(global, 'parentOnly'),\n  global[2], Reflect.has(global, '2'),\n  vm.runInContext(\"'parentOnly' in globalThis && '2' in globalThis\", context)\n]));\n",
		exitCode: 0,
		stdout: "[7,false,8,false,false]\n",
		errorText: "",
	},
	{
		name: "node vm symbol brand",
		module: false,
		source:
			"const vm = require('node:vm');\nconst keys = ['agentos.vm.context', 'agentos.vm.context.id'].map(Symbol.for);\nconst sandbox = {};\nvm.createContext(sandbox);\nconst global = vm.runInContext('globalThis', sandbox);\nconsole.log(JSON.stringify({\n  sandboxSymbols: keys.map(key => Object.hasOwn(sandbox, key)),\n  globalSymbols: keys.map(key => Reflect.ownKeys(global).includes(key)),\n  derivedIsContext: vm.isContext(Object.create(sandbox))\n}));\n",
		exitCode: 0,
		stdout:
			'{"sandboxSymbols":[false,false],"globalSymbols":[false,false],"derivedIsContext":false}\n',
		errorText: "",
	},
	{
		name: "node vm import interop",
		module: true,
		source:
			"import vm, { createContext, isContext, runInContext, runInNewContext, runInThisContext, Script, compileFunction, measureMemory } from 'node:vm';\nimport { createRequire } from 'node:module';\nconst cjs = createRequire(import.meta.url)('node:vm');\nconst fromEsm = vm.createContext({ value: 10 });\nconst fromCjs = cjs.createContext({ value: 20 });\nconst names = ['Script', 'createContext', 'isContext', 'runInContext', 'runInNewContext', 'runInThisContext', 'compileFunction', 'measureMemory'];\nconsole.log(JSON.stringify({\n  recognized: [cjs.isContext(fromEsm), isContext(fromCjs)],\n  crossRun: [cjs.runInContext('++value', fromEsm), runInContext('++value', fromCjs)],\n  crossScript: [new cjs.Script('value + 1').runInContext(fromEsm), new Script('value + 1').runInContext(fromCjs)],\n  names: {\n    esm: [Script.name, createContext.name, isContext.name, runInContext.name, runInNewContext.name, runInThisContext.name, compileFunction.name, measureMemory.name],\n    cjs: names.map(name => cjs[name].name)\n  }\n}));\n",
		exitCode: 0,
		stdout:
			'{"recognized":[true,true],"crossRun":[11,21],"crossScript":[12,22],"names":{"esm":["Script","createContext","isContext","runInContext","runInNewContext","runInThisContext","compileFunction","measureMemory"],"cjs":["Script","createContext","isContext","runInContext","runInNewContext","runInThisContext","compileFunction","measureMemory"]}}\n',
		errorText: "",
	},
] as const;
