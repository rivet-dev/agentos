import { exposeCustomGlobal } from "../global-exposure.js";
import { dgramModule } from "./dgram.js";
import { createBridgeSyncFacade } from "./fs.js";

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
	if (
		value === null ||
		value === void 0 ||
		typeof value === "boolean" ||
		typeof value === "string"
	) {
		return value ?? null;
	}
	if (typeof value === "number") {
		const bytes = Buffer.allocUnsafe(8);
		bytes.writeDoubleBE(value);
		return { __agentosSqliteType: "double", value: bytes.toString("hex") };
	}
	if (typeof value === "bigint") {
		return {
			__agentosSqliteType: "bigint",
			value: value.toString(),
		};
	}
	if (Buffer.isBuffer(value) || value instanceof Uint8Array) {
		return {
			__agentosSqliteType: "uint8array",
			value: Buffer.from(value).toString("base64"),
		};
	}
	if (Array.isArray(value)) {
		return value.map((entry) => encodeSqliteValue(entry));
	}
	if (value && typeof value === "object") {
		return Object.fromEntries(
			Object.entries(value).map(([key, entry]) => [
				key,
				encodeSqliteValue(entry),
			]),
		);
	}
	return null;
}

function decodeSqliteValue(value) {
	if (
		value === null ||
		value === void 0 ||
		typeof value === "boolean" ||
		typeof value === "number" ||
		typeof value === "string"
	) {
		return value ?? null;
	}
	if (Array.isArray(value)) {
		return value.map((entry) => decodeSqliteValue(entry));
	}
	if (value && typeof value === "object") {
		if (value.__agentosSqliteType === "double") {
			if (
				typeof value.value !== "string" ||
				!/^[0-9a-f]{16}$/.test(value.value)
			)
				throw new Error("Invalid SQLite double value");
			return Buffer.from(value.value, "hex").readDoubleBE();
		}
		if (
			value.__agentosSqliteType === "bigint" &&
			typeof value.value === "string"
		) {
			return BigInt(value.value);
		}
		if (
			value.__agentosSqliteType === "uint8array" &&
			typeof value.value === "string"
		) {
			return Buffer.from(value.value, "base64");
		}
		return Object.fromEntries(
			Object.entries(value).map(([key, entry]) => [
				key,
				decodeSqliteValue(entry),
			]),
		);
	}
	return value;
}

function normalizeSqliteParamsRaw(params) {
	if (!Array.isArray(params) || params.length === 0) {
		return null;
	}
	if (params.length === 1 && isSqlitePlainObject(params[0])) {
		return encodeSqliteValue(params[0]);
	}
	return params.map((entry) => encodeSqliteValue(entry));
}

const SQLITE_TRANSFER_CHUNK = 32 * 1024;
const SQLITE_TRANSFER_INLINE = 256 * 1024;
const sqliteParameters = Symbol("sqlite.parameters");
function normalizeSqliteParams(params, database) {
	return {
		[sqliteParameters]: true,
		database,
		value: normalizeSqliteParamsRaw(params),
	};
}
function invokeSqliteBridge(bridgeFn, args, label) {
	if (typeof bridgeFn === "function") return bridgeFn(...args);
	if (!bridgeFn) throw new Error(`sqlite bridge is not available for ${label}`);
	if (typeof bridgeFn.applySync === "function")
		return bridgeFn.applySync(void 0, args);
	if (typeof bridgeFn.applySyncPromise === "function")
		return bridgeFn.applySyncPromise(void 0, args);
	throw new Error(`sqlite bridge is not available for ${label}`);
}
function withSqliteCleanup<T>(action: () => T, cleanup: () => unknown): T {
	let failed = false;
	let original: unknown;
	let result!: T;
	try {
		result = action();
	} catch (error) {
		failed = true;
		original = error;
	}
	try {
		cleanup();
	} catch (error) {
		if (failed) {
			throw new AggregateError(
				[original, error],
				"SQLite transfer cleanup failed",
			);
		}
		throw error;
	}
	if (failed) throw original;
	return result;
}
function decodeSqliteReply(result) {
	if (result?.kind === "inline") return decodeSqliteValue(result.value);
	if (
		result?.kind !== "transfer" ||
		!Number.isSafeInteger(result.length) ||
		result.length < 1 ||
		result.length > 2147483647
	)
		throw new Error("Invalid SQLite result envelope");
	return withSqliteCleanup(
		() => {
			const bytes = Buffer.allocUnsafe(result.length);
			let offset = 0;
			while (offset < bytes.length) {
				const encoded = invokeSqliteBridge(
					_sqliteTransferRead,
					[result.database, result.generation, offset],
					"result.read",
				);
				const chunk = Buffer.from(encoded, "base64");
				if (
					chunk.length < 1 ||
					chunk.length > SQLITE_TRANSFER_CHUNK ||
					chunk.length > bytes.length - offset
				)
					throw new Error("Invalid SQLite result chunk");
				chunk.copy(bytes, offset);
				offset += chunk.length;
			}
			return decodeSqliteValue(JSON.parse(bytes.toString("utf8")));
		},
		() =>
			invokeSqliteBridge(
				_sqliteTransferRelease,
				[result.database, result.generation],
				"result.release",
			),
	);
}
function sqliteBridgeCall(bridgeFn, args, label) {
	let staged: { database: unknown; generation: unknown } | undefined;
	try {
		return withSqliteCleanup(
			() => {
				const index = args.findIndex(
					(value) => value?.[sqliteParameters] === true,
				);
				if (index >= 0) {
					args = args.slice();
					const parameter = args[index];
					const bytes = Buffer.from(JSON.stringify(parameter.value), "utf8");
					if (bytes.length > SQLITE_TRANSFER_INLINE) {
						const start = invokeSqliteBridge(
							_sqliteTransferBegin,
							[parameter.database, bytes.length],
							"parameters.begin",
						);
						staged = {
							database: parameter.database,
							generation: start.generation,
						};
						for (
							let offset = 0;
							offset < bytes.length;
							offset += SQLITE_TRANSFER_CHUNK
						) {
							invokeSqliteBridge(
								_sqliteTransferAppend,
								[
									staged.database,
									staged.generation,
									offset,
									bytes
										.subarray(offset, offset + SQLITE_TRANSFER_CHUNK)
										.toString("base64"),
								],
								"parameters.append",
							);
						}
						args[index] = null;
						args.push(staged);
					} else {
						args[index] = parameter.value;
						args.push(null);
					}
				}
				const result = invokeSqliteBridge(bridgeFn, args, label);
				return ["statement.get", "statement.all", "database.query"].includes(
					label,
				)
					? decodeSqliteReply(result)
					: decodeSqliteValue(result);
			},
			() => {
				if (staged)
					invokeSqliteBridge(
						_sqliteTransferCancel,
						[staged.database, staged.generation],
						"parameters.cancel",
					);
			},
		);
	} catch (error) {
		if (error?.code === "ERR_SQLITE_ERROR" && error.details) {
			if (typeof error.details.errcode === "number")
				error.errcode = error.details.errcode;
			if (typeof error.details.errstr === "string")
				error.errstr = error.details.errstr;
			delete error.details;
		}
		throw error;
	}
}
var _sqliteTransferBegin = createBridgeSyncFacade("_sqliteTransferBeginRaw");
var _sqliteTransferAppend = createBridgeSyncFacade("_sqliteTransferAppendRaw");
var _sqliteTransferCancel = createBridgeSyncFacade("_sqliteTransferCancelRaw");
var _sqliteTransferRead = createBridgeSyncFacade("_sqliteTransferReadRaw");
var _sqliteTransferRelease = createBridgeSyncFacade(
	"_sqliteTransferReleaseRaw",
);

var _sqliteConstants = createBridgeSyncFacade("_sqliteConstantsRaw");

var _sqliteDatabaseOpen = createBridgeSyncFacade("_sqliteDatabaseOpenRaw");

var _sqliteDatabaseClose = createBridgeSyncFacade("_sqliteDatabaseCloseRaw");

var _sqliteDatabaseExec = createBridgeSyncFacade("_sqliteDatabaseExecRaw");

var _sqliteDatabaseQuery = createBridgeSyncFacade(
	"_sqliteDatabaseQueryChunkedRaw",
);

var _sqliteDatabasePrepare = createBridgeSyncFacade(
	"_sqliteDatabasePrepareRaw",
);

var _sqliteDatabaseLocation = createBridgeSyncFacade(
	"_sqliteDatabaseLocationRaw",
);

var _sqliteDatabaseCheckpoint = createBridgeSyncFacade(
	"_sqliteDatabaseCheckpointRaw",
);
var _sqliteDatabaseIsTransaction = createBridgeSyncFacade(
	"_sqliteDatabaseIsTransactionRaw",
);

var _sqliteStatementRun = createBridgeSyncFacade(
	"_sqliteStatementRunChunkedRaw",
);

var _sqliteStatementGet = createBridgeSyncFacade(
	"_sqliteStatementGetChunkedRaw",
);

var _sqliteStatementAll = createBridgeSyncFacade(
	"_sqliteStatementAllChunkedRaw",
);

var _sqliteStatementColumns = createBridgeSyncFacade(
	"_sqliteStatementColumnsRaw",
);

var _sqliteStatementSetReturnArrays = createBridgeSyncFacade(
	"_sqliteStatementSetReturnArraysRaw",
);

var _sqliteStatementSetReadBigInts = createBridgeSyncFacade(
	"_sqliteStatementSetReadBigIntsRaw",
);

var _sqliteStatementSetAllowBareNamedParameters = createBridgeSyncFacade(
	"_sqliteStatementSetAllowBareNamedParametersRaw",
);

var _sqliteStatementSetAllowUnknownNamedParameters = createBridgeSyncFacade(
	"_sqliteStatementSetAllowUnknownNamedParametersRaw",
);

var _sqliteStatementFinalize = createBridgeSyncFacade(
	"_sqliteStatementFinalizeRaw",
);

var sqliteStatementFinalizer =
	typeof FinalizationRegistry === "function"
		? new FinalizationRegistry((statementId) => {
				try {
					sqliteBridgeCall(
						_sqliteStatementFinalize,
						[statementId],
						"statement garbage collection",
					);
				} catch {
					// Database close and process teardown also release statement handles.
				}
			})
		: null;

var StatementSync = class {
	constructor(database, sql, statementId) {
		this._database = database;
		this._sql = sql;
		this._statementId = statementId;
		this._finalized = false;
		this._returnArrays = false;
		this._readBigInts = false;
		this._allowBareNamedParameters = false;
		this._allowUnknownNamedParameters = false;
		sqliteStatementFinalizer?.register(this, statementId, this);
	}
	get sourceSQL() {
		this._assertOpen();
		return this._sql;
	}
	_assertOpen() {
		this._database._assertOpen();
		if (this._finalized) {
			throw new Error("SQLite statement is already finalized");
		}
	}
	_ensureStatement() {
		this._assertOpen();
		if (this._statementId !== null) return this._statementId;
		const { statementId } = sqliteBridgeCall(
			_sqliteDatabasePrepare,
			[this._database._databaseId, this._sql],
			"database.prepare",
		);
		this._statementId = statementId;
		sqliteStatementFinalizer?.register(this, statementId, this);
		if (this._returnArrays) {
			sqliteBridgeCall(
				_sqliteStatementSetReturnArrays,
				[statementId, true],
				"statement.setReturnArrays",
			);
		}
		if (this._readBigInts) {
			sqliteBridgeCall(
				_sqliteStatementSetReadBigInts,
				[statementId, true],
				"statement.setReadBigInts",
			);
		}
		if (this._allowBareNamedParameters) {
			sqliteBridgeCall(
				_sqliteStatementSetAllowBareNamedParameters,
				[statementId, true],
				"statement.setAllowBareNamedParameters",
			);
		}
		if (this._allowUnknownNamedParameters) {
			sqliteBridgeCall(
				_sqliteStatementSetAllowUnknownNamedParameters,
				[statementId, true],
				"statement.setAllowUnknownNamedParameters",
			);
		}
		return statementId;
	}
	_releaseStatement(suppressErrors = false) {
		if (this._statementId === null) return;
		const statementId = this._statementId;
		this._statementId = null;
		sqliteStatementFinalizer?.unregister(this);
		try {
			sqliteBridgeCall(
				_sqliteStatementFinalize,
				[statementId],
				"statement.finalize",
			);
		} catch (error) {
			if (!suppressErrors) throw error;
		}
	}
	_execute(bridgeFn, args, label) {
		const statementId = this._ensureStatement();
		let failed = false;
		try {
			return sqliteBridgeCall(bridgeFn, [statementId, ...args], label);
		} catch (error) {
			failed = true;
			throw error;
		} finally {
			this._releaseStatement(failed);
		}
	}
	run(...params) {
		return this._execute(
			_sqliteStatementRun,
			[normalizeSqliteParams(params, this._database._databaseId)],
			"statement.run",
		);
	}
	get(...params) {
		return this._execute(
			_sqliteStatementGet,
			[normalizeSqliteParams(params, this._database._databaseId)],
			"statement.get",
		);
	}
	all(...params) {
		return this._execute(
			_sqliteStatementAll,
			[normalizeSqliteParams(params, this._database._databaseId)],
			"statement.all",
		);
	}
	iterate(...params) {
		const rows = this.all(...params);
		return rows[Symbol.iterator]();
	}
	columns() {
		return this._execute(_sqliteStatementColumns, [], "statement.columns");
	}
	setReturnArrays(enabled) {
		this._assertOpen();
		this._returnArrays = Boolean(enabled);
		if (this._statementId !== null) {
			sqliteBridgeCall(
				_sqliteStatementSetReturnArrays,
				[this._statementId, this._returnArrays],
				"statement.setReturnArrays",
			);
		}
	}
	setReadBigInts(enabled) {
		this._assertOpen();
		this._readBigInts = Boolean(enabled);
		if (this._statementId !== null) {
			sqliteBridgeCall(
				_sqliteStatementSetReadBigInts,
				[this._statementId, this._readBigInts],
				"statement.setReadBigInts",
			);
		}
	}
	setAllowBareNamedParameters(enabled) {
		this._assertOpen();
		this._allowBareNamedParameters = Boolean(enabled);
		if (this._statementId !== null) {
			sqliteBridgeCall(
				_sqliteStatementSetAllowBareNamedParameters,
				[this._statementId, this._allowBareNamedParameters],
				"statement.setAllowBareNamedParameters",
			);
		}
	}
	setAllowUnknownNamedParameters(enabled) {
		this._assertOpen();
		this._allowUnknownNamedParameters = Boolean(enabled);
		if (this._statementId !== null) {
			sqliteBridgeCall(
				_sqliteStatementSetAllowUnknownNamedParameters,
				[this._statementId, this._allowUnknownNamedParameters],
				"statement.setAllowUnknownNamedParameters",
			);
		}
	}
	finalize() {
		if (this._finalized) {
			return null;
		}
		this._database._assertOpen();
		this._releaseStatement();
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
			"database.open",
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
			"database.close",
		);
		this._closed = true;
		return null;
	}
	exec(sql) {
		this._assertOpen();
		return sqliteBridgeCall(
			_sqliteDatabaseExec,
			[this._databaseId, String(sql ?? "")],
			"database.exec",
		);
	}
	query(sql, params = null, options = null) {
		this._assertOpen();
		const normalized =
			params === null
				? null
				: normalizeSqliteParams(
						Array.isArray(params) ? params : [params],
						this._databaseId,
					);
		return sqliteBridgeCall(
			_sqliteDatabaseQuery,
			[this._databaseId, String(sql ?? ""), normalized, options ?? null],
			"database.query",
		);
	}
	prepare(sql) {
		this._assertOpen();
		const normalizedSql = String(sql ?? "");
		const { statementId, sourceSQLLength } = sqliteBridgeCall(
			_sqliteDatabasePrepare,
			[this._databaseId, normalizedSql],
			"database.prepare",
		);
		if (
			!Number.isSafeInteger(sourceSQLLength) ||
			sourceSQLLength < 0 ||
			sourceSQLLength > normalizedSql.length
		) {
			throw Object.assign(new Error("Invalid SQLite statement source length"), {
				code: "ERR_SQLITE_ERROR",
			});
		}
		return new StatementSync(
			this,
			normalizedSql.slice(0, sourceSQLLength),
			statementId,
		);
	}
	location() {
		this._assertOpen();
		return sqliteBridgeCall(
			_sqliteDatabaseLocation,
			[this._databaseId],
			"database.location",
		);
	}
	get isTransaction() {
		this._assertOpen();
		return sqliteBridgeCall(
			_sqliteDatabaseIsTransaction,
			[this._databaseId],
			"database.isTransaction",
		);
	}
	checkpoint() {
		this._assertOpen();
		return sqliteBridgeCall(
			_sqliteDatabaseCheckpoint,
			[this._databaseId],
			"database.checkpoint",
		);
	}
};

DatabaseSync.prototype[Symbol.dispose] = DatabaseSync.prototype.close;

StatementSync.prototype[Symbol.dispose] = StatementSync.prototype.finalize;

var sqliteConstants: Readonly<Record<string, number>> | undefined;

function getSqliteConstants() {
	if (sqliteConstants === void 0) {
		sqliteConstants = Object.freeze(
			sqliteBridgeCall(_sqliteConstants, [], "constants") ?? {},
		);
	}
	return sqliteConstants;
}

var sqliteModule = {
	DatabaseSync,
	StatementSync,
	get constants() {
		return getSqliteConstants();
	},
};

exposeCustomGlobal("_dgramModule", dgramModule);
exposeCustomGlobal("_sqliteModule", sqliteModule);

export {
	_sqliteConstants,
	_sqliteDatabaseCheckpoint,
	_sqliteDatabaseClose,
	_sqliteDatabaseExec,
	_sqliteDatabaseLocation,
	_sqliteDatabaseOpen,
	_sqliteDatabasePrepare,
	_sqliteDatabaseQuery,
	_sqliteStatementAll,
	_sqliteStatementColumns,
	_sqliteStatementFinalize,
	_sqliteStatementGet,
	_sqliteStatementRun,
	_sqliteStatementSetAllowBareNamedParameters,
	_sqliteStatementSetAllowUnknownNamedParameters,
	_sqliteStatementSetReadBigInts,
	_sqliteStatementSetReturnArrays,
	DatabaseSync,
	decodeSqliteValue,
	encodeSqliteValue,
	getSqliteConstants,
	isSqlitePlainObject,
	normalizeSqliteParams,
	StatementSync,
	sqliteBridgeCall,
	sqliteConstants,
	sqliteModule,
};
