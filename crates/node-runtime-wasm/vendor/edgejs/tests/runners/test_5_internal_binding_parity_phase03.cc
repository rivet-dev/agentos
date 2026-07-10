#include <string>

#include "test_env.h"
#include "edge_version.h"
#include "edge_runtime.h"

class Test5InternalBindingParityPhase03 : public FixtureTestBase {};

namespace {

constexpr const char* kParityWaveScript = R"JS(
const assert = require('assert');
const fs = require('fs');
const os = require('os');
const path = require('path');

const constants = internalBinding('constants');
assert.ok(constants && typeof constants === 'object');
assert.ok(constants.os && typeof constants.os === 'object');
assert.ok(constants.fs && typeof constants.fs === 'object');
assert.ok(constants.crypto && typeof constants.crypto === 'object');
assert.ok(constants.zlib && typeof constants.zlib === 'object');
assert.ok(constants.internal && typeof constants.internal === 'object');
assert.strictEqual(constants.internal.EXTENSIONLESS_FORMAT_JAVASCRIPT, 0);
assert.strictEqual(constants.internal.EXTENSIONLESS_FORMAT_WASM, 1);

const config = internalBinding('config');
assert.ok(config && typeof config === 'object');
assert.strictEqual(typeof config.getDefaultLocale, 'function');
assert.strictEqual(config.hasOpenSSL, !!(process.versions && process.versions.openssl));
assert.strictEqual(process.versions.edge, ')JS" EDGE_VERSION_STRING R"JS(');
assert.strictEqual(typeof config.bits, 'number');
assert.ok(config.bits === 32 || config.bits === 64);
const locale = config.getDefaultLocale();
assert.ok(locale === undefined || typeof locale === 'string');

const asyncWrap = internalBinding('async_wrap');
assert.ok(asyncWrap && typeof asyncWrap === 'object');
assert.strictEqual(typeof asyncWrap.setupHooks, 'function');
assert.strictEqual(typeof asyncWrap.setCallbackTrampoline, 'function');
assert.strictEqual(typeof asyncWrap.pushAsyncContext, 'function');
assert.strictEqual(typeof asyncWrap.popAsyncContext, 'function');
assert.strictEqual(typeof asyncWrap.executionAsyncResource, 'function');
assert.strictEqual(typeof asyncWrap.clearAsyncIdStack, 'function');
assert.strictEqual(typeof asyncWrap.queueDestroyAsyncId, 'function');
assert.strictEqual(typeof asyncWrap.setPromiseHooks, 'function');
assert.strictEqual(typeof asyncWrap.getPromiseHooks, 'function');
assert.strictEqual(typeof asyncWrap.registerDestroyHook, 'function');
assert.ok(asyncWrap.async_hook_fields instanceof Uint32Array);
assert.ok(asyncWrap.async_id_fields instanceof Float64Array);
assert.ok(asyncWrap.async_ids_stack instanceof Float64Array);
assert.ok(Array.isArray(asyncWrap.execution_async_resources));
assert.strictEqual(asyncWrap.constants.kInit, 0);
assert.strictEqual(asyncWrap.constants.kBefore, 1);
assert.strictEqual(asyncWrap.constants.kAfter, 2);
assert.strictEqual(asyncWrap.constants.kDestroy, 3);
assert.strictEqual(asyncWrap.constants.kPromiseResolve, 4);
assert.strictEqual(asyncWrap.constants.kStackLength, 7);
assert.strictEqual(asyncWrap.constants.kUsesExecutionAsyncResource, 8);
assert.strictEqual(typeof asyncWrap.Providers.PROMISE, 'number');
const destroyCalls = [];
asyncWrap.setupHooks({
  init() {},
  before() {},
  after() {},
  destroy(id) { destroyCalls.push(id); },
  promise_resolve() {},
});
asyncWrap.queueDestroyAsyncId(321);
// Destroy hooks are dispatched through a deferred platform task, matching
// Node's deferred queueDestroyAsyncId semantics. The drained result is
// asserted by a follow-up script after the event loop goes idle.
globalThis.__edge_destroy_calls = destroyCalls;
asyncWrap.setPromiseHooks(() => {}, undefined, () => {}, undefined);
const promiseHooks = asyncWrap.getPromiseHooks();
assert.ok(Array.isArray(promiseHooks));
assert.strictEqual(promiseHooks.length, 4);
assert.strictEqual(typeof promiseHooks[0], 'function');
assert.strictEqual(typeof promiseHooks[2], 'function');
const stackLenField = asyncWrap.constants.kStackLength;
const execField = asyncWrap.constants.kExecutionAsyncId;
const trigField = asyncWrap.constants.kTriggerAsyncId;
const prevStackLen = asyncWrap.async_hook_fields[stackLenField];
const prevExec = asyncWrap.async_id_fields[execField];
const prevTrig = asyncWrap.async_id_fields[trigField];
asyncWrap.execution_async_resources[prevStackLen] = { marker: 1 };
asyncWrap.pushAsyncContext(123, 456);
assert.strictEqual(asyncWrap.async_hook_fields[stackLenField], prevStackLen + 1);
assert.strictEqual(asyncWrap.async_id_fields[execField], 123);
assert.strictEqual(asyncWrap.async_id_fields[trigField], 456);
assert.strictEqual(asyncWrap.executionAsyncResource(prevStackLen).marker, 1);
assert.strictEqual(asyncWrap.popAsyncContext(123), prevStackLen > 0);
assert.strictEqual(asyncWrap.async_id_fields[execField], prevExec);
assert.strictEqual(asyncWrap.async_id_fields[trigField], prevTrig);
asyncWrap.pushAsyncContext(10, 11);
asyncWrap.clearAsyncIdStack();
assert.strictEqual(asyncWrap.async_hook_fields[stackLenField], 0);
assert.strictEqual(asyncWrap.async_id_fields[execField], 0);
assert.strictEqual(asyncWrap.async_id_fields[trigField], 0);
asyncWrap.setCallbackTrampoline(() => {});
asyncWrap.setCallbackTrampoline(undefined);
const destroyTrackedObj = {};
asyncWrap.registerDestroyHook(destroyTrackedObj, 7, { destroyed: false });

const bufferBinding = internalBinding('buffer');
assert.ok(bufferBinding && typeof bufferBinding === 'object');
assert.strictEqual(typeof bufferBinding.getZeroFillToggle, 'function');
const zeroFillToggle = bufferBinding.getZeroFillToggle();
assert.ok(zeroFillToggle instanceof Uint32Array);
assert.strictEqual(zeroFillToggle.length, 1);

const encodingBinding = internalBinding('encoding_binding');
assert.ok(encodingBinding && typeof encodingBinding === 'object');
assert.deepStrictEqual(
  Object.keys(encodingBinding).sort(),
  ['decodeUTF8', 'encodeInto', 'encodeIntoResults', 'encodeUtf8String', 'toASCII', 'toUnicode'],
);
assert.ok(encodingBinding.encodeIntoResults instanceof Uint32Array);
assert.strictEqual(encodingBinding.encodeIntoResults.length, 2);
for (const key of [
  'decodeLatin1',
  'decodeUtf8',
  'encodeBase64',
  'encodeUtf8',
  'decodeBase64',
  'utf8ByteLength',
  'validateUtf8',
]) {
  assert.ok(!(key in encodingBinding), key);
}

const symbolsBinding = internalBinding('symbols');
assert.ok(symbolsBinding && typeof symbolsBinding === 'object');
for (const key of [
  'fs_use_promises_symbol',
  'constructor_key_symbol',
  'handle_onclose',
  'messaging_deserialize_symbol',
  'messaging_transfer_symbol',
  'messaging_clone_symbol',
  'messaging_transfer_list_symbol',
  'oninit',
  'vm_context_no_contextify',
  'vm_dynamic_import_default_internal',
  'vm_dynamic_import_main_context_default',
  'vm_dynamic_import_missing_flag',
  'vm_dynamic_import_no_callback',
]) {
  assert.strictEqual(typeof symbolsBinding[key], 'symbol');
}

const utilBinding = internalBinding('util');
assert.ok(utilBinding && typeof utilBinding === 'object');
assert.strictEqual(utilBinding.constants.kPending, 0);
assert.strictEqual(utilBinding.constants.kFulfilled, 1);
assert.strictEqual(utilBinding.constants.kRejected, 2);
assert.strictEqual(utilBinding.constants.kCloneable, 2);
assert.strictEqual(typeof utilBinding.getCallerLocation, 'function');
const loc = utilBinding.getCallerLocation();
assert.ok(loc === undefined || (Array.isArray(loc) && loc.length === 3));
if (loc !== undefined) {
  assert.strictEqual(typeof loc[0], 'number');
  assert.strictEqual(typeof loc[1], 'number');
  assert.strictEqual(typeof loc[2], 'string');
}
assert.strictEqual(typeof utilBinding.privateSymbols.arrow_message_private_symbol, 'symbol');
assert.strictEqual(typeof utilBinding.privateSymbols.decorated_private_symbol, 'symbol');
assert.strictEqual(typeof utilBinding.isAnyArrayBuffer, 'function');
assert.strictEqual(typeof utilBinding.isArrayBuffer, 'function');
assert.strictEqual(typeof utilBinding.isArrayBufferView, 'function');
assert.strictEqual(typeof utilBinding.isAsyncFunction, 'function');
assert.strictEqual(typeof utilBinding.isDataView, 'function');
assert.strictEqual(typeof utilBinding.isDate, 'function');
assert.strictEqual(typeof utilBinding.isExternal, 'function');
assert.strictEqual(typeof utilBinding.isMap, 'function');
assert.strictEqual(typeof utilBinding.isMapIterator, 'function');
assert.strictEqual(typeof utilBinding.isNativeError, 'function');
assert.strictEqual(typeof utilBinding.isPromise, 'function');
assert.strictEqual(typeof utilBinding.isRegExp, 'function');
assert.strictEqual(typeof utilBinding.isSet, 'function');
assert.strictEqual(typeof utilBinding.isSetIterator, 'function');
assert.strictEqual(typeof utilBinding.isTypedArray, 'function');
assert.strictEqual(typeof utilBinding.isUint8Array, 'function');
assert.strictEqual(utilBinding.isArrayBuffer(new ArrayBuffer(8)), true);
assert.strictEqual(utilBinding.isArrayBufferView(new Uint8Array(4)), true);
assert.strictEqual(utilBinding.isTypedArray(new Uint16Array(2)), true);
assert.strictEqual(utilBinding.isUint8Array(new Uint8Array(2)), true);
assert.strictEqual(utilBinding.isMap(new Map()), true);
assert.strictEqual(utilBinding.isSet(new Set()), true);
assert.strictEqual(utilBinding.isPromise(Promise.resolve(1)), true);
assert.strictEqual(utilBinding.isRegExp(/x/), true);
assert.strictEqual(utilBinding.isDate(new Date()), true);
const callSites = utilBinding.getCallSites(3);
assert.ok(Array.isArray(callSites));
assert.ok(callSites.length >= 0 && callSites.length <= 3);
if (callSites.length > 0) {
  assert.strictEqual(typeof callSites[0].scriptId, 'string');
  assert.ok(!String(callSites[0].scriptName).includes('node:util'));
}

const messagingBinding = internalBinding('messaging');
assert.ok(messagingBinding && typeof messagingBinding === 'object');
const domException = new messagingBinding.DOMException('boom');
assert.strictEqual(domException[utilBinding.privateSymbols.transfer_mode_private_symbol], utilBinding.constants.kCloneable);
assert.strictEqual(typeof messagingBinding.DOMException.prototype[symbolsBinding.messaging_clone_symbol], 'function');
assert.strictEqual(typeof messagingBinding.DOMException.prototype[symbolsBinding.messaging_deserialize_symbol], 'function');

const proxyTarget = { a: 1 };
const proxyHandler = {
  get(target, prop) {
    return target[prop];
  },
};
const proxy = new Proxy(proxyTarget, proxyHandler);
const proxyDetails = utilBinding.getProxyDetails(proxy, true);
assert.strictEqual(proxyDetails[0], proxyTarget);
assert.strictEqual(proxyDetails[1], proxyHandler);
assert.strictEqual(utilBinding.getProxyDetails(proxy, false), proxyTarget);

const revocable = Proxy.revocable({}, {});
revocable.revoke();
const revokedDetails = utilBinding.getProxyDetails(revocable.proxy, true);
assert.strictEqual(revokedDetails[0], null);
assert.strictEqual(revokedDetails[1], null);
assert.strictEqual(utilBinding.getProxyDetails(revocable.proxy, false), null);

const errorsBinding = internalBinding('errors');
errorsBinding.setSourceMapsEnabled(true);
errorsBinding.setGetSourceMapErrorSource((file, line, column) =>
  `mapped:${file}:${line}:${column}`);
const syntheticError = { stack: 'Error: boom\n    at fn (/tmp/example.js:7:9)' };
const sourcePos = errorsBinding.getErrorSourcePositions(syntheticError);
assert.strictEqual(sourcePos.sourceLine, 'mapped:/tmp/example.js:7:9');
errorsBinding.setSourceMapsEnabled(false);

const traceEvents = internalBinding('trace_events');
assert.ok(traceEvents && typeof traceEvents === 'object');
assert.strictEqual(typeof traceEvents.getEnabledCategories, 'function');
assert.strictEqual(typeof traceEvents.getCategoryEnabledBuffer, 'function');
assert.strictEqual(typeof traceEvents.isTraceCategoryEnabled, 'function');
assert.strictEqual(typeof traceEvents.setTraceCategoryStateUpdateHandler, 'function');
assert.strictEqual(typeof traceEvents.CategorySet, 'function');
const nodeHttpBuffer = traceEvents.getCategoryEnabledBuffer('node.http');
assert.ok(nodeHttpBuffer instanceof Uint8Array);
assert.strictEqual(nodeHttpBuffer.length, 1);
assert.strictEqual(traceEvents.getCategoryEnabledBuffer('node.http'), nodeHttpBuffer);
assert.strictEqual(nodeHttpBuffer[0], 0);
let traceStateCalls = 0;
traceEvents.setTraceCategoryStateUpdateHandler((enabled) => {
  traceStateCalls++;
  assert.strictEqual(typeof enabled, 'boolean');
});
const categorySet = new traceEvents.CategorySet(['node.http', 'node.async_hooks']);
categorySet.enable();
assert.strictEqual(traceEvents.isTraceCategoryEnabled('node.http'), true);
assert.strictEqual(nodeHttpBuffer[0], 1);
assert.ok(traceEvents.getEnabledCategories().includes('node.http'));
const asyncHooksBuffer = traceEvents.getCategoryEnabledBuffer('node.async_hooks');
assert.strictEqual(asyncHooksBuffer[0], 1);
categorySet.disable();
assert.strictEqual(traceEvents.isTraceCategoryEnabled('node.http'), false);
assert.strictEqual(nodeHttpBuffer[0], 0);
assert.ok(traceStateCalls >= 2);

const uv = internalBinding('uv');
assert.strictEqual(typeof uv.UV_UNKNOWN, 'number');
assert.strictEqual(typeof uv.UV_EAI_MEMORY, 'number');
const uvMap = uv.getErrorMap();
assert.ok(uvMap instanceof Map);
assert.ok(uvMap.has(uv.UV_EINVAL));
assert.strictEqual(typeof uv.getErrorMessage(uv.UV_EINVAL), 'string');

const contextify = internalBinding('contextify');
assert.ok(contextify && typeof contextify === 'object');
assert.strictEqual(typeof contextify.makeContext, 'function');
assert.strictEqual(typeof contextify.measureMemory, 'function');
assert.strictEqual(typeof contextify.startSigintWatchdog, 'function');
assert.strictEqual(typeof contextify.stopSigintWatchdog, 'function');
assert.strictEqual(typeof contextify.watchdogHasPendingSigint, 'function');
assert.ok(contextify.constants && typeof contextify.constants === 'object');
assert.strictEqual(contextify.constants.measureMemory.mode.SUMMARY, 0);
assert.strictEqual(contextify.constants.measureMemory.mode.DETAILED, 1);
assert.strictEqual(contextify.constants.measureMemory.execution.DEFAULT, 0);
assert.strictEqual(contextify.constants.measureMemory.execution.EAGER, 1);
const hdo = Symbol('hostDefinedOption');
const sandbox = {};
const contextified = contextify.makeContext(sandbox, 'ctx', undefined, true, true, false, hdo);
assert.strictEqual(contextified, sandbox);
const contextSymbol = utilBinding.privateSymbols.contextify_context_private_symbol;
assert.strictEqual(contextified[contextSymbol], contextified);
assert.strictEqual(typeof contextify.watchdogHasPendingSigint(), 'boolean');
const measureMemoryPromise = contextify.measureMemory(
  contextify.constants.measureMemory.mode.SUMMARY,
  contextify.constants.measureMemory.execution.DEFAULT,
);
assert.ok(measureMemoryPromise && typeof measureMemoryPromise.then === 'function');

const cryptoBinding = internalBinding('crypto');
assert.ok(cryptoBinding && typeof cryptoBinding === 'object');
assert.strictEqual(typeof cryptoBinding.Hash, 'function');
assert.strictEqual(typeof cryptoBinding.Hmac, 'function');
assert.strictEqual(typeof cryptoBinding.Sign, 'function');
assert.strictEqual(typeof cryptoBinding.Verify, 'function');
assert.strictEqual(typeof cryptoBinding.SecureContext, 'function');
assert.strictEqual(typeof cryptoBinding.KeyObjectHandle, 'function');
assert.strictEqual(typeof cryptoBinding.createNativeKeyObjectClass, 'function');
assert.strictEqual(typeof cryptoBinding.RandomBytesJob, 'function');
assert.strictEqual(typeof cryptoBinding.PBKDF2Job, 'function');
assert.strictEqual(typeof cryptoBinding.ScryptJob, 'function');
assert.strictEqual(typeof cryptoBinding.HKDFJob, 'function');
assert.strictEqual(typeof cryptoBinding.HashJob, 'function');
assert.strictEqual(typeof cryptoBinding.SignJob, 'function');
assert.strictEqual(typeof cryptoBinding.oneShotDigest, 'function');
assert.strictEqual(typeof cryptoBinding.timingSafeEqual, 'function');
assert.strictEqual(typeof cryptoBinding.getFipsCrypto, 'function');
assert.strictEqual(typeof cryptoBinding.setFipsCrypto, 'function');
assert.strictEqual(typeof cryptoBinding.testFipsCrypto, 'function');
assert.strictEqual(typeof cryptoBinding.kCryptoJobAsync, 'number');
assert.strictEqual(typeof cryptoBinding.kCryptoJobSync, 'number');
assert.strictEqual(typeof cryptoBinding.kSignJobModeSign, 'number');
assert.strictEqual(typeof cryptoBinding.kSignJobModeVerify, 'number');
assert.strictEqual(typeof cryptoBinding.kWebCryptoCipherEncrypt, 'number');
assert.strictEqual(typeof cryptoBinding.kWebCryptoCipherDecrypt, 'number');
assert.strictEqual(typeof cryptoBinding.kKeyTypeSecret, 'number');
assert.strictEqual(typeof cryptoBinding.kKeyTypePublic, 'number');
assert.strictEqual(typeof cryptoBinding.kKeyTypePrivate, 'number');
assert.ok(Array.isArray(cryptoBinding.getBundledRootCertificates()));
assert.ok(Array.isArray(cryptoBinding.getExtraCACertificates()));
assert.ok(Array.isArray(cryptoBinding.getSystemCACertificates()));
assert.ok(Array.isArray(cryptoBinding.getUserRootCertificates()));
assert.strictEqual(typeof cryptoBinding.getCachedAliases(), 'object');
assert.strictEqual(typeof cryptoBinding.getOpenSSLSecLevelCrypto(), 'number');
const secureHeap = cryptoBinding.secureHeapUsed();
assert.ok(
  secureHeap === undefined ||
  typeof secureHeap === 'number' ||
  typeof secureHeap === 'bigint'
);
assert.strictEqual(
  cryptoBinding.oneShotDigest('sha256', -1, {}, Buffer.from('abc'), 'hex', 0),
  'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
);
assert.strictEqual(
  cryptoBinding.timingSafeEqual(Buffer.from([1, 2, 3]), Buffer.from([1, 2, 3])),
  true,
);
assert.strictEqual(
  cryptoBinding.timingSafeEqual(Buffer.from([1, 2, 3]), Buffer.from([1, 9, 3])),
  false,
);
const hashHandle = new cryptoBinding.Hash('sha256');
hashHandle.update(Buffer.from('abc'));
assert.strictEqual(
  hashHandle.digest('hex'),
  'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
);
const hmacHandle = new cryptoBinding.Hmac();
hmacHandle.init('sha256', Buffer.from('key'));
hmacHandle.update(Buffer.from('abc'));
assert.strictEqual(
  hmacHandle.digest('hex'),
  '9c196e32dc0175f86f4b1cb89289d6619de6bee699e4c378e68309ed97a1a6ab',
);
const randomFillTarget = new Uint8Array(16);
const randomJob = new cryptoBinding.RandomBytesJob(
  cryptoBinding.kCryptoJobSync,
  randomFillTarget,
  0,
  randomFillTarget.length,
);
const randomJobResult = randomJob.run();
assert.strictEqual(randomJobResult[0], undefined);
assert.strictEqual(randomFillTarget.length, 16);
const pbkdf2Job = new cryptoBinding.PBKDF2Job(
  cryptoBinding.kCryptoJobSync,
  Buffer.from('password'),
  Buffer.from('salt'),
  2,
  16,
  'sha256',
);
const pbkdf2JobResult = pbkdf2Job.run();
assert.strictEqual(pbkdf2JobResult[0], undefined);
assert.strictEqual(Buffer.from(pbkdf2JobResult[1]).length, 16);
const scryptJob = new cryptoBinding.ScryptJob(
  cryptoBinding.kCryptoJobSync,
  Buffer.from('password'),
  Buffer.from('salt'),
  1024,
  8,
  1,
  8 << 20,
  16,
);
const scryptJobResult = scryptJob.run();
assert.strictEqual(scryptJobResult[0], undefined);
assert.strictEqual(Buffer.from(scryptJobResult[1]).length, 16);
const hkdfJob = new cryptoBinding.HKDFJob(
  cryptoBinding.kCryptoJobSync,
  'sha256',
  Buffer.from('ikm'),
  Buffer.from('salt'),
  Buffer.from('info'),
  16,
);
const hkdfJobResult = hkdfJob.run();
assert.strictEqual(hkdfJobResult[0], undefined);
assert.strictEqual(Buffer.from(hkdfJobResult[1]).length, 16);
const hashJob = new cryptoBinding.HashJob(
  cryptoBinding.kCryptoJobSync,
  'sha256',
  Buffer.from('abc'),
);
const hashJobResult = hashJob.run();
assert.strictEqual(hashJobResult[0], undefined);
assert.strictEqual(
  Buffer.from(hashJobResult[1]).toString('hex'),
  'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
);
const secretHandleA = new cryptoBinding.KeyObjectHandle();
secretHandleA.init(cryptoBinding.kKeyTypeSecret, Buffer.from('shared-secret'));
const secretHandleB = new cryptoBinding.KeyObjectHandle();
secretHandleB.init(cryptoBinding.kKeyTypeSecret, Buffer.from('shared-secret'));
assert.strictEqual(secretHandleA.getSymmetricKeySize(), 13);
assert.strictEqual(Buffer.from(secretHandleA.export()).toString('utf8'), 'shared-secret');
assert.strictEqual(secretHandleA.equals(secretHandleB), true);
const nativeClasses = cryptoBinding.createNativeKeyObjectClass((NativeKeyObject) => {
  class GenericKey extends NativeKeyObject {
    constructor(handle) {
      super(handle);
    }
  }
  return [GenericKey, GenericKey, GenericKey, GenericKey];
});
assert.ok(Array.isArray(nativeClasses));
assert.strictEqual(nativeClasses.length, 4);
const secureContextHandle = new cryptoBinding.SecureContext();
secureContextHandle.init(undefined, 0, 0);
secureContextHandle.setOptions(0);
assert.strictEqual(typeof secureContextHandle.getMinProto(), 'number');
assert.strictEqual(typeof secureContextHandle.getMaxProto(), 'number');
secureContextHandle.close();

const moduleWrapBinding = internalBinding('module_wrap');
assert.ok(moduleWrapBinding && typeof moduleWrapBinding === 'object');
assert.strictEqual(moduleWrapBinding.kUninstantiated, 0);
assert.strictEqual(moduleWrapBinding.kInstantiating, 1);
assert.strictEqual(moduleWrapBinding.kInstantiated, 2);
assert.strictEqual(moduleWrapBinding.kEvaluating, 3);
assert.strictEqual(moduleWrapBinding.kEvaluated, 4);
assert.strictEqual(moduleWrapBinding.kErrored, 5);
assert.strictEqual(moduleWrapBinding.kSourcePhase, 1);
assert.strictEqual(moduleWrapBinding.kEvaluationPhase, 2);
assert.strictEqual(typeof moduleWrapBinding.ModuleWrap, 'function');
assert.strictEqual(typeof moduleWrapBinding.createRequiredModuleFacade, 'function');
assert.strictEqual(typeof moduleWrapBinding.setImportModuleDynamicallyCallback, 'function');
assert.strictEqual(typeof moduleWrapBinding.setInitializeImportMetaObjectCallback, 'function');
assert.strictEqual(typeof moduleWrapBinding.throwIfPromiseRejected, 'function');
const moduleWrap = new moduleWrapBinding.ModuleWrap('vm:test', undefined, 'export const x = 1;', 0, 0);
assert.strictEqual(moduleWrap.getStatus(), moduleWrapBinding.kUninstantiated);
moduleWrap.instantiate();
assert.ok(moduleWrap.getStatus() >= moduleWrapBinding.kInstantiated);
const moduleEval = moduleWrap.evaluate();
assert.ok(moduleEval && typeof moduleEval.then === 'function');
const syntheticWrap = new moduleWrapBinding.ModuleWrap('vm:synthetic', undefined, ['x'], () => {});
syntheticWrap.instantiateSync();
syntheticWrap.evaluateSync();
syntheticWrap.setExport('x', 42);
assert.strictEqual(syntheticWrap.getNamespace().x, 42);
moduleWrapBinding.throwIfPromiseRejected(Promise.resolve());

const fsBinding = internalBinding('fs');
const fsConstants = constants.fs;
assert.strictEqual(typeof fsBinding.FSReqCallback, 'function');
assert.strictEqual(typeof fsBinding.FileHandle, 'function');
assert.strictEqual(typeof fsBinding.StatWatcher, 'function');
assert.strictEqual(typeof fsBinding.access, 'function');
assert.strictEqual(typeof fsBinding.internalModuleStat, 'function');
assert.strictEqual(typeof fsBinding.legacyMainResolve, 'function');
assert.strictEqual(typeof fsBinding.openFileHandle, 'function');
assert.strictEqual(typeof fsBinding.read, 'function');
assert.strictEqual(typeof fsBinding.readBuffers, 'function');
assert.strictEqual(typeof fsBinding.writeString, 'function');
assert.strictEqual(typeof fsBinding.writeBuffers, 'function');
assert.strictEqual(typeof fsBinding.statfs, 'function');
assert.strictEqual(typeof fsBinding.cpSyncCheckPaths, 'function');
assert.strictEqual(typeof fsBinding.cpSyncCopyDir, 'function');
assert.strictEqual(typeof fsBinding.cpSyncOverrideFile, 'function');
assert.strictEqual(typeof fsBinding.kUsePromises, 'symbol');
assert.strictEqual(fsBinding.kFsStatsFieldsNumber, 18);
assert.ok(fsBinding.statValues instanceof Float64Array);
assert.ok(fsBinding.bigintStatValues instanceof BigInt64Array);
assert.ok(fsBinding.statFsValues instanceof Float64Array);
assert.ok(fsBinding.bigintStatFsValues instanceof BigInt64Array);
assert.strictEqual(typeof fsBinding.getFormatOfExtensionlessFile, 'function');
assert.strictEqual(typeof fsBinding.accessSync, 'undefined');
assert.strictEqual(typeof fsBinding.readSync, 'undefined');
assert.strictEqual(typeof fsBinding.writeSync, 'undefined');
assert.strictEqual(typeof fsBinding.writeSyncString, 'undefined');
assert.strictEqual(typeof fsBinding.F_OK, 'undefined');
assert.strictEqual(typeof fsBinding.O_RDONLY, 'undefined');
const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'edge-fmt-'));
const wasmPath = path.join(tmpRoot, 'mod');
const jsPath = path.join(tmpRoot, 'main');
const wasmFd = fsBinding.open(
  wasmPath,
  fsConstants.O_WRONLY | fsConstants.O_CREAT | fsConstants.O_TRUNC,
  0o666,
);
fsBinding.writeBuffer(wasmFd, new Uint8Array([0x00, 0x61, 0x73, 0x6d]), 0, 4, 0);
fsBinding.close(wasmFd);
fs.writeFileSync(jsPath, 'console.log(1);');
assert.strictEqual(
  fsBinding.getFormatOfExtensionlessFile(wasmPath),
  constants.internal.EXTENSIONLESS_FORMAT_WASM,
);
assert.strictEqual(
  fsBinding.getFormatOfExtensionlessFile(jsPath),
  constants.internal.EXTENSIONLESS_FORMAT_JAVASCRIPT,
);
assert.strictEqual(fsBinding.internalModuleStat(jsPath), 0);
assert.strictEqual(fsBinding.internalModuleStat(tmpRoot), 1);
assert.ok(fsBinding.internalModuleStat(path.join(tmpRoot, 'missing-file')) < 0);
const fsStatValues = fsBinding.stat(jsPath, false);
assert.ok(fsStatValues instanceof Float64Array);
assert.strictEqual(fsStatValues.length, 18);
const fsStatBigValues = fsBinding.stat(jsPath, true);
assert.ok(fsStatBigValues instanceof BigInt64Array);
assert.strictEqual(fsStatBigValues.length, 18);
const fsStatFsValues = fsBinding.statfs(jsPath, false);
assert.ok(fsStatFsValues instanceof Float64Array);
assert.strictEqual(fsStatFsValues.length, 7);
fsBinding.access(jsPath, fsConstants.F_OK);
const ioPath = path.join(tmpRoot, 'io.txt');
const ioWriteFd = fsBinding.open(
  ioPath,
  fsConstants.O_WRONLY | fsConstants.O_CREAT | fsConstants.O_TRUNC,
  0o666,
);
assert.strictEqual(fsBinding.writeString(ioWriteFd, 'hello', null, 'utf8'), 5);
assert.strictEqual(
  fsBinding.writeBuffers(ioWriteFd, [Buffer.from(' '), Buffer.from('world')], -1),
  6,
);
fsBinding.close(ioWriteFd);
const ioReadFd = fsBinding.open(ioPath, fsConstants.O_RDONLY, 0o666);
const ioReadBuffer = Buffer.alloc(11);
assert.strictEqual(fsBinding.read(ioReadFd, ioReadBuffer, 0, 11, 0), 11);
assert.strictEqual(ioReadBuffer.toString('utf8'), 'hello world');
const ioReadParts = [Buffer.alloc(5), Buffer.alloc(6)];
assert.strictEqual(fsBinding.readBuffers(ioReadFd, ioReadParts, 0), 11);
assert.strictEqual(Buffer.concat(ioReadParts).toString('utf8'), 'hello world');
fsBinding.close(ioReadFd);
const syncFileHandle = fsBinding.openFileHandle(ioPath, fsConstants.O_RDONLY, 0o666);
assert.ok(syncFileHandle instanceof fsBinding.FileHandle);
assert.strictEqual(typeof syncFileHandle.fd, 'number');
assert.strictEqual(typeof syncFileHandle.getAsyncId, 'function');
assert.strictEqual(typeof syncFileHandle.isStreamBase, 'boolean');
assert.strictEqual(typeof syncFileHandle.bytesRead, 'number');
assert.strictEqual(typeof syncFileHandle.bytesWritten, 'number');
assert.strictEqual(typeof syncFileHandle._externalStream, 'object');
syncFileHandle.close();
const statWatcher = new fsBinding.StatWatcher(false);
assert.strictEqual(typeof statWatcher.start, 'function');
assert.strictEqual(typeof statWatcher.close, 'function');
assert.strictEqual(typeof statWatcher.ref, 'function');
assert.strictEqual(typeof statWatcher.unref, 'function');
assert.strictEqual(typeof statWatcher.getAsyncId, 'function');
assert.strictEqual(typeof statWatcher.getAsyncId(), 'number');
statWatcher.close();
const fsEventBinding = internalBinding('fs_event_wrap');
assert.ok(fsEventBinding && typeof fsEventBinding === 'object');
assert.strictEqual(typeof fsEventBinding.FSEvent, 'function');
const fsEventHandle = new fsEventBinding.FSEvent();
assert.strictEqual(fsEventHandle.initialized, false);
fsEventHandle.owner = { marker: 1 };
assert.strictEqual(fsEventHandle.owner.marker, 1);
assert.strictEqual(typeof fsEventHandle.start, 'function');
assert.strictEqual(typeof fsEventHandle.close, 'function');
assert.strictEqual(typeof fsEventHandle.ref, 'function');
assert.strictEqual(typeof fsEventHandle.unref, 'function');
assert.strictEqual(typeof fsEventHandle.getAsyncId(), 'number');
fsEventHandle.close();
const pkgMainDir = path.join(tmpRoot, 'pkg-main');
fs.mkdirSync(pkgMainDir);
fs.writeFileSync(path.join(pkgMainDir, 'entry.js'), 'module.exports = 1;');
fs.writeFileSync(path.join(pkgMainDir, 'index.js'), 'module.exports = 2;');
assert.strictEqual(
  fsBinding.legacyMainResolve(pkgMainDir, 'entry', 'file:///tmp/base.mjs'),
  1,
);
assert.strictEqual(
  fsBinding.legacyMainResolve(pkgMainDir, undefined, 'file:///tmp/base.mjs'),
  7,
);
const cpSrc = path.join(tmpRoot, 'cp-src');
const cpDst = path.join(tmpRoot, 'cp-dst');
fs.mkdirSync(cpSrc);
fs.writeFileSync(path.join(cpSrc, 'a.txt'), 'A');
fsBinding.cpSyncCheckPaths(cpSrc, cpDst, false, true);
fsBinding.cpSyncCopyDir(cpSrc, cpDst, true, false, false, false, false);
assert.strictEqual(fs.readFileSync(path.join(cpDst, 'a.txt'), 'utf8'), 'A');
fs.writeFileSync(path.join(cpDst, 'a.txt'), 'OLD');
fsBinding.cpSyncOverrideFile(path.join(cpSrc, 'a.txt'), path.join(cpDst, 'a.txt'), 0, false);
assert.strictEqual(fs.readFileSync(path.join(cpDst, 'a.txt'), 'utf8'), 'A');
fs.rmSync(tmpRoot, { recursive: true, force: true });

const processMethods = internalBinding('process_methods');
assert.strictEqual(typeof processMethods.patchProcessObject, 'function');
assert.strictEqual(typeof processMethods.loadEnvFile, 'function');
assert.strictEqual(typeof processMethods.resetStdioForTesting, 'function');
const patchedProcess = {};
processMethods.patchProcessObject(patchedProcess);
assert.ok(Array.isArray(patchedProcess.argv));
assert.ok(Array.isArray(patchedProcess.execArgv));
assert.strictEqual(typeof patchedProcess.pid, 'number');
assert.strictEqual(typeof patchedProcess.execPath, 'string');

const envRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'edge-env-'));
const envPath = path.join(envRoot, '.env');
const envVar = 'EDGE_INTERNAL_BINDING_PARITY_ENV';
delete process.env[envVar];
fs.writeFileSync(envPath, `${envVar}=fresh\n`);
processMethods.loadEnvFile(envPath);
assert.strictEqual(process.env[envVar], 'fresh');
fs.writeFileSync(envPath, `${envVar}=override\n`);
processMethods.loadEnvFile(envPath);
assert.strictEqual(process.env[envVar], 'fresh');
assert.throws(
  () => processMethods.loadEnvFile(path.join(envRoot, 'missing.env')),
  (err) => err && err.code === 'ENOENT',
);
fs.rmSync(envRoot, { recursive: true, force: true });
delete process.env[envVar];

const credentialsBinding = internalBinding('credentials');
assert.ok(credentialsBinding && typeof credentialsBinding === 'object');
assert.strictEqual(typeof credentialsBinding.safeGetenv, 'function');
assert.strictEqual(typeof credentialsBinding.getTempDir, 'function');
assert.strictEqual(typeof credentialsBinding.getuid, 'function');
assert.strictEqual(typeof credentialsBinding.geteuid, 'function');
assert.strictEqual(typeof credentialsBinding.getgid, 'function');
assert.strictEqual(typeof credentialsBinding.getegid, 'function');
assert.strictEqual(typeof credentialsBinding.getgroups, 'function');
assert.strictEqual(typeof credentialsBinding.setuid, 'function');
assert.strictEqual(typeof credentialsBinding.seteuid, 'function');
assert.strictEqual(typeof credentialsBinding.setgid, 'function');
assert.strictEqual(typeof credentialsBinding.setegid, 'function');
assert.strictEqual(typeof credentialsBinding.setgroups, 'function');
assert.strictEqual(typeof credentialsBinding.initgroups, 'function');
const credEnvVar = 'EDGE_INTERNAL_BINDING_CREDENTIALS_SAFE_GETENV';
process.env[credEnvVar] = 'present';
assert.strictEqual(credentialsBinding.safeGetenv(credEnvVar), 'present');
assert.strictEqual(credentialsBinding.safeGetenv('EDGE_INTERNAL_BINDING_MISSING_ENV'), undefined);
assert.strictEqual(typeof credentialsBinding.getTempDir(), 'string');
assert.ok(Array.isArray(credentialsBinding.getgroups()));
delete process.env[credEnvVar];

assert.throws(
  () => processMethods._debugProcess(0x7fffffff),
  (err) => err && typeof err.code === 'string',
);

const builtinsBinding = internalBinding('builtins');
assert.strictEqual(typeof builtinsBinding.compileFunction, 'function');
assert.strictEqual(typeof builtinsBinding.setInternalLoaders, 'function');
assert.ok(builtinsBinding.builtinCategories && typeof builtinsBinding.builtinCategories === 'object');
assert.ok(Array.isArray(builtinsBinding.builtinCategories.canBeRequired));
assert.ok(Array.isArray(builtinsBinding.builtinCategories.cannotBeRequired));
assert.ok(builtinsBinding.natives && typeof builtinsBinding.natives === 'object');
assert.ok(Object.prototype.hasOwnProperty.call(builtinsBinding, 'configs'));
assert.strictEqual(typeof builtinsBinding.getCacheUsage, 'function');
const builtinsCacheUsage = builtinsBinding.getCacheUsage();
assert.ok(builtinsCacheUsage.compiledWithCache instanceof Set);
assert.ok(builtinsCacheUsage.compiledWithoutCache instanceof Set);
assert.ok(Array.isArray(builtinsCacheUsage.compiledInSnapshot));
const compiledBuiltin = builtinsBinding.compileFunction('internal/test/binding');
assert.strictEqual(typeof compiledBuiltin, 'function');
builtinsBinding.setInternalLoaders(
  (name) => internalBinding(name),
  (name) => require(name),
);

const stringDecoderBinding = internalBinding('string_decoder');
assert.ok(stringDecoderBinding && typeof stringDecoderBinding === 'object');
assert.strictEqual(typeof stringDecoderBinding.kNumFields, 'number');
assert.strictEqual(stringDecoderBinding.kNumFields, stringDecoderBinding.kSize);

const udpWrap = internalBinding('udp_wrap');
const udpHandle = new udpWrap.UDP();
const udpSendWrap = new udpWrap.SendWrap();
assert.strictEqual(typeof udpHandle.asyncReset, 'function');
assert.strictEqual(typeof udpHandle.getProviderType, 'function');
assert.strictEqual(typeof udpSendWrap.getAsyncId, 'function');
assert.strictEqual(typeof udpSendWrap.asyncReset, 'function');
assert.strictEqual(typeof udpSendWrap.getProviderType, 'function');
assert.strictEqual(udpSendWrap.getAsyncId(), -1);
const recvBufferContext = {};
const sendBufferContext = {};
const recvBufferSize = udpHandle.bufferSize(0, true, recvBufferContext);
const sendBufferSize = udpHandle.bufferSize(0, false, sendBufferContext);
assert.ok(recvBufferSize === undefined || typeof recvBufferSize === 'number');
assert.ok(sendBufferSize === undefined || typeof sendBufferSize === 'number');
const recvSetContext = {};
const sendSetContext = {};
const recvSetResult = udpHandle.bufferSize(
  typeof recvBufferSize === 'number' ? recvBufferSize : 0,
  true,
  recvSetContext,
);
const sendSetResult = udpHandle.bufferSize(
  typeof sendBufferSize === 'number' ? sendBufferSize : 0,
  false,
  sendSetContext,
);
assert.ok(recvSetResult === undefined || typeof recvSetResult === 'number');
assert.ok(sendSetResult === undefined || typeof sendSetResult === 'number');
assert.strictEqual(typeof udpHandle.setMulticastLoopback(true), 'number');
udpHandle.close(() => {});

const jsUdpWrap = internalBinding('js_udp_wrap');
assert.strictEqual(typeof jsUdpWrap.JSUDPWrap, 'function');
const jsUdpHandle = new jsUdpWrap.JSUDPWrap();
assert.strictEqual(typeof jsUdpHandle.recvStart, 'function');
assert.strictEqual(typeof jsUdpHandle.recvStop, 'function');
assert.strictEqual(typeof jsUdpHandle.emitReceived, 'function');
assert.strictEqual(typeof jsUdpHandle.onSendDone, 'function');
assert.strictEqual(typeof jsUdpHandle.onAfterBind, 'function');
assert.strictEqual(typeof jsUdpHandle.getAsyncId, 'function');
assert.strictEqual(typeof jsUdpHandle.asyncReset, 'function');
assert.strictEqual(typeof jsUdpHandle.getProviderType, 'function');

globalThis.__edge_internal_binding_parity_ok = 1;
)JS";

constexpr const char* kParityDestroyDrainScript = R"JS(
const assert = require('assert');

assert.deepStrictEqual(globalThis.__edge_destroy_calls, [321]);

globalThis.__edge_internal_binding_destroy_drain_ok = 1;
)JS";

constexpr const char* kSymbolBootstrapParityScript = R"JS(
const assert = require('assert');

const utilBinding = internalBinding('util');
const symbolsBinding = internalBinding('symbols');
const messagingBinding = internalBinding('messaging');

const domException = new messagingBinding.DOMException('boom');

assert.strictEqual(
  domException[utilBinding.privateSymbols.transfer_mode_private_symbol],
  utilBinding.constants.kCloneable,
);
assert.strictEqual(
  typeof messagingBinding.DOMException.prototype[symbolsBinding.messaging_clone_symbol],
  'function',
);
assert.strictEqual(
  typeof messagingBinding.DOMException.prototype[symbolsBinding.messaging_deserialize_symbol],
  'function',
);

globalThis.__edge_symbol_bootstrap_parity_ok = 1;
)JS";

constexpr const char* kBindingCleanupRecreateScript = R"JS(
const assert = require('assert');

for (const name of [
  'blob',
  'config',
  'constants',
  'fs',
  'fs_dir',
  'messaging',
  'mksnapshot',
  'module_wrap',
  'performance',
  'permission',
  'pipe_wrap',
  'stream_wrap',
  'symbols',
  'tcp_wrap',
  'tty_wrap',
  'util',
  'wasm_web_api',
]) {
  const binding = internalBinding(name);
  assert.ok(binding && (typeof binding === 'object' || typeof binding === 'function'), name);
}

require('fs');
require('node:buffer');
require('node:net');
require('node:perf_hooks');
require('node:tls');

globalThis.__edge_binding_cleanup_recreate_ok = 1;
)JS";

}  // namespace

TEST_F(Test5InternalBindingParityPhase03, WaveOneAndTwoBindingsHaveCriticalParitySurface) {
  EnvScope s(runtime_.get());

  std::string error;
  const int exit_code = EdgeRunScriptSource(s.env, kParityWaveScript, &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty());

  napi_value global = nullptr;
  ASSERT_EQ(napi_get_global(s.env, &global), napi_ok);

  napi_value ok_value = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, global, "__edge_internal_binding_parity_ok", &ok_value), napi_ok);

  int32_t ok = 0;
  ASSERT_EQ(napi_get_value_int32(s.env, ok_value, &ok), napi_ok);
  EXPECT_EQ(ok, 1);

  // The first run drains the event loop, so deferred destroy hooks queued by
  // queueDestroyAsyncId have fired by the time this second script asserts.
  const int drain_exit_code = EdgeRunScriptSource(s.env, kParityDestroyDrainScript, &error);
  EXPECT_EQ(drain_exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty());

  napi_value drain_ok_value = nullptr;
  ASSERT_EQ(
      napi_get_named_property(
          s.env, global, "__edge_internal_binding_destroy_drain_ok", &drain_ok_value),
      napi_ok);

  int32_t drain_ok = 0;
  ASSERT_EQ(napi_get_value_int32(s.env, drain_ok_value, &drain_ok), napi_ok);
  EXPECT_EQ(drain_ok, 1);
}

TEST_F(Test5InternalBindingParityPhase03, BindingCachesCanBeDestroyedAndRecreatedAcrossEnvs) {
  for (int i = 0; i < 2; ++i) {
    EnvScope s(runtime_.get());

    std::string error;
    const int exit_code = EdgeRunScriptSource(s.env, kBindingCleanupRecreateScript, &error);
    EXPECT_EQ(exit_code, 0) << "iteration=" << i << " error=" << error;
    EXPECT_TRUE(error.empty()) << "iteration=" << i << " error=" << error;

    napi_value global = nullptr;
    ASSERT_EQ(napi_get_global(s.env, &global), napi_ok);

    napi_value ok_value = nullptr;
    ASSERT_EQ(napi_get_named_property(s.env, global, "__edge_binding_cleanup_recreate_ok", &ok_value), napi_ok);

    int32_t ok = 0;
    ASSERT_EQ(napi_get_value_int32(s.env, ok_value, &ok), napi_ok);
    EXPECT_EQ(ok, 1) << "iteration=" << i;
  }
}

TEST_F(Test5InternalBindingParityPhase03, BootstrapSymbolsShareStateAcrossBindings) {
  EnvScope s(runtime_.get());

  std::string error;
  const int exit_code = EdgeRunScriptSource(s.env, kSymbolBootstrapParityScript, &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty());

  napi_value global = nullptr;
  ASSERT_EQ(napi_get_global(s.env, &global), napi_ok);

  napi_value ok_value = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, global, "__edge_symbol_bootstrap_parity_ok", &ok_value), napi_ok);

  int32_t ok = 0;
  ASSERT_EQ(napi_get_value_int32(s.env, ok_value, &ok), napi_ok);
  EXPECT_EQ(ok, 1);
}
