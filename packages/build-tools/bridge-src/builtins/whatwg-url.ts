import { builtinPathStdlibModule } from "./builtin-modules.js";
import { Blob } from "./network.js";
import { TextDecoder } from "../polyfills/index.js";
import { urlStdlibModuleNs } from "../prelude.js";

var inspectCustomSymbol = /* @__PURE__ */ Symbol.for("nodejs.util.inspect.custom");
var toStringTagSymbol = Symbol.toStringTag;
var ERR_INVALID_THIS = "ERR_INVALID_THIS";
var ERR_MISSING_ARGS = "ERR_MISSING_ARGS";
var ERR_INVALID_URL = "ERR_INVALID_URL";
var ERR_ARG_NOT_ITERABLE = "ERR_ARG_NOT_ITERABLE";
var ERR_INVALID_TUPLE = "ERR_INVALID_TUPLE";
var URL_SEARCH_PARAMS_TYPE = "URLSearchParams";
var kLinkedSearchParams = /* @__PURE__ */ Symbol("secureExecLinkedURLSearchParams");
var kBlobUrlStore = /* @__PURE__ */ Symbol.for("secureExec.blobUrlStore");
var kBlobUrlCounter = /* @__PURE__ */ Symbol.for("secureExec.blobUrlCounter");
var SEARCH_PARAM_METHOD_NAMES = ["append", "delete", "get", "getAll", "has"];
var SEARCH_PARAM_PAIR_METHOD_NAMES = ["append", "set"];
var URL_SCHEME_TYPES = {
  "http:": 0,
  "https:": 2,
  "ws:": 4,
  "wss:": 5,
  "file:": 6,
  "ftp:": 8
};
var searchParamsBrand = /* @__PURE__ */ new WeakSet();
var searchParamsState = /* @__PURE__ */ new WeakMap();
var searchParamsIteratorBrand = /* @__PURE__ */ new WeakSet();
var searchParamsIteratorState = /* @__PURE__ */ new WeakMap();
function createNodeTypeError(message, code) {
  const error = new TypeError(message);
  error.code = code;
  return error;
}
function createInvalidUrlError() {
  const error = new TypeError("Invalid URL");
  error.code = ERR_INVALID_URL;
  return error;
}
function createUrlReceiverTypeError() {
  return new TypeError("Receiver must be an instance of class URL");
}
function createMissingArgsError(message) {
  return createNodeTypeError(message, ERR_MISSING_ARGS);
}
function createIterableTypeError() {
  return createNodeTypeError("Query pairs must be iterable", ERR_ARG_NOT_ITERABLE);
}
function createTupleTypeError() {
  return createNodeTypeError(
    "Each query pair must be an iterable [name, value] tuple",
    ERR_INVALID_TUPLE
  );
}
function createSymbolStringError() {
  return new TypeError("Cannot convert a Symbol value to a string");
}
function toNodeString(value) {
  if (typeof value === "symbol") {
    throw createSymbolStringError();
  }
  return String(value);
}
function toWellFormedString(value) {
  let result = "";
  for (let index = 0; index < value.length; index += 1) {
    const codeUnit = value.charCodeAt(index);
    if (codeUnit >= 55296 && codeUnit <= 56319) {
      const nextIndex = index + 1;
      if (nextIndex < value.length) {
        const nextCodeUnit = value.charCodeAt(nextIndex);
        if (nextCodeUnit >= 56320 && nextCodeUnit <= 57343) {
          result += value[index] + value[nextIndex];
          index = nextIndex;
          continue;
        }
      }
      result += "\uFFFD";
      continue;
    }
    if (codeUnit >= 56320 && codeUnit <= 57343) {
      result += "\uFFFD";
      continue;
    }
    result += value[index];
  }
  return result;
}
function toNodeUSVString(value) {
  return toWellFormedString(toNodeString(value));
}
function assertUrlSearchParamsReceiver(receiver) {
  if (!searchParamsBrand.has(receiver)) {
    throw createNodeTypeError(
      'Value of "this" must be of type URLSearchParams',
      ERR_INVALID_THIS
    );
  }
}
function assertUrlSearchParamsIteratorReceiver(receiver) {
  if (!searchParamsIteratorBrand.has(receiver)) {
    throw createNodeTypeError(
      'Value of "this" must be of type URLSearchParamsIterator',
      ERR_INVALID_THIS
    );
  }
}
function getUrlSearchParamsImpl(receiver) {
  const state = searchParamsState.get(receiver);
  if (!state) {
    throw createNodeTypeError(
      'Value of "this" must be of type URLSearchParams',
      ERR_INVALID_THIS
    );
  }
  return state.getImpl();
}
function countSearchParams(params) {
  let count = 0;
  for (const _entry of params) {
    count++;
  }
  return count;
}
function isAsciiHexCodeUnit(codeUnit) {
  return codeUnit >= 48 && codeUnit <= 57 || codeUnit >= 65 && codeUnit <= 70 || codeUnit >= 97 && codeUnit <= 102;
}
function decodeAsciiHexCodeUnit(codeUnit) {
  if (codeUnit >= 48 && codeUnit <= 57) {
    return codeUnit - 48;
  }
  if (codeUnit >= 65 && codeUnit <= 70) {
    return codeUnit - 55;
  }
  return codeUnit - 87;
}
function appendUtf8CodePoint(bytes, codePoint) {
  if (codePoint <= 127) {
    bytes.push(codePoint);
    return;
  }
  if (codePoint <= 2047) {
    bytes.push(
      192 | codePoint >> 6,
      128 | codePoint & 63
    );
    return;
  }
  if (codePoint <= 65535) {
    bytes.push(
      224 | codePoint >> 12,
      128 | codePoint >> 6 & 63,
      128 | codePoint & 63
    );
    return;
  }
  bytes.push(
    240 | codePoint >> 18,
    128 | codePoint >> 12 & 63,
    128 | codePoint >> 6 & 63,
    128 | codePoint & 63
  );
}
function decodeFormUrlencodedComponent(value) {
  const source = String(value).replace(/\+/g, " ");
  let output = "";
  for (let index = 0; index < source.length; index += 1) {
    if (source.charCodeAt(index) === 37 && index + 2 < source.length) {
      const bytes = [];
      let nextIndex = index;
      while (nextIndex + 2 < source.length && source.charCodeAt(nextIndex) === 37) {
        const high = source.charCodeAt(nextIndex + 1);
        const low = source.charCodeAt(nextIndex + 2);
        if (!isAsciiHexCodeUnit(high) || !isAsciiHexCodeUnit(low)) {
          break;
        }
        bytes.push((decodeAsciiHexCodeUnit(high) << 4) + decodeAsciiHexCodeUnit(low));
        nextIndex += 3;
      }
      if (bytes.length > 0) {
        output += new TextDecoder().decode(Uint8Array.from(bytes));
        index = nextIndex - 1;
        continue;
      }
    }
    const codePoint = source.codePointAt(index);
    output += String.fromCodePoint(codePoint);
    if (codePoint > 65535) {
      index += 1;
    }
  }
  return output;
}
function serializeFormUrlencodedComponent(value) {
  const input = String(value);
  const bytes = [];
  for (let index = 0; index < input.length; index += 1) {
    const codePoint = input.codePointAt(index);
    appendUtf8CodePoint(bytes, codePoint);
    if (codePoint > 65535) {
      index += 1;
    }
  }
  let output = "";
  for (const byte of bytes) {
    if (byte === 32) {
      output += "+";
      continue;
    }
    const isAlphaNumeric = byte >= 48 && byte <= 57 || byte >= 65 && byte <= 90 || byte >= 97 && byte <= 122;
    if (isAlphaNumeric || byte === 42 || byte === 45 || byte === 46 || byte === 95) {
      output += String.fromCharCode(byte);
      continue;
    }
    output += `%${byte.toString(16).toUpperCase().padStart(2, "0")}`;
  }
  return output;
}
function compareCodeUnitStrings(left, right) {
  const minLength = Math.min(left.length, right.length);
  for (let index = 0; index < minLength; index += 1) {
    const diff = left.charCodeAt(index) - right.charCodeAt(index);
    if (diff !== 0) {
      return diff;
    }
  }
  return left.length - right.length;
}
function normalizeSearchParamsInit(init) {
  if (init && typeof init === "object" && kLinkedSearchParams in init) {
    return init;
  }
  if (init == null) {
    return void 0;
  }
  if (typeof init === "string") {
    return toNodeUSVString(init);
  }
  if (typeof init === "object" || typeof init === "function") {
    const iterator2 = init[Symbol.iterator];
    if (iterator2 !== void 0) {
      if (typeof iterator2 !== "function") {
        throw createIterableTypeError();
      }
      const pairs2 = [];
      for (const pair of init) {
        if (pair == null) {
          throw createTupleTypeError();
        }
        const pairIterator = pair[Symbol.iterator];
        if (typeof pairIterator !== "function") {
          throw createTupleTypeError();
        }
        const values = Array.from(pair);
        if (values.length !== 2) {
          throw createTupleTypeError();
        }
        pairs2.push([toNodeUSVString(values[0]), toNodeUSVString(values[1])]);
      }
      return pairs2;
    }
    const pairs = [];
    for (const key of Reflect.ownKeys(init)) {
      if (!Object.prototype.propertyIsEnumerable.call(init, key)) {
        continue;
      }
      pairs.push([
        toNodeUSVString(key),
        toNodeUSVString(init[key])
      ]);
    }
    return pairs;
  }
  return toNodeUSVString(init);
}
var NativeURLSearchParams = typeof globalThis.URLSearchParams === "function" && globalThis.URLSearchParams.__secureExecBootstrapStub !== true ? globalThis.URLSearchParams : class URLSearchParams {
  constructor(init = "") {
    this._pairs = [];
    if (typeof init === "string") {
      const query = init.replace(/^\?/, "");
      if (!query) {
        return;
      }
      for (const entry of query.split("&")) {
        if (!entry) {
          continue;
        }
        const [key, ...rest] = entry.split("=");
        this._pairs.push([
          decodeFormUrlencodedComponent(key),
          decodeFormUrlencodedComponent(rest.join("="))
        ]);
      }
      return;
    }
    if (Array.isArray(init)) {
      for (const pair of init) {
        if (pair == null || pair.length !== 2) {
          continue;
        }
        this._pairs.push([String(pair[0]), String(pair[1])]);
      }
      return;
    }
    if (init && typeof init === "object") {
      for (const [key, value] of Object.entries(init)) {
        this._pairs.push([String(key), String(value)]);
      }
    }
  }
  append(name, value) {
    this._pairs.push([String(name), String(value)]);
  }
  delete(name) {
    const key = String(name);
    this._pairs = this._pairs.filter(([candidate]) => candidate !== key);
  }
  get(name) {
    const key = String(name);
    const match = this._pairs.find(([candidate]) => candidate === key);
    return match ? match[1] : null;
  }
  getAll(name) {
    const key = String(name);
    return this._pairs.filter(([candidate]) => candidate === key).map(([, value]) => value);
  }
  has(name) {
    const key = String(name);
    return this._pairs.some(([candidate]) => candidate === key);
  }
  set(name, value) {
    const key = String(name);
    const stringValue = String(value);
    const nextPairs = [];
    let replaced = false;
    for (const [candidate, currentValue] of this._pairs) {
      if (candidate !== key) {
        nextPairs.push([candidate, currentValue]);
        continue;
      }
      if (!replaced) {
        replaced = true;
        nextPairs.push([key, stringValue]);
      }
    }
    if (!replaced) {
      nextPairs.push([key, stringValue]);
    }
    this._pairs = nextPairs;
  }
  sort() {
    this._pairs = this._pairs.map((pair, index) => ({ pair, index })).sort((left, right) => {
      const diff = compareCodeUnitStrings(left.pair[0], right.pair[0]);
      return diff !== 0 ? diff : left.index - right.index;
    }).map(({ pair }) => pair);
  }
  entries() {
    return this._pairs[Symbol.iterator]();
  }
  keys() {
    return this._pairs.map(([key]) => key)[Symbol.iterator]();
  }
  values() {
    return this._pairs.map(([, value]) => value)[Symbol.iterator]();
  }
  [Symbol.iterator]() {
    return this.entries();
  }
  toString() {
    return this._pairs.map(([key, value]) => `${serializeFormUrlencodedComponent(key)}=${serializeFormUrlencodedComponent(value)}`).join("&");
  }
};
function createStandaloneSearchParams(init) {
  if (typeof init === "string") {
    return new NativeURLSearchParams(init);
  }
  return init === void 0 ? new NativeURLSearchParams() : new NativeURLSearchParams(init);
}
function createCollectionBody(items, options, emptyBody) {
  if (items.length === 0) {
    return emptyBody;
  }
  const oneLine = `{ ${items.join(", ")} }`;
  const breakLength = options?.breakLength ?? Infinity;
  if (oneLine.length <= breakLength) {
    return oneLine;
  }
  return `{
${items.join(",\n  ")} }`;
}
function createUrlContext(url) {
  const href = url.href;
  const protocolEnd = href.indexOf(":") + 1;
  const authIndex = href.indexOf("@");
  const pathnameStart = href.indexOf("/", protocolEnd + 2);
  const searchStart = href.indexOf("?");
  const hashStart = href.indexOf("#");
  const usernameEnd = url.username.length > 0 ? href.indexOf(":", protocolEnd + 2) : protocolEnd + 2;
  const hostStart = authIndex === -1 ? protocolEnd + 2 : authIndex;
  const hostEnd = pathnameStart === -1 ? href.length : pathnameStart - (url.port.length > 0 ? url.port.length + 1 : 0);
  const port = url.port.length > 0 ? Number(url.port) : null;
  return {
    href,
    protocol_end: protocolEnd,
    username_end: usernameEnd,
    host_start: hostStart,
    host_end: hostEnd,
    pathname_start: pathnameStart === -1 ? href.length : pathnameStart,
    search_start: searchStart === -1 ? href.length : searchStart,
    hash_start: hashStart === -1 ? href.length : hashStart,
    port,
    scheme_type: URL_SCHEME_TYPES[url.protocol] ?? 1,
    hasPort: url.port.length > 0,
    hasSearch: url.search.length > 0,
    hasHash: url.hash.length > 0
  };
}
function formatUrlContext(url, inspect, options) {
  const context = createUrlContext(url);
  const formatValue = typeof inspect === "function" ? (value) => inspect(value, options) : (value) => JSON.stringify(value);
  const portValue = context.port === null ? "null" : String(context.port);
  return [
    "URLContext {",
    `    href: ${formatValue(context.href)},`,
    `    protocol_end: ${context.protocol_end},`,
    `    username_end: ${context.username_end},`,
    `    host_start: ${context.host_start},`,
    `    host_end: ${context.host_end},`,
    `    pathname_start: ${context.pathname_start},`,
    `    search_start: ${context.search_start},`,
    `    hash_start: ${context.hash_start},`,
    `    port: ${portValue},`,
    `    scheme_type: ${context.scheme_type},`,
    "    [hasPort]: [Getter],",
    "    [hasSearch]: [Getter],",
    "    [hasHash]: [Getter]",
    "  }"
  ].join("\n");
}
function getBlobUrlStore() {
  const globalRecord = globalThis;
  const existing = globalRecord[kBlobUrlStore];
  if (existing instanceof Map) {
    return existing;
  }
  const store = /* @__PURE__ */ new Map();
  globalRecord[kBlobUrlStore] = store;
  return store;
}
function nextBlobUrlId() {
  const globalRecord = globalThis;
  const nextId = typeof globalRecord[kBlobUrlCounter] === "number" ? globalRecord[kBlobUrlCounter] : 1;
  globalRecord[kBlobUrlCounter] = nextId + 1;
  return nextId;
}
var URLSearchParamsIterator = class _URLSearchParamsIterator {
  constructor(values) {
    searchParamsIteratorBrand.add(this);
    searchParamsIteratorState.set(this, { values, index: 0 });
  }
  next() {
    assertUrlSearchParamsIteratorReceiver(this);
    const state = searchParamsIteratorState.get(this);
    if (state.index >= state.values.length) {
      return { value: void 0, done: true };
    }
    const value = state.values[state.index];
    state.index += 1;
    return { value, done: false };
  }
  [inspectCustomSymbol](depth, options, inspect) {
    assertUrlSearchParamsIteratorReceiver(this);
    if (depth < 0) {
      return "[Object]";
    }
    const state = searchParamsIteratorState.get(this);
    const formatValue = typeof inspect === "function" ? (value) => inspect(value, options) : (value) => JSON.stringify(value);
    const remaining = state.values.slice(state.index).map((value) => formatValue(value));
    return `URLSearchParams Iterator ${createCollectionBody(remaining, options, "{  }")}`;
  }
  get [toStringTagSymbol]() {
    if (this !== _URLSearchParamsIterator.prototype) {
      assertUrlSearchParamsIteratorReceiver(this);
    }
    return "URLSearchParams Iterator";
  }
};
Object.defineProperties(URLSearchParamsIterator.prototype, {
  next: {
    value: URLSearchParamsIterator.prototype.next,
    writable: true,
    configurable: true,
    enumerable: true
  },
  [Symbol.iterator]: {
    value: function iterator() {
      assertUrlSearchParamsIteratorReceiver(this);
      return this;
    },
    writable: true,
    configurable: true,
    enumerable: false
  },
  [inspectCustomSymbol]: {
    value: URLSearchParamsIterator.prototype[inspectCustomSymbol],
    writable: true,
    configurable: true,
    enumerable: false
  },
  [toStringTagSymbol]: {
    get: Object.getOwnPropertyDescriptor(URLSearchParamsIterator.prototype, toStringTagSymbol)?.get,
    configurable: true,
    enumerable: false
  }
});
Object.defineProperty(
  Object.getOwnPropertyDescriptor(URLSearchParamsIterator.prototype, Symbol.iterator)?.value,
  "name",
  {
    value: "entries",
    configurable: true
  }
);
var URLSearchParams = class _URLSearchParams {
  constructor(init) {
    searchParamsBrand.add(this);
    const normalized = normalizeSearchParamsInit(init);
    if (normalized && typeof normalized === "object" && kLinkedSearchParams in normalized) {
      searchParamsState.set(this, {
        getImpl: normalized[kLinkedSearchParams]
      });
      return;
    }
    const impl = createStandaloneSearchParams(
      normalized
    );
    searchParamsState.set(this, { getImpl: () => impl });
  }
  append(name, value) {
    assertUrlSearchParamsReceiver(this);
    if (arguments.length < 2) {
      throw createMissingArgsError('The "name" and "value" arguments must be specified');
    }
    getUrlSearchParamsImpl(this).append(toNodeUSVString(name), toNodeUSVString(value));
  }
  delete(name) {
    assertUrlSearchParamsReceiver(this);
    if (arguments.length < 1) {
      throw createMissingArgsError('The "name" argument must be specified');
    }
    getUrlSearchParamsImpl(this).delete(toNodeUSVString(name));
  }
  get(name) {
    assertUrlSearchParamsReceiver(this);
    if (arguments.length < 1) {
      throw createMissingArgsError('The "name" argument must be specified');
    }
    return getUrlSearchParamsImpl(this).get(toNodeUSVString(name));
  }
  getAll(name) {
    assertUrlSearchParamsReceiver(this);
    if (arguments.length < 1) {
      throw createMissingArgsError('The "name" argument must be specified');
    }
    return getUrlSearchParamsImpl(this).getAll(toNodeUSVString(name));
  }
  has(name) {
    assertUrlSearchParamsReceiver(this);
    if (arguments.length < 1) {
      throw createMissingArgsError('The "name" argument must be specified');
    }
    return getUrlSearchParamsImpl(this).has(toNodeUSVString(name));
  }
  set(name, value) {
    assertUrlSearchParamsReceiver(this);
    if (arguments.length < 2) {
      throw createMissingArgsError('The "name" and "value" arguments must be specified');
    }
    getUrlSearchParamsImpl(this).set(toNodeUSVString(name), toNodeUSVString(value));
  }
  sort() {
    assertUrlSearchParamsReceiver(this);
    getUrlSearchParamsImpl(this).sort();
  }
  entries() {
    assertUrlSearchParamsReceiver(this);
    return new URLSearchParamsIterator(Array.from(getUrlSearchParamsImpl(this)));
  }
  keys() {
    assertUrlSearchParamsReceiver(this);
    return new URLSearchParamsIterator(Array.from(getUrlSearchParamsImpl(this).keys()));
  }
  values() {
    assertUrlSearchParamsReceiver(this);
    return new URLSearchParamsIterator(Array.from(getUrlSearchParamsImpl(this).values()));
  }
  forEach(callback, thisArg) {
    assertUrlSearchParamsReceiver(this);
    if (typeof callback !== "function") {
      throw createNodeTypeError(
        'The "callback" argument must be of type function. Received ' + (callback === void 0 ? "undefined" : typeof callback),
        "ERR_INVALID_ARG_TYPE"
      );
    }
    for (const [key, value] of getUrlSearchParamsImpl(this)) {
      callback.call(thisArg, value, key, this);
    }
  }
  toString() {
    assertUrlSearchParamsReceiver(this);
    return getUrlSearchParamsImpl(this).toString();
  }
  get size() {
    assertUrlSearchParamsReceiver(this);
    return countSearchParams(getUrlSearchParamsImpl(this));
  }
  [inspectCustomSymbol](depth, options, inspect) {
    assertUrlSearchParamsReceiver(this);
    if (depth < 0) {
      return "[Object]";
    }
    const formatValue = typeof inspect === "function" ? (value) => inspect(value, options) : (value) => JSON.stringify(value);
    const items = Array.from(
      getUrlSearchParamsImpl(this)
    ).map(
      ([key, value]) => `${formatValue(key)} => ${formatValue(value)}`
    );
    return `URLSearchParams ${createCollectionBody(items, options, "{}")}`;
  }
  get [toStringTagSymbol]() {
    if (this !== _URLSearchParams.prototype) {
      assertUrlSearchParamsReceiver(this);
    }
    return URL_SEARCH_PARAMS_TYPE;
  }
};
for (const name of SEARCH_PARAM_METHOD_NAMES) {
  Object.defineProperty(URLSearchParams.prototype, name, {
    value: URLSearchParams.prototype[name],
    writable: true,
    configurable: true,
    enumerable: true
  });
}
for (const name of SEARCH_PARAM_PAIR_METHOD_NAMES) {
  Object.defineProperty(URLSearchParams.prototype, name, {
    value: URLSearchParams.prototype[name],
    writable: true,
    configurable: true,
    enumerable: true
  });
}
for (const name of ["sort", "entries", "forEach", "keys", "values", "toString"]) {
  Object.defineProperty(URLSearchParams.prototype, name, {
    value: URLSearchParams.prototype[name],
    writable: true,
    configurable: true,
    enumerable: true
  });
}
Object.defineProperties(URLSearchParams.prototype, {
  size: {
    get: Object.getOwnPropertyDescriptor(URLSearchParams.prototype, "size")?.get,
    configurable: true,
    enumerable: true
  },
  [Symbol.iterator]: {
    value: URLSearchParams.prototype.entries,
    writable: true,
    configurable: true,
    enumerable: false
  },
  [inspectCustomSymbol]: {
    value: URLSearchParams.prototype[inspectCustomSymbol],
    writable: true,
    configurable: true,
    enumerable: false
  },
  [toStringTagSymbol]: {
    get: Object.getOwnPropertyDescriptor(URLSearchParams.prototype, toStringTagSymbol)?.get,
    configurable: true,
    enumerable: false
  }
});
function canUseNativeUrlImplementation(candidate) {
  if (typeof candidate !== "function" || candidate.__secureExecBootstrapStub === true) {
    return false;
  }
  try {
    return String(new candidate("./child.mjs", "file:///root/base/entry.mjs").href) === "file:///root/base/child.mjs";
  } catch {
    return false;
  }
}
function ensureTrailingSlashForFilePath(pathname) {
  return pathname.endsWith("/") ? pathname : `${pathname}/`;
}
function normalizeRelativeFileUrlInput(input, base) {
  const rawInput = String(input ?? "");
  if (!rawInput.startsWith("file:")) {
    return { input: rawInput, base };
  }
  const relativeMatch = /^file:(\.\.?(?:\/[^?#]*)?)([?#].*)?$/.exec(rawInput);
  if (!relativeMatch) {
    return { input: rawInput, base };
  }
  const relativePath = relativeMatch[1];
  const suffix = relativeMatch[2] ?? "";
  let baseHref = typeof base === "undefined" ? "file:///" : String(base);
  try {
    const parsedBase = new globalThis.URL(baseHref);
    if (parsedBase.protocol !== "file:") {
      return { input: rawInput, base };
    }
    let basePathname = parsedBase.pathname || "/";
    if (!basePathname.startsWith("/")) {
      basePathname = `/${basePathname}`;
    }
    const baseDirectory = basePathname.endsWith("/") ? basePathname : builtinPathStdlibModule.posix.dirname(basePathname);
    const resolvedPath = builtinPathStdlibModule.posix.resolve(baseDirectory, relativePath);
    const needsTrailingSlash = relativePath === "." || relativePath === ".." || relativePath.endsWith("/");
    const normalizedPath = needsTrailingSlash ? ensureTrailingSlashForFilePath(resolvedPath) : resolvedPath;
    return {
      input: `file://${normalizedPath}${suffix}`,
      base: void 0
    };
  } catch {
    return { input: rawInput, base };
  }
}
var nativeUrlCandidate = typeof urlStdlibModuleNs?.URL === "function" ? urlStdlibModuleNs.URL : typeof urlStdlibModuleNs?.default?.URL === "function" ? urlStdlibModuleNs.default.URL : globalThis.URL;
var NativeURL = canUseNativeUrlImplementation(nativeUrlCandidate) ? nativeUrlCandidate : class URL {
  constructor(url, base) {
    const raw = String(url ?? "");
    const hasScheme = /^[a-zA-Z][a-zA-Z\d+\-.]*:/.test(raw);
    if (!hasScheme && typeof base === "undefined") {
      throw new TypeError(`Invalid URL: ${raw}`);
    }
    let full = raw;
    if (!hasScheme) {
      const baseUrl = new URL(base);
      if (baseUrl.protocol === "file:") {
        const queryIndex2 = raw.indexOf("?");
        const hashIndex2 = raw.indexOf("#");
        const searchStart2 = queryIndex2 === -1 ? raw.length : queryIndex2;
        const hashStart2 = hashIndex2 === -1 ? raw.length : hashIndex2;
        const pathEnd2 = Math.min(searchStart2, hashStart2);
        const relativePath = raw.slice(0, pathEnd2);
        const suffix = raw.slice(pathEnd2);
        let basePathname = baseUrl.pathname || "/";
        if (!basePathname.startsWith("/")) {
          basePathname = `/${basePathname}`;
        }
        const baseDirectory = basePathname.endsWith("/") ? basePathname : builtinPathStdlibModule.posix.dirname(basePathname);
        let resolvedPath = builtinPathStdlibModule.posix.resolve(baseDirectory, relativePath);
        if ((relativePath.endsWith("/") || /(^|\/)\.\.?$/.test(relativePath)) && !resolvedPath.endsWith("/")) {
          resolvedPath += "/";
        }
        full = `file://${resolvedPath}${suffix}`;
      } else {
        const baseHref = String(baseUrl.href);
        full = baseHref.replace(/\/[^/]*$/, "/") + raw;
      }
    }
    const queryIndex = full.indexOf("?");
    const hashIndex = full.indexOf("#");
    const searchStart = queryIndex === -1 ? full.length : queryIndex;
    const hashStart = hashIndex === -1 ? full.length : hashIndex;
    const pathEnd = Math.min(searchStart, hashStart);
    const searchValue = queryIndex === -1 ? "" : full.slice(queryIndex, hashStart);
    const hashValue = hashIndex === -1 ? "" : full.slice(hashIndex);
    if (full.startsWith("file:")) {
      let pathname = full.slice(5, pathEnd);
      if (pathname.startsWith("//")) {
        const authorityMatch = /^\/\/[^/]*(.*)$/.exec(pathname);
        pathname = authorityMatch?.[1] || "/";
      }
      if (!pathname.startsWith("/")) {
        pathname = `/${pathname}`;
      }
      this.protocol = "file:";
      this.hostname = "";
      this.port = "";
      this.pathname = pathname || "/";
      this.search = searchValue;
      this.hash = hashValue;
      this.host = "";
      this.href = `file://${this.pathname}${this.search}${this.hash}`;
      this.origin = "null";
      this.searchParams = new URLSearchParams(this.search);
      const syncHrefFromSearchParams = () => {
        const query = this.searchParams.toString();
        this.search = query ? `?${query}` : "";
        this.href = `file://${this.pathname}${this.search}${this.hash}`;
      };
      for (const method of ["append", "delete", "set", "sort"]) {
        const original = this.searchParams[method]?.bind(this.searchParams);
        if (!original) {
          continue;
        }
        this.searchParams[method] = (...args) => {
          const result = original(...args);
          syncHrefFromSearchParams();
          return result;
        };
      }
      return;
    }
    const opaqueSchemeMatch = full.match(/^([a-zA-Z][a-zA-Z\d+\-.]*:)(?!\/\/)/);
    if (opaqueSchemeMatch) {
      const protocol = opaqueSchemeMatch[1];
      this.protocol = protocol;
      this.hostname = "";
      this.port = "";
      this.host = "";
      this.pathname = full.slice(protocol.length, pathEnd);
      this.search = searchValue;
      this.hash = hashValue;
      this.href = protocol + this.pathname + this.search + this.hash;
      this.origin = "null";
      this.searchParams = new URLSearchParams(this.search);
      const syncHrefFromSearchParams = () => {
        const query = this.searchParams.toString();
        this.search = query ? `?${query}` : "";
        this.href = protocol + this.pathname + this.search + this.hash;
      };
      for (const method of ["append", "delete", "set", "sort"]) {
        const original = this.searchParams[method]?.bind(this.searchParams);
        if (!original) {
          continue;
        }
        this.searchParams[method] = (...args) => {
          const result = original(...args);
          syncHrefFromSearchParams();
          return result;
        };
      }
      return;
    }
    const match = full.match(/^(\w+:)\/\/([^/:?#]+)(:\d+)?(.*)$/);
    this.protocol = match?.[1] || "";
    this.hostname = match?.[2] || "";
    this.port = (match?.[3] || "").slice(1);
    this.pathname = (match?.[4] || "/").split("?")[0].split("#")[0] || "/";
    this.search = full.includes("?") ? "?" + full.split("?")[1].split("#")[0] : "";
    this.hash = full.includes("#") ? "#" + full.split("#")[1] : "";
    this.host = this.hostname + (this.port ? ":" + this.port : "");
    this.href = this.protocol + "//" + this.host + this.pathname + this.search + this.hash;
    this.origin = this.protocol + "//" + this.host;
    this.searchParams = new URLSearchParams(this.search);
    const syncHrefFromSearchParams = () => {
      const query = this.searchParams.toString();
      this.search = query ? `?${query}` : "";
      this.href = this.protocol + "//" + this.host + this.pathname + this.search + this.hash;
    };
    for (const method of ["append", "delete", "set", "sort"]) {
      const original = this.searchParams[method]?.bind(this.searchParams);
      if (!original) {
        continue;
      }
      this.searchParams[method] = (...args) => {
        const result = original(...args);
        syncHrefFromSearchParams();
        return result;
      };
    }
  }
  toString() {
    return this.href;
  }
};
var URL2 = class _URL {
  #impl;
  #searchParams;
  constructor(input, base) {
    if (arguments.length < 1) {
      throw createMissingArgsError('The "url" argument must be specified');
    }
    const normalizedArgs = normalizeRelativeFileUrlInput(
      toNodeUSVString(input),
      arguments.length >= 2 ? toNodeUSVString(base) : void 0
    );
    try {
      this.#impl = normalizedArgs.base !== void 0 ? new NativeURL(normalizedArgs.input, normalizedArgs.base) : new NativeURL(normalizedArgs.input);
    } catch {
      throw createInvalidUrlError();
    }
  }
  static canParse(input, base) {
    if (arguments.length < 1) {
      throw createMissingArgsError('The "url" argument must be specified');
    }
    try {
      if (arguments.length >= 2) {
        new _URL(input, base);
      } else {
        new _URL(input);
      }
      return true;
    } catch {
      return false;
    }
  }
  static createObjectURL(obj) {
    if (typeof Blob === "undefined" || !(obj instanceof Blob)) {
      throw createNodeTypeError(
        'The "obj" argument must be an instance of Blob. Received ' + (obj === null ? "null" : typeof obj),
        "ERR_INVALID_ARG_TYPE"
      );
    }
    const id = `blob:nodedata:${nextBlobUrlId()}`;
    getBlobUrlStore().set(id, obj);
    return id;
  }
  static revokeObjectURL(url) {
    if (arguments.length < 1) {
      throw createMissingArgsError('The "url" argument must be specified');
    }
    if (typeof url !== "string") {
      return;
    }
    getBlobUrlStore().delete(url);
  }
  get href() {
    if (!(this instanceof _URL)) {
      throw createUrlReceiverTypeError();
    }
    return this.#impl.href;
  }
  set href(value) {
    this.#impl.href = toNodeUSVString(value);
  }
  get origin() {
    return this.#impl.origin;
  }
  get protocol() {
    return this.#impl.protocol;
  }
  set protocol(value) {
    this.#impl.protocol = toNodeUSVString(value);
  }
  get username() {
    return this.#impl.username;
  }
  set username(value) {
    this.#impl.username = toNodeUSVString(value);
  }
  get password() {
    return this.#impl.password;
  }
  set password(value) {
    this.#impl.password = toNodeUSVString(value);
  }
  get host() {
    return this.#impl.host;
  }
  set host(value) {
    this.#impl.host = toNodeUSVString(value);
  }
  get hostname() {
    return this.#impl.hostname;
  }
  set hostname(value) {
    this.#impl.hostname = toNodeUSVString(value);
  }
  get port() {
    return this.#impl.port;
  }
  set port(value) {
    this.#impl.port = toNodeUSVString(value);
  }
  get pathname() {
    return this.#impl.pathname;
  }
  set pathname(value) {
    this.#impl.pathname = toNodeUSVString(value);
  }
  get search() {
    if (!(this instanceof _URL)) {
      throw createUrlReceiverTypeError();
    }
    return this.#impl.search;
  }
  set search(value) {
    this.#impl.search = toNodeUSVString(value);
  }
  get searchParams() {
    if (!this.#searchParams) {
      this.#searchParams = new URLSearchParams({
        [kLinkedSearchParams]: () => this.#impl.searchParams
      });
    }
    return this.#searchParams;
  }
  get hash() {
    return this.#impl.hash;
  }
  set hash(value) {
    this.#impl.hash = toNodeUSVString(value);
  }
  toString() {
    if (!(this instanceof _URL)) {
      throw createUrlReceiverTypeError();
    }
    return this.#impl.href;
  }
  toJSON() {
    if (!(this instanceof _URL)) {
      throw createUrlReceiverTypeError();
    }
    return this.#impl.href;
  }
  [inspectCustomSymbol](depth, options, inspect) {
    const inspectName = this.constructor === _URL ? "URL" : this.constructor.name;
    if (depth < 0) {
      return `${inspectName} {}`;
    }
    const formatValue = typeof inspect === "function" ? (value) => inspect(value, options) : (value) => JSON.stringify(value);
    const lines = [
      `${inspectName} {`,
      `  href: ${formatValue(this.href)},`,
      `  origin: ${formatValue(this.origin)},`,
      `  protocol: ${formatValue(this.protocol)},`,
      `  username: ${formatValue(this.username)},`,
      `  password: ${formatValue(this.password)},`,
      `  host: ${formatValue(this.host)},`,
      `  hostname: ${formatValue(this.hostname)},`,
      `  port: ${formatValue(this.port)},`,
      `  pathname: ${formatValue(this.pathname)},`,
      `  search: ${formatValue(this.search)},`,
      `  searchParams: ${this.searchParams[inspectCustomSymbol](depth - 1, void 0, inspect)},`,
      `  hash: ${formatValue(this.hash)}`
    ];
    if (options?.showHidden) {
      lines[lines.length - 1] += ",";
      lines.push(`  [Symbol(context)]: ${formatUrlContext(this, inspect, options)}`);
    }
    lines.push("}");
    return lines.join("\n");
  }
  get [toStringTagSymbol]() {
    return "URL";
  }
};
for (const name of ["toString", "toJSON"]) {
  Object.defineProperty(URL2.prototype, name, {
    value: URL2.prototype[name],
    writable: true,
    configurable: true,
    enumerable: true
  });
}
for (const name of [
  "href",
  "protocol",
  "username",
  "password",
  "host",
  "hostname",
  "port",
  "pathname",
  "search",
  "hash",
  "origin",
  "searchParams"
]) {
  const descriptor = Object.getOwnPropertyDescriptor(URL2.prototype, name);
  if (!descriptor) {
    continue;
  }
  descriptor.enumerable = true;
  Object.defineProperty(URL2.prototype, name, descriptor);
}
Object.defineProperties(URL2.prototype, {
  [inspectCustomSymbol]: {
    value: URL2.prototype[inspectCustomSymbol],
    writable: true,
    configurable: true,
    enumerable: false
  },
  [toStringTagSymbol]: {
    get: Object.getOwnPropertyDescriptor(URL2.prototype, toStringTagSymbol)?.get,
    configurable: true,
    enumerable: false
  }
});
for (const name of ["canParse", "createObjectURL", "revokeObjectURL"]) {
  Object.defineProperty(URL2, name, {
    value: URL2[name],
    writable: true,
    configurable: true,
    enumerable: true
  });
}
function installWhatwgUrlGlobals(target = globalThis) {
  Object.defineProperty(target, "URL", {
    value: URL2,
    writable: true,
    configurable: true,
    enumerable: false
  });
  Object.defineProperty(target, "URLSearchParams", {
    value: URLSearchParams,
    writable: true,
    configurable: true,
    enumerable: false
  });
}
export { inspectCustomSymbol, toStringTagSymbol, ERR_INVALID_THIS, ERR_MISSING_ARGS, ERR_INVALID_URL, ERR_ARG_NOT_ITERABLE, ERR_INVALID_TUPLE, URL_SEARCH_PARAMS_TYPE, kLinkedSearchParams, kBlobUrlStore, kBlobUrlCounter, SEARCH_PARAM_METHOD_NAMES, SEARCH_PARAM_PAIR_METHOD_NAMES, URL_SCHEME_TYPES, searchParamsBrand, searchParamsState, searchParamsIteratorBrand, searchParamsIteratorState, createNodeTypeError, createInvalidUrlError, createUrlReceiverTypeError, createMissingArgsError, createIterableTypeError, createTupleTypeError, createSymbolStringError, toNodeString, toWellFormedString, toNodeUSVString, assertUrlSearchParamsReceiver, assertUrlSearchParamsIteratorReceiver, getUrlSearchParamsImpl, countSearchParams, isAsciiHexCodeUnit, decodeAsciiHexCodeUnit, appendUtf8CodePoint, decodeFormUrlencodedComponent, serializeFormUrlencodedComponent, compareCodeUnitStrings, normalizeSearchParamsInit, NativeURLSearchParams, createStandaloneSearchParams, createCollectionBody, createUrlContext, formatUrlContext, getBlobUrlStore, nextBlobUrlId, URLSearchParamsIterator, URLSearchParams, canUseNativeUrlImplementation, ensureTrailingSlashForFilePath, normalizeRelativeFileUrlInput, nativeUrlCandidate, NativeURL, URL2, installWhatwgUrlGlobals };
