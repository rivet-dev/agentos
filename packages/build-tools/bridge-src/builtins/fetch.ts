import { getSecureExecUndiciDispatcher, undiciFetch } from "./undici.js";
import { exposeCustomGlobal, exposeInstallCompatibleHardenedGlobal } from "../global-exposure.js";
import { undiciHeadersModule, undiciRequestModule, undiciResponseModule } from "../prelude.js";
import { isFlatHeaderList, onUpgradeSocketEnd } from "./http.js";

var MAX_HTTP_BODY_BYTES = 50 * 1024 * 1024;

var MAX_HTTP_REQUEST_HEADER_BYTES = 64 * 1024;

var MAX_HTTP_REQUEST_HEADERS = 2e3;

var _fetchHandleCounter = 0;

var UndiciHeaders = undiciHeadersModule?.Headers ?? undiciHeadersModule?.default ?? undiciHeadersModule;

var UndiciRequest = undiciRequestModule?.Request ?? undiciRequestModule?.default ?? undiciRequestModule;

var UndiciResponse = undiciResponseModule?.Response ?? undiciResponseModule?.default ?? undiciResponseModule;

function serializeFetchHeaders(headers) {
  if (!headers) {
    return {};
  }
  if (headers instanceof Headers) {
    return Object.fromEntries(headers.entries());
  }
  if (typeof UndiciHeaders === "function" && headers instanceof UndiciHeaders) {
    return Object.fromEntries(headers.entries());
  }
  if (isFlatHeaderList(headers)) {
    const normalized = {};
    for (let index = 0; index < headers.length; index += 2) {
      const key = headers[index];
      const value = headers[index + 1];
      if (key !== void 0 && value !== void 0) {
        normalized[key] = value;
      }
    }
    return normalized;
  }
  if (typeof headers.entries === "function") {
    return Object.fromEntries(headers.entries());
  }
  if (typeof headers[Symbol.iterator] === "function") {
    return Object.fromEntries(headers);
  }
  return Object.fromEntries(new Headers(headers).entries());
}

function createFetchHeaders(headers) {
  return new Headers(serializeFetchHeaders(headers));
}

function normalizeFetchRequestInit(options = {}) {
  const normalized = { ...options };
  // Some bundled Node SDKs pass node-fetch style `agent` options into fetch().
  // Undici doesn't accept that field, and the default global dispatcher already
  // routes through the secure-exec virtual network stack.
  if (Object.prototype.hasOwnProperty.call(normalized, "agent")) {
    delete normalized.agent;
  }
  if (Object.prototype.hasOwnProperty.call(normalized, "headers")) {
    normalized.headers = serializeFetchHeaders(normalized.headers);
  }
  if (
    normalized.body != null &&
    normalized.duplex == null &&
    String(normalized.method ?? "GET").toUpperCase() !== "GET" &&
    String(normalized.method ?? "GET").toUpperCase() !== "HEAD"
  ) {
    normalized.duplex = "half";
  }
  return normalized;
}

function ensureFetchAcceptEncoding(options) {
  const headers = serializeFetchHeaders(options?.headers);
  const hasAcceptEncoding = Object.keys(headers).some(
    (key) => key.toLowerCase() === "accept-encoding"
  );
  if (!hasAcceptEncoding) {
    headers["accept-encoding"] = "gzip, deflate";
  }
  return { ...(options || {}), headers };
}

async function fetch(input, options = {}) {
  if (typeof undiciFetch !== "function") {
    throw new Error("fetch requires undici to be configured");
  }
  let resolvedInput = input;
  let normalizedOptions = options;
  if (input instanceof Request || typeof UndiciRequest === "function" && input instanceof UndiciRequest) {
    resolvedInput = input.url;
    normalizedOptions = {
      method: input.method,
      headers: serializeFetchHeaders(input.headers),
      body: input.body,
      ...options
    };
  }
  normalizedOptions = normalizeFetchRequestInit(normalizedOptions);
  normalizedOptions = ensureFetchAcceptEncoding(normalizedOptions);
  const requestLabel = typeof resolvedInput === "string" ? resolvedInput : resolvedInput?.url ? String(resolvedInput.url) : String(resolvedInput);
  const handleId = typeof _registerHandle === "function" ? `fetch:${++_fetchHandleCounter}` : null;
  if (handleId) {
    _registerHandle?.(handleId, `fetch ${requestLabel}`);
  }
  // Shared bounded dispatcher (see undici.ts): keepalive pooling across fetch()
  // calls. Per-call dispatchers (the 4f470c61 workaround for pooled clients
  // going stale against released sockets) are no longer needed now that
  // host->guest socket event push keeps pooled connections live.
  const fetchDispatcher = normalizedOptions.dispatcher == null && typeof getSecureExecUndiciDispatcher === "function" ? getSecureExecUndiciDispatcher() : null;
  try {
    return await undiciFetch(
      resolvedInput,
      fetchDispatcher ? { ...normalizedOptions, dispatcher: fetchDispatcher } : normalizedOptions
    );
  } finally {
    if (handleId) {
      _unregisterHandle?.(handleId);
    }
  }
}

var Headers = class _Headers {
  _headers = {};
  constructor(init) {
    if (init && init !== null) {
      if (init instanceof _Headers) {
        this._headers = { ...init._headers };
      } else if (Array.isArray(init)) {
        init.forEach(([key, value]) => {
          this._headers[key.toLowerCase()] = value;
        });
      } else if (typeof init === "object") {
        Object.entries(init).forEach(([key, value]) => {
          this._headers[key.toLowerCase()] = value;
        });
      }
    }
  }
  get(name) {
    return this._headers[name.toLowerCase()] || null;
  }
  set(name, value) {
    this._headers[name.toLowerCase()] = value;
  }
  has(name) {
    return name.toLowerCase() in this._headers;
  }
  delete(name) {
    delete this._headers[name.toLowerCase()];
  }
  entries() {
    return Object.entries(this._headers)[Symbol.iterator]();
  }
  [Symbol.iterator]() {
    return this.entries();
  }
  keys() {
    return Object.keys(this._headers)[Symbol.iterator]();
  }
  values() {
    return Object.values(this._headers)[Symbol.iterator]();
  }
  append(name, value) {
    const key = name.toLowerCase();
    if (key in this._headers) {
      this._headers[key] = this._headers[key] + ", " + value;
    } else {
      this._headers[key] = value;
    }
  }
  forEach(callback) {
    Object.entries(this._headers).forEach(([k, v]) => callback(v, k, this));
  }
};

var Request = class _Request {
  url;
  method;
  headers;
  body;
  mode;
  credentials;
  cache;
  redirect;
  referrer;
  integrity;
  constructor(input, init = {}) {
    this.url = typeof input === "string" ? input : input.url;
    this.method = init.method || (typeof input !== "string" ? input.method : void 0) || "GET";
    this.headers = createFetchHeaders(
      init.headers || (typeof input !== "string" ? input.headers : void 0)
    );
    this.body = init.body || null;
    this.mode = init.mode || "cors";
    this.credentials = init.credentials || "same-origin";
    this.cache = init.cache || "default";
    this.redirect = init.redirect || "follow";
    this.referrer = init.referrer || "about:client";
    this.integrity = init.integrity || "";
  }
  clone() {
    return new _Request(this.url, this);
  }
};

var Response = class _Response {
  _body;
  status;
  statusText;
  headers;
  ok;
  type;
  url;
  redirected;
  constructor(body, init = {}) {
    this._body = body || null;
    this.status = init.status || 200;
    this.statusText = init.statusText || "OK";
    this.headers = new Headers(init.headers);
    this.ok = this.status >= 200 && this.status < 300;
    this.type = "default";
    this.url = "";
    this.redirected = false;
  }
  async text() {
    return String(this._body || "");
  }
  async json() {
    return JSON.parse(this._body || "{}");
  }
  get body() {
    const bodyStr = this._body;
    if (bodyStr === null) return null;
    return {
      getReader() {
        let consumed = false;
        return {
          async read() {
            if (consumed) return { done: true };
            consumed = true;
            const encoder = new TextEncoder();
            return { done: false, value: encoder.encode(bodyStr) };
          }
        };
      }
    };
  }
  clone() {
    return new _Response(this._body, { status: this.status, statusText: this.statusText });
  }
  static error() {
    return new _Response(null, { status: 0, statusText: "" });
  }
  static redirect(url, status = 302) {
    return new _Response(null, { status, headers: { Location: url } });
  }
};

exposeCustomGlobal("_upgradeSocketEnd", onUpgradeSocketEnd);

exposeInstallCompatibleHardenedGlobal("fetch", fetch);

exposeInstallCompatibleHardenedGlobal("Headers", UndiciHeaders);

exposeInstallCompatibleHardenedGlobal("Request", UndiciRequest);

exposeInstallCompatibleHardenedGlobal("Response", UndiciResponse);

var Blob = globalThis.Blob;

if (typeof Blob === "undefined") {
  Blob = class BlobStub {
  };
}
exposeInstallCompatibleHardenedGlobal("Blob", Blob);

var File = globalThis.File;

if (typeof File === "undefined") {
  File = class FileStub extends Blob {
    name;
    lastModified;
    webkitRelativePath;
    constructor(parts = [], name = "", options = {}) {
      super(parts, options);
      this.name = String(name);
      this.lastModified = typeof options.lastModified === "number" ? options.lastModified : Date.now();
      this.webkitRelativePath = "";
    }
  };
}
exposeInstallCompatibleHardenedGlobal("File", File);

var FormData = globalThis.FormData;

if (typeof FormData === "undefined") {
  FormData = class FormDataStub {
    _entries = [];
    append(name, value) {
      this._entries.push([name, value]);
    }
    get(name) {
      const entry = this._entries.find(([k]) => k === name);
      return entry ? entry[1] : null;
    }
    getAll(name) {
      return this._entries.filter(([k]) => k === name).map(([, v]) => v);
    }
    has(name) {
      return this._entries.some(([k]) => k === name);
    }
    delete(name) {
      this._entries = this._entries.filter(([k]) => k !== name);
    }
    entries() {
      return this._entries[Symbol.iterator]();
    }
    [Symbol.iterator]() {
      return this.entries();
    }
  };
}
exposeInstallCompatibleHardenedGlobal("FormData", FormData);
export { Blob, File, Headers, MAX_HTTP_BODY_BYTES, MAX_HTTP_REQUEST_HEADERS, MAX_HTTP_REQUEST_HEADER_BYTES, Request, Response, UndiciHeaders, UndiciRequest, UndiciResponse, _fetchHandleCounter, createFetchHeaders, ensureFetchAcceptEncoding, fetch, normalizeFetchRequestInit, serializeFetchHeaders };
