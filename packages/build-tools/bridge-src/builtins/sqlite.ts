import { createBridgeSyncFacade } from "./fs.js";
import { exposeCustomGlobal } from "../global-exposure.js";
import { dgramModule } from "./dgram.js";

function isSqlitePlainObject(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  if (Buffer.isBuffer(value) || value instanceof Uint8Array) {
    return false;
  }
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

function encodeSqliteValue(value) {
  if (value === null || value === void 0 || typeof value === "boolean" || typeof value === "number" || typeof value === "string") {
    return value ?? null;
  }
  if (typeof value === "bigint") {
    return {
      __agentosSqliteType: "bigint",
      value: value.toString()
    };
  }
  if (Buffer.isBuffer(value) || value instanceof Uint8Array) {
    return {
      __agentosSqliteType: "uint8array",
      value: Buffer.from(value).toString("base64")
    };
  }
  if (Array.isArray(value)) {
    return value.map((entry) => encodeSqliteValue(entry));
  }
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => [key, encodeSqliteValue(entry)])
    );
  }
  return null;
}

function decodeSqliteValue(value) {
  if (value === null || value === void 0 || typeof value === "boolean" || typeof value === "number" || typeof value === "string") {
    return value ?? null;
  }
  if (Array.isArray(value)) {
    return value.map((entry) => decodeSqliteValue(entry));
  }
  if (value && typeof value === "object") {
    if (value.__agentosSqliteType === "bigint" && typeof value.value === "string") {
      return BigInt(value.value);
    }
    if (value.__agentosSqliteType === "uint8array" && typeof value.value === "string") {
      return Buffer.from(value.value, "base64");
    }
    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => [key, decodeSqliteValue(entry)])
    );
  }
  return value;
}

function normalizeSqliteParams(params) {
  if (!Array.isArray(params) || params.length === 0) {
    return null;
  }
  if (params.length === 1 && isSqlitePlainObject(params[0])) {
    return encodeSqliteValue(params[0]);
  }
  return params.map((entry) => encodeSqliteValue(entry));
}

function sqliteBridgeCall(bridgeFn, args, label) {
  if (typeof bridgeFn === "function") {
    return decodeSqliteValue(bridgeFn(...args));
  }
  if (!bridgeFn) {
    throw new Error(`sqlite bridge is not available for ${label}`);
  }
  if (typeof bridgeFn.applySync === "function") {
    return decodeSqliteValue(bridgeFn.applySync(void 0, args));
  }
  if (typeof bridgeFn.applySyncPromise === "function") {
    return decodeSqliteValue(bridgeFn.applySyncPromise(void 0, args));
  }
  throw new Error(`sqlite bridge is not available for ${label}`);
}

var _sqliteConstants = createBridgeSyncFacade("_sqliteConstantsRaw");

var _sqliteDatabaseOpen = createBridgeSyncFacade("_sqliteDatabaseOpenRaw");

var _sqliteDatabaseClose = createBridgeSyncFacade("_sqliteDatabaseCloseRaw");

var _sqliteDatabaseExec = createBridgeSyncFacade("_sqliteDatabaseExecRaw");

var _sqliteDatabaseQuery = createBridgeSyncFacade("_sqliteDatabaseQueryRaw");

var _sqliteDatabasePrepare = createBridgeSyncFacade("_sqliteDatabasePrepareRaw");

var _sqliteDatabaseLocation = createBridgeSyncFacade("_sqliteDatabaseLocationRaw");

var _sqliteDatabaseCheckpoint = createBridgeSyncFacade("_sqliteDatabaseCheckpointRaw");

var _sqliteStatementRun = createBridgeSyncFacade("_sqliteStatementRunRaw");

var _sqliteStatementGet = createBridgeSyncFacade("_sqliteStatementGetRaw");

var _sqliteStatementAll = createBridgeSyncFacade("_sqliteStatementAllRaw");

var _sqliteStatementColumns = createBridgeSyncFacade("_sqliteStatementColumnsRaw");

var _sqliteStatementSetReturnArrays = createBridgeSyncFacade("_sqliteStatementSetReturnArraysRaw");

var _sqliteStatementSetReadBigInts = createBridgeSyncFacade("_sqliteStatementSetReadBigIntsRaw");

var _sqliteStatementSetAllowBareNamedParameters = createBridgeSyncFacade("_sqliteStatementSetAllowBareNamedParametersRaw");

var _sqliteStatementSetAllowUnknownNamedParameters = createBridgeSyncFacade("_sqliteStatementSetAllowUnknownNamedParametersRaw");

var _sqliteStatementFinalize = createBridgeSyncFacade("_sqliteStatementFinalizeRaw");

var StatementSync = class {
  constructor(database, statementId) {
    this._database = database;
    this._statementId = statementId;
    this._finalized = false;
  }
  _assertOpen() {
    this._database._assertOpen();
    if (this._finalized) {
      throw new Error("SQLite statement is already finalized");
    }
  }
  run(...params) {
    this._assertOpen();
    return sqliteBridgeCall(
      _sqliteStatementRun,
      [this._statementId, normalizeSqliteParams(params)],
      "statement.run"
    );
  }
  get(...params) {
    this._assertOpen();
    return sqliteBridgeCall(
      _sqliteStatementGet,
      [this._statementId, normalizeSqliteParams(params)],
      "statement.get"
    );
  }
  all(...params) {
    this._assertOpen();
    return sqliteBridgeCall(
      _sqliteStatementAll,
      [this._statementId, normalizeSqliteParams(params)],
      "statement.all"
    );
  }
  iterate(...params) {
    const rows = this.all(...params);
    return rows[Symbol.iterator]();
  }
  columns() {
    this._assertOpen();
    return sqliteBridgeCall(
      _sqliteStatementColumns,
      [this._statementId],
      "statement.columns"
    );
  }
  setReturnArrays(enabled) {
    this._assertOpen();
    sqliteBridgeCall(
      _sqliteStatementSetReturnArrays,
      [this._statementId, Boolean(enabled)],
      "statement.setReturnArrays"
    );
  }
  setReadBigInts(enabled) {
    this._assertOpen();
    sqliteBridgeCall(
      _sqliteStatementSetReadBigInts,
      [this._statementId, Boolean(enabled)],
      "statement.setReadBigInts"
    );
  }
  setAllowBareNamedParameters(enabled) {
    this._assertOpen();
    sqliteBridgeCall(
      _sqliteStatementSetAllowBareNamedParameters,
      [this._statementId, Boolean(enabled)],
      "statement.setAllowBareNamedParameters"
    );
  }
  setAllowUnknownNamedParameters(enabled) {
    this._assertOpen();
    sqliteBridgeCall(
      _sqliteStatementSetAllowUnknownNamedParameters,
      [this._statementId, Boolean(enabled)],
      "statement.setAllowUnknownNamedParameters"
    );
  }
  finalize() {
    if (this._finalized) {
      return null;
    }
    this._database._assertOpen();
    sqliteBridgeCall(
      _sqliteStatementFinalize,
      [this._statementId],
      "statement.finalize"
    );
    this._finalized = true;
    return null;
  }
};

var DatabaseSync = class {
  constructor(location = ":memory:", options = void 0) {
    this._closed = false;
    this._databaseId = sqliteBridgeCall(
      _sqliteDatabaseOpen,
      [typeof location === "string" ? location : ":memory:", options ?? null],
      "database.open"
    );
  }
  _assertOpen() {
    if (this._closed) {
      throw new Error("SQLite database is already closed");
    }
  }
  close() {
    if (this._closed) {
      return null;
    }
    sqliteBridgeCall(
      _sqliteDatabaseClose,
      [this._databaseId],
      "database.close"
    );
    this._closed = true;
    return null;
  }
  exec(sql) {
    this._assertOpen();
    return sqliteBridgeCall(
      _sqliteDatabaseExec,
      [this._databaseId, String(sql ?? "")],
      "database.exec"
    );
  }
  query(sql, params = null, options = null) {
    this._assertOpen();
    const normalized = params === null ? null : normalizeSqliteParams(Array.isArray(params) ? params : [params]);
    return sqliteBridgeCall(
      _sqliteDatabaseQuery,
      [this._databaseId, String(sql ?? ""), normalized, options ?? null],
      "database.query"
    );
  }
  prepare(sql) {
    this._assertOpen();
    const statementId = sqliteBridgeCall(
      _sqliteDatabasePrepare,
      [this._databaseId, String(sql ?? "")],
      "database.prepare"
    );
    return new StatementSync(this, statementId);
  }
  location() {
    this._assertOpen();
    return sqliteBridgeCall(
      _sqliteDatabaseLocation,
      [this._databaseId],
      "database.location"
    );
  }
  checkpoint() {
    this._assertOpen();
    return sqliteBridgeCall(
      _sqliteDatabaseCheckpoint,
      [this._databaseId],
      "database.checkpoint"
    );
  }
};

DatabaseSync.prototype[Symbol.dispose] = DatabaseSync.prototype.close;

StatementSync.prototype[Symbol.dispose] = StatementSync.prototype.finalize;

var sqliteConstants;

function getSqliteConstants() {
  if (sqliteConstants === void 0) {
    sqliteConstants = Object.freeze(
      sqliteBridgeCall(_sqliteConstants, [], "constants") ?? {}
    );
  }
  return sqliteConstants;
}

var sqliteModule = {
  DatabaseSync,
  StatementSync,
  get constants() {
    return getSqliteConstants();
  }
};

exposeCustomGlobal("_dgramModule", dgramModule);
exposeCustomGlobal("_sqliteModule", sqliteModule);
export { DatabaseSync, StatementSync, _sqliteConstants, _sqliteDatabaseCheckpoint, _sqliteDatabaseClose, _sqliteDatabaseExec, _sqliteDatabaseLocation, _sqliteDatabaseOpen, _sqliteDatabasePrepare, _sqliteDatabaseQuery, _sqliteStatementAll, _sqliteStatementColumns, _sqliteStatementFinalize, _sqliteStatementGet, _sqliteStatementRun, _sqliteStatementSetAllowBareNamedParameters, _sqliteStatementSetAllowUnknownNamedParameters, _sqliteStatementSetReadBigInts, _sqliteStatementSetReturnArrays, decodeSqliteValue, encodeSqliteValue, getSqliteConstants, isSqlitePlainObject, normalizeSqliteParams, sqliteBridgeCall, sqliteConstants, sqliteModule };
