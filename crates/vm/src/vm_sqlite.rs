//! VM-scoped SQLite substrate shared by VFS and agentOS Core state.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use rusqlite::types::{Value, ValueRef};
use thiserror::Error;

use agentos_vm_config::{
    VmSqliteCallbackRequest, VmSqliteCallbackResponse, VmSqliteDescriptor, VmSqliteQueryResult,
    VmSqliteStatement, VmSqliteValue, VM_SQLITE_CALLBACK_NAMESPACE,
};

use crate::protocol::{ExtEnvelope, OwnershipScope, SidecarRequestPayload, SidecarResponsePayload};
use crate::state::SharedSidecarRequestClient;

const LOCAL_SQLITE_JOB_BYTES: usize = 64 * 1024;
const HOST_CALLBACK_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    SqlNull,
    SqlInteger(i64),
    SqlReal(f64),
    SqlText(String),
    SqlBlob(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<SqlValue>>,
    pub changes: i64,
    pub last_insert_row_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct SqlStatement {
    pub sql: String,
    pub params: Vec<SqlValue>,
    expected_changes: Option<i64>,
}

impl SqlStatement {
    pub fn new(sql: impl Into<String>, params: Vec<SqlValue>) -> Self {
        Self {
            sql: sql.into(),
            params,
            expected_changes: None,
        }
    }

    pub fn plain(sql: impl Into<String>) -> Self {
        Self::new(sql, Vec::new())
    }

    /// Require this statement to affect exactly `expected` rows. Transaction
    /// backends evaluate this before COMMIT so a failed compare-and-set rolls
    /// the complete operation back.
    pub fn expect_changes(mut self, expected: i64) -> Self {
        self.expected_changes = Some(expected);
        self
    }
}

#[derive(Debug, Error)]
pub enum VmSqliteError {
    #[error("local SQLite failed: {0}")]
    Local(#[from] rusqlite::Error),
    #[error("local SQLite blocking executor failed: {0}")]
    Blocking(#[from] agentos_driver_tokio::BlockingJobError),
    #[error("invalid SQLite result: {0}")]
    InvalidResult(String),
    #[error(
        "sqlite_result_limit: SQLite result used {used} bytes, limit {limit}; raise limits.sqlite.maxResultBytes"
    )]
    ResultTooLarge { used: usize, limit: usize },
    #[error("SQLite backend {backend} did not enable PRAGMA foreign_keys")]
    ForeignKeysDisabled { backend: &'static str },
    #[error("SQLite compare-and-set affected {actual} rows; expected {expected}")]
    UnexpectedChanges { expected: i64, actual: i64 },
    #[error("SQLite database is closed")]
    Closed,
    #[error("Rivet SQLite callback failed: {0}")]
    Callback(String),
    #[error(
        "schema component {component} is at future version {found}; sidecar supports {supported}"
    )]
    FutureSchema {
        component: String,
        found: i64,
        supported: i64,
    },
    #[error("schema migration ladder for {component} skipped version {expected}")]
    InvalidMigrationLadder { component: String, expected: i64 },
}

#[async_trait]
pub trait VmSqliteDatabase: Send + Sync {
    async fn query(&self, statement: SqlStatement) -> Result<QueryResult, VmSqliteError>;

    /// Execute all statements atomically and return their results in order.
    async fn transaction(
        &self,
        statements: Vec<SqlStatement>,
    ) -> Result<Vec<QueryResult>, VmSqliteError>;

    /// Close the backing database. Closing an already closed database is safe;
    /// all later queries and transactions fail with [`VmSqliteError::Closed`].
    async fn close(&self) -> Result<(), VmSqliteError>;
}

pub type SharedVmSqliteDatabase = Arc<dyn VmSqliteDatabase>;

/// Open a trusted local SQLite database for embedded VM storage.
pub(crate) async fn open_local_vm_sqlite(
    path: PathBuf,
    runtime: agentos_driver_tokio::DriverHandle,
    max_result_bytes: usize,
) -> Result<SharedVmSqliteDatabase, VmSqliteError> {
    Ok(Arc::new(
        LocalVmSqliteDatabase::open(path, runtime, max_result_bytes).await?,
    ))
}

pub(crate) async fn resolve_vm_sqlite(
    descriptor: &VmSqliteDescriptor,
    runtime: agentos_driver_tokio::DriverHandle,
    max_result_bytes: usize,
    host: Option<(SharedSidecarRequestClient, OwnershipScope)>,
) -> Result<SharedVmSqliteDatabase, VmSqliteError> {
    match descriptor {
        VmSqliteDescriptor::SqliteFile { path } => {
            open_local_vm_sqlite(PathBuf::from(path), runtime, max_result_bytes).await
        }
        VmSqliteDescriptor::HostCallback { namespace } => {
            let (requests, ownership) = host.ok_or_else(|| {
                VmSqliteError::Callback(String::from(
                    "sidecar request transport is unavailable for host_callback",
                ))
            })?;
            Ok(Arc::new(HostCallbackVmSqliteDatabase {
                database: namespace.clone(),
                requests,
                ownership,
                max_result_bytes,
                closed: AtomicBool::new(false),
            }))
        }
    }
}

struct HostCallbackVmSqliteDatabase {
    database: String,
    requests: SharedSidecarRequestClient,
    ownership: OwnershipScope,
    max_result_bytes: usize,
    closed: AtomicBool,
}

impl HostCallbackVmSqliteDatabase {
    async fn invoke(
        &self,
        request: VmSqliteCallbackRequest,
    ) -> Result<VmSqliteCallbackResponse, VmSqliteError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(VmSqliteError::Closed);
        }
        let payload = serde_json::to_vec(&request)
            .map_err(|error| VmSqliteError::Callback(format!("encode request: {error}")))?;
        let response = self
            .requests
            .invoke_async(
                self.ownership.clone(),
                SidecarRequestPayload::Ext(ExtEnvelope {
                    namespace: VM_SQLITE_CALLBACK_NAMESPACE.to_owned(),
                    payload,
                }),
                HOST_CALLBACK_TIMEOUT,
            )
            .await
            .map_err(|error| VmSqliteError::Callback(error.to_string()))?;
        let SidecarResponsePayload::ExtResult(envelope) = response else {
            return Err(VmSqliteError::Callback(String::from(
                "host returned the wrong callback response type",
            )));
        };
        if envelope.namespace != VM_SQLITE_CALLBACK_NAMESPACE {
            return Err(VmSqliteError::Callback(format!(
                "host returned callback namespace {:?}",
                envelope.namespace
            )));
        }
        serde_json::from_slice(&envelope.payload)
            .map_err(|error| VmSqliteError::Callback(format!("decode response: {error}")))
    }
}

#[async_trait]
impl VmSqliteDatabase for HostCallbackVmSqliteDatabase {
    async fn query(&self, statement: SqlStatement) -> Result<QueryResult, VmSqliteError> {
        let expected_changes = statement.expected_changes;
        let response = self
            .invoke(VmSqliteCallbackRequest::Query {
                database: self.database.clone(),
                statement: statement.into(),
            })
            .await?;
        let VmSqliteCallbackResponse::Query { result } = response else {
            return Err(callback_response_error(response, "query"));
        };
        let result = QueryResult::from(result);
        validate_query_result_size(&result, self.max_result_bytes)?;
        validate_expected_changes(expected_changes, &result)?;
        Ok(result)
    }

    async fn transaction(
        &self,
        statements: Vec<SqlStatement>,
    ) -> Result<Vec<QueryResult>, VmSqliteError> {
        let expected_changes = statements
            .iter()
            .map(|statement| statement.expected_changes)
            .collect::<Vec<_>>();
        let response = self
            .invoke(VmSqliteCallbackRequest::Transaction {
                database: self.database.clone(),
                statements: statements.into_iter().map(Into::into).collect(),
            })
            .await?;
        let VmSqliteCallbackResponse::Transaction { results } = response else {
            return Err(callback_response_error(response, "transaction"));
        };
        if results.len() != expected_changes.len() {
            return Err(VmSqliteError::InvalidResult(format!(
                "Rivet SQLite transaction returned {} results for {} statements",
                results.len(),
                expected_changes.len()
            )));
        }
        let results = results
            .into_iter()
            .map(QueryResult::from)
            .collect::<Vec<_>>();
        for (expected, result) in expected_changes.into_iter().zip(&results) {
            validate_query_result_size(result, self.max_result_bytes)?;
            validate_expected_changes(expected, result)?;
        }
        Ok(results)
    }

    async fn close(&self) -> Result<(), VmSqliteError> {
        if self.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let payload = serde_json::to_vec(&VmSqliteCallbackRequest::Close {
            database: self.database.clone(),
        })
        .map_err(|error| VmSqliteError::Callback(format!("encode close request: {error}")))?;
        let response = self
            .requests
            .invoke_async(
                self.ownership.clone(),
                SidecarRequestPayload::Ext(ExtEnvelope {
                    namespace: VM_SQLITE_CALLBACK_NAMESPACE.to_owned(),
                    payload,
                }),
                HOST_CALLBACK_TIMEOUT,
            )
            .await
            .map_err(|error| VmSqliteError::Callback(error.to_string()))?;
        let SidecarResponsePayload::ExtResult(envelope) = response else {
            return Err(VmSqliteError::Callback(String::from(
                "host returned the wrong close response type",
            )));
        };
        let response: VmSqliteCallbackResponse = serde_json::from_slice(&envelope.payload)
            .map_err(|error| VmSqliteError::Callback(format!("decode close response: {error}")))?;
        match response {
            VmSqliteCallbackResponse::Closed => Ok(()),
            other => Err(callback_response_error(other, "close")),
        }
    }
}

fn callback_response_error(response: VmSqliteCallbackResponse, operation: &str) -> VmSqliteError {
    match response {
        VmSqliteCallbackResponse::Error { message } => VmSqliteError::Callback(message),
        other => VmSqliteError::InvalidResult(format!(
            "Rivet SQLite {operation} returned unexpected response {other:?}"
        )),
    }
}

fn validate_query_result_size(result: &QueryResult, limit: usize) -> Result<(), VmSqliteError> {
    let used = result
        .columns
        .iter()
        .map(String::len)
        .chain(result.rows.iter().flatten().map(sql_value_bytes))
        .try_fold(0usize, |total, bytes| total.checked_add(bytes))
        .unwrap_or(usize::MAX);
    if used > limit {
        return Err(VmSqliteError::ResultTooLarge { used, limit });
    }
    Ok(())
}

impl From<SqlStatement> for VmSqliteStatement {
    fn from(statement: SqlStatement) -> Self {
        Self {
            sql: statement.sql,
            params: statement.params.into_iter().map(Into::into).collect(),
            expected_changes: statement.expected_changes,
        }
    }
}

impl From<SqlValue> for VmSqliteValue {
    fn from(value: SqlValue) -> Self {
        match value {
            SqlValue::SqlNull => Self::Null,
            SqlValue::SqlInteger(value) => Self::Integer(value),
            SqlValue::SqlReal(value) => Self::Real(value),
            SqlValue::SqlText(value) => Self::Text(value),
            SqlValue::SqlBlob(value) => Self::Blob(value),
        }
    }
}

impl From<VmSqliteValue> for SqlValue {
    fn from(value: VmSqliteValue) -> Self {
        match value {
            VmSqliteValue::Null => Self::SqlNull,
            VmSqliteValue::Integer(value) => Self::SqlInteger(value),
            VmSqliteValue::Real(value) => Self::SqlReal(value),
            VmSqliteValue::Text(value) => Self::SqlText(value),
            VmSqliteValue::Blob(value) => Self::SqlBlob(value),
        }
    }
}

impl From<VmSqliteQueryResult> for QueryResult {
    fn from(result: VmSqliteQueryResult) -> Self {
        Self {
            columns: result.columns,
            rows: result
                .rows
                .into_iter()
                .map(|row| row.into_iter().map(Into::into).collect())
                .collect(),
            changes: result.changes,
            last_insert_row_id: result.last_insert_row_id,
        }
    }
}

struct LocalVmSqliteDatabase {
    connection: Arc<Mutex<Option<rusqlite::Connection>>>,
    runtime: agentos_driver_tokio::DriverHandle,
    max_result_bytes: usize,
}

impl LocalVmSqliteDatabase {
    async fn open(
        path: PathBuf,
        runtime: agentos_driver_tokio::DriverHandle,
        max_result_bytes: usize,
    ) -> Result<Self, VmSqliteError> {
        let connection = runtime
            .blocking()
            .run(LOCAL_SQLITE_JOB_BYTES, move || {
                let connection = rusqlite::Connection::open(path)?;
                connection.busy_timeout(Duration::from_secs(5))?;
                connection.execute_batch("PRAGMA foreign_keys = ON;")?;
                let enabled: i64 =
                    connection.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
                if enabled != 1 {
                    return Err(VmSqliteError::ForeignKeysDisabled {
                        backend: "sqlite_file",
                    });
                }
                Ok::<_, VmSqliteError>(connection)
            })
            .await??;
        Ok(Self {
            connection: Arc::new(Mutex::new(Some(connection))),
            runtime,
            max_result_bytes,
        })
    }

    async fn run<T>(
        &self,
        operation: impl FnOnce(&mut rusqlite::Connection) -> Result<T, VmSqliteError> + Send + 'static,
    ) -> Result<T, VmSqliteError>
    where
        T: Send + 'static,
    {
        let connection = Arc::clone(&self.connection);
        self.runtime
            .blocking()
            .run(LOCAL_SQLITE_JOB_BYTES, move || {
                let mut connection = connection.lock().map_err(|_| {
                    VmSqliteError::InvalidResult("local SQLite mutex poisoned".to_owned())
                })?;
                let connection = connection.as_mut().ok_or(VmSqliteError::Closed)?;
                operation(connection)
            })
            .await?
    }
}

#[async_trait]
impl VmSqliteDatabase for LocalVmSqliteDatabase {
    async fn query(&self, statement: SqlStatement) -> Result<QueryResult, VmSqliteError> {
        if statement.expected_changes.is_some() {
            return self
                .transaction(vec![statement])
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| {
                    VmSqliteError::InvalidResult(
                        "single-statement transaction returned no result".to_owned(),
                    )
                });
        }
        let max_result_bytes = self.max_result_bytes;
        self.run(move |connection| {
            execute_local_statement(connection, &statement, max_result_bytes)
        })
        .await
    }

    async fn transaction(
        &self,
        statements: Vec<SqlStatement>,
    ) -> Result<Vec<QueryResult>, VmSqliteError> {
        let max_result_bytes = self.max_result_bytes;
        self.run(move |connection| {
            connection.execute_batch("BEGIN IMMEDIATE")?;
            let mut results = Vec::with_capacity(statements.len());
            for statement in &statements {
                match execute_local_statement(connection, statement, max_result_bytes) {
                    Ok(result) => {
                        if let Err(error) =
                            validate_expected_changes(statement.expected_changes, &result)
                        {
                            if let Err(rollback_error) = connection.execute_batch("ROLLBACK") {
                                eprintln!(
                                    "ERR_AGENTOS_SQLITE_ROLLBACK: local transaction rollback failed after {error}: {rollback_error}"
                                );
                            }
                            return Err(error);
                        }
                        results.push(result)
                    }
                    Err(error) => {
                        if let Err(rollback_error) = connection.execute_batch("ROLLBACK") {
                            eprintln!(
                                "ERR_AGENTOS_SQLITE_ROLLBACK: local transaction rollback failed after {error}: {rollback_error}"
                            );
                        }
                        return Err(error);
                    }
                }
            }
            if let Err(error) = connection.execute_batch("COMMIT") {
                if let Err(rollback_error) = connection.execute_batch("ROLLBACK") {
                    eprintln!(
                        "ERR_AGENTOS_SQLITE_ROLLBACK: local rollback failed after commit error {error}: {rollback_error}"
                    );
                }
                return Err(error.into());
            }
            Ok(results)
        })
        .await
    }

    async fn close(&self) -> Result<(), VmSqliteError> {
        let connection = Arc::clone(&self.connection);
        self.runtime
            .blocking()
            .run(LOCAL_SQLITE_JOB_BYTES, move || {
                let mut guard = connection.lock().map_err(|_| {
                    VmSqliteError::InvalidResult("local SQLite mutex poisoned".to_owned())
                })?;
                let Some(database) = guard.take() else {
                    return Ok(());
                };
                match database.close() {
                    Ok(()) => Ok(()),
                    Err((database, error)) => {
                        *guard = Some(database);
                        Err(VmSqliteError::Local(error))
                    }
                }
            })
            .await?
    }
}

fn validate_expected_changes(
    expected_changes: Option<i64>,
    result: &QueryResult,
) -> Result<(), VmSqliteError> {
    if let Some(expected) = expected_changes {
        if result.changes != expected {
            return Err(VmSqliteError::UnexpectedChanges {
                expected,
                actual: result.changes,
            });
        }
    }
    Ok(())
}

fn execute_local_statement(
    connection: &mut rusqlite::Connection,
    statement: &SqlStatement,
    max_result_bytes: usize,
) -> Result<QueryResult, VmSqliteError> {
    let values = statement
        .params
        .iter()
        .map(sql_value_to_local)
        .collect::<Result<Vec<_>, _>>()?;
    let mut prepared = connection.prepare(&statement.sql)?;
    let readonly = prepared.readonly();
    let columns = prepared
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if columns.is_empty() {
        let changes = prepared.execute(rusqlite::params_from_iter(values.iter()))?;
        return Ok(QueryResult {
            columns,
            rows: Vec::new(),
            changes: i64::try_from(changes).unwrap_or(i64::MAX),
            last_insert_row_id: Some(connection.last_insert_rowid()),
        });
    }

    let column_count = columns.len();
    let mut cursor = prepared.query(rusqlite::params_from_iter(values.iter()))?;
    let mut rows = Vec::new();
    let mut result_bytes = columns.iter().map(String::len).sum::<usize>();
    let mut warned = false;
    while let Some(row) = cursor.next()? {
        let mut output = Vec::with_capacity(column_count);
        for index in 0..column_count {
            let value = local_value_to_sql(row.get_ref(index)?)?;
            result_bytes = result_bytes.checked_add(sql_value_bytes(&value)).ok_or(
                VmSqliteError::ResultTooLarge {
                    used: usize::MAX,
                    limit: max_result_bytes,
                },
            )?;
            if result_bytes > max_result_bytes {
                return Err(VmSqliteError::ResultTooLarge {
                    used: result_bytes,
                    limit: max_result_bytes,
                });
            }
            if !warned && result_bytes >= max_result_bytes.saturating_mul(4) / 5 {
                tracing::warn!(
                    used = result_bytes,
                    limit = max_result_bytes,
                    config_path = "limits.sqlite.maxResultBytes",
                    "SQLite result materialization is near its configured limit"
                );
                warned = true;
            }
            output.push(value);
        }
        rows.push(output);
    }
    drop(cursor);
    drop(prepared);
    Ok(QueryResult {
        columns,
        rows,
        changes: if readonly {
            0
        } else {
            i64::try_from(connection.changes()).unwrap_or(i64::MAX)
        },
        last_insert_row_id: (!readonly).then(|| connection.last_insert_rowid()),
    })
}

fn sql_value_bytes(value: &SqlValue) -> usize {
    match value {
        SqlValue::SqlNull => 0,
        SqlValue::SqlInteger(_) | SqlValue::SqlReal(_) => 8,
        SqlValue::SqlText(value) => value.len(),
        SqlValue::SqlBlob(value) => value.len(),
    }
}

fn sql_value_to_local(value: &SqlValue) -> Result<Value, VmSqliteError> {
    Ok(match value {
        SqlValue::SqlNull => Value::Null,
        SqlValue::SqlInteger(value) => Value::Integer(*value),
        SqlValue::SqlReal(value) if value.is_finite() => Value::Real(*value),
        SqlValue::SqlReal(_) => {
            return Err(VmSqliteError::InvalidResult(
                "SQLite real parameters must be finite".to_owned(),
            ));
        }
        SqlValue::SqlText(value) => Value::Text(value.clone()),
        SqlValue::SqlBlob(value) => Value::Blob(value.clone()),
    })
}

fn local_value_to_sql(value: ValueRef<'_>) -> Result<SqlValue, VmSqliteError> {
    Ok(match value {
        ValueRef::Null => SqlValue::SqlNull,
        ValueRef::Integer(value) => SqlValue::SqlInteger(value),
        ValueRef::Real(value) if value.is_finite() => SqlValue::SqlReal(value),
        ValueRef::Real(_) => {
            return Err(VmSqliteError::InvalidResult(
                "SQLite returned a non-finite real".to_owned(),
            ));
        }
        ValueRef::Text(value) => SqlValue::SqlText(
            std::str::from_utf8(value)
                .map_err(|error| VmSqliteError::InvalidResult(error.to_string()))?
                .to_owned(),
        ),
        ValueRef::Blob(value) => SqlValue::SqlBlob(value.to_vec()),
    })
}

pub struct VmSqliteMigration {
    pub version: i64,
    pub statements: &'static [&'static str],
}

fn validate_migration_namespace(owner: &str, sql: &str) -> Result<(), VmSqliteError> {
    let expected_prefix = match owner {
        "filesystem" => "agentos_fs_",
        "core" => "agentos_core_",
        "actor" => "agentos_actor_",
        _ => unreachable!("owner was validated before migration SQL"),
    };
    for identifier in sql
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|identifier| !identifier.is_empty())
    {
        let identifier = identifier.to_ascii_lowercase();
        if identifier.starts_with("agentos_") && !identifier.starts_with(expected_prefix) {
            return Err(VmSqliteError::InvalidResult(format!(
                "SQLite {owner} migration referenced schema object {identifier}; expected prefix {expected_prefix}"
            )));
        }
    }
    Ok(())
}

pub async fn migrate_schema(
    database: &dyn VmSqliteDatabase,
    owner: &str,
    version_table: &str,
    migrations: &[VmSqliteMigration],
) -> Result<(), VmSqliteError> {
    let expected_version_table = match owner {
        "filesystem" => "agentos_fs_schema_version",
        "core" => "agentos_core_schema_version",
        "actor" => "agentos_actor_schema_version",
        _ => {
            return Err(VmSqliteError::InvalidResult(format!(
                "unknown AgentOS SQLite schema owner {owner:?}"
            )));
        }
    };
    if version_table != expected_version_table {
        return Err(VmSqliteError::InvalidResult(format!(
            "SQLite schema owner {owner} must use {expected_version_table}, not {version_table}"
        )));
    }
    if version_table.is_empty()
        || !version_table
            .bytes()
            .all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
    {
        return Err(VmSqliteError::InvalidResult(format!(
            "invalid schema version table {version_table:?}"
        )));
    }
    for (index, migration) in migrations.iter().enumerate() {
        let expected = i64::try_from(index).unwrap_or(i64::MAX) + 1;
        if migration.version != expected {
            return Err(VmSqliteError::InvalidMigrationLadder {
                component: owner.to_owned(),
                expected,
            });
        }
        for statement in migration.statements {
            validate_migration_namespace(owner, statement)?;
        }
    }
    database
        .query(SqlStatement::plain(format!(
            "CREATE TABLE IF NOT EXISTS {version_table} (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), schema_version INTEGER NOT NULL CHECK (schema_version >= 0)) STRICT"
        )))
        .await?;
    let result = database
        .query(SqlStatement::plain(format!(
            "SELECT schema_version FROM {version_table} WHERE singleton = 1"
        )))
        .await?;
    let current = match result.rows.first().and_then(|row| row.first()) {
        None => 0,
        Some(SqlValue::SqlInteger(version)) => *version,
        Some(value) => {
            return Err(VmSqliteError::InvalidResult(format!(
                "schema version for {component} was not an integer: {value:?}",
                component = owner
            )));
        }
    };
    let supported = migrations
        .last()
        .map(|migration| migration.version)
        .unwrap_or(0);
    if current > supported {
        return Err(VmSqliteError::FutureSchema {
            component: owner.to_owned(),
            found: current,
            supported,
        });
    }
    for migration in migrations
        .iter()
        .filter(|migration| migration.version > current)
    {
        let mut statements = migration
            .statements
            .iter()
            .map(|sql| SqlStatement::plain(*sql))
            .collect::<Vec<_>>();
        statements.push(SqlStatement::new(
            format!("INSERT INTO {version_table} (singleton, schema_version) VALUES (1, ?) ON CONFLICT(singleton) DO UPDATE SET schema_version = excluded.schema_version"),
            vec![SqlValue::SqlInteger(migration.version)],
        ));
        database.transaction(statements).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> &'static agentos_driver_tokio::TokioDriver {
        agentos_driver_tokio::TokioDriver::process(&agentos_driver_tokio::DriverConfig::default())
            .expect("runtime")
    }

    #[test]
    fn local_query_cas_rolls_back_and_returning_preserves_write_metadata() {
        let runtime = runtime();
        runtime.block_on(async {
            let directory = tempfile::tempdir().unwrap();
            let database =
                open_local_vm_sqlite(directory.path().join("cas.sqlite"), runtime.handle(), 1024)
                    .await
                    .unwrap();
            database
                .query(SqlStatement::plain(
                    "CREATE TABLE test_cas (id INTEGER PRIMARY KEY, value INTEGER NOT NULL) STRICT",
                ))
                .await
                .unwrap();
            let inserted = database
                .query(
                    SqlStatement::plain("INSERT INTO test_cas VALUES (1, 10) RETURNING id")
                        .expect_changes(1),
                )
                .await
                .unwrap();
            assert_eq!(inserted.rows, vec![vec![SqlValue::SqlInteger(1)]]);
            assert_eq!(inserted.changes, 1);
            assert_eq!(inserted.last_insert_row_id, Some(1));
            for statement in [
                "INSERT INTO test_cas VALUES (2, 20)",
                "UPDATE test_cas SET value = 30 RETURNING value",
            ] {
                let error = database
                    .query(SqlStatement::plain(statement).expect_changes(0))
                    .await
                    .unwrap_err();
                assert!(matches!(
                    error,
                    VmSqliteError::UnexpectedChanges {
                        expected: 0,
                        actual: 1
                    }
                ));
            }
            let result = database
                .query(SqlStatement::plain("SELECT id, value FROM test_cas"))
                .await
                .unwrap();
            assert_eq!(
                result.rows,
                vec![vec![SqlValue::SqlInteger(1), SqlValue::SqlInteger(10)]]
            );
            assert_eq!(
                result.changes, 0,
                "SELECT must not reuse the prior write count"
            );
            assert_eq!(result.last_insert_row_id, None);
            database.close().await.unwrap();
        });
    }

    #[test]
    fn local_transactions_commit_and_roll_back() {
        let runtime = runtime();
        let context = runtime.handle();
        runtime.block_on(async move {
            let dir = tempfile::tempdir().expect("tempdir");
            let database = resolve_vm_sqlite(
                &VmSqliteDescriptor::SqliteFile {
                    path: dir.path().join("state.sqlite").display().to_string(),
                },
                context,
                crate::core::limits::DEFAULT_SQLITE_MAX_RESULT_BYTES,
                None,
            )
            .await
            .expect("database");
            database
                .transaction(vec![
                    SqlStatement::plain("CREATE TABLE values_table (value INTEGER NOT NULL)"),
                    SqlStatement::plain("INSERT INTO values_table VALUES (1)"),
                ])
                .await
                .expect("commit");
            let failed = database
                .transaction(vec![
                    SqlStatement::plain("INSERT INTO values_table VALUES (2)"),
                    SqlStatement::plain("INSERT INTO missing_table VALUES (3)"),
                ])
                .await;
            assert!(failed.is_err());
            let raced = database
                .transaction(vec![
                    SqlStatement::plain("INSERT INTO values_table VALUES (2)"),
                    SqlStatement::plain("UPDATE values_table SET value = 3 WHERE value = 99")
                        .expect_changes(1),
                ])
                .await;
            assert!(matches!(
                raced,
                Err(VmSqliteError::UnexpectedChanges {
                    expected: 1,
                    actual: 0,
                })
            ));
            let result = database
                .query(SqlStatement::plain("SELECT value FROM values_table"))
                .await
                .expect("query");
            assert_eq!(result.rows, vec![vec![SqlValue::SqlInteger(1)]]);

            database.close().await.expect("close");
            database.close().await.expect("idempotent close");
            assert!(matches!(
                database
                    .query(SqlStatement::plain("SELECT value FROM values_table"))
                    .await,
                Err(VmSqliteError::Closed)
            ));
        });
    }

    #[test]
    fn local_migrations_keep_strict_owner_versions_independent() {
        const FS_MIGRATIONS: &[VmSqliteMigration] = &[
            VmSqliteMigration {
                version: 1,
                statements: &["CREATE TABLE agentos_fs_probe (value INTEGER NOT NULL) STRICT"],
            },
            VmSqliteMigration {
                version: 2,
                statements: &["CREATE TABLE agentos_fs_probe_v2 (value TEXT NOT NULL) STRICT"],
            },
        ];
        const CORE_MIGRATIONS: &[VmSqliteMigration] = &[VmSqliteMigration {
            version: 1,
            statements: &["CREATE TABLE agentos_core_probe (value BLOB NOT NULL) STRICT"],
        }];

        let runtime = runtime();
        let context = runtime.handle();
        runtime.block_on(async move {
            let dir = tempfile::tempdir().expect("tempdir");
            let database = resolve_vm_sqlite(
                &VmSqliteDescriptor::SqliteFile {
                    path: dir.path().join("owner-versions.sqlite").display().to_string(),
                },
                context,
                crate::core::limits::DEFAULT_SQLITE_MAX_RESULT_BYTES,
                None,
            )
            .await
            .expect("database");

            let crossed_owner = migrate_schema(
                database.as_ref(),
                "core",
                "agentos_fs_schema_version",
                CORE_MIGRATIONS,
            )
            .await;
            assert!(matches!(crossed_owner, Err(VmSqliteError::InvalidResult(message)) if message.contains("must use agentos_core_schema_version")));

            const CROSSED_MIGRATION: &[VmSqliteMigration] = &[VmSqliteMigration {
                version: 1,
                statements: &["CREATE TABLE agentos_actor_forbidden (value INTEGER) STRICT"],
            }];
            let crossed_statement = migrate_schema(
                database.as_ref(),
                "core",
                "agentos_core_schema_version",
                CROSSED_MIGRATION,
            )
            .await;
            assert!(matches!(crossed_statement, Err(VmSqliteError::InvalidResult(message)) if message.contains("agentos_actor_forbidden")));
            let owner_side_effects = database
                .query(SqlStatement::plain(
                    "SELECT COUNT(*) FROM sqlite_schema WHERE name LIKE 'agentos_%_schema_version'",
                ))
                .await
                .expect("owner isolation side effects");
            assert_eq!(
                owner_side_effects.rows,
                vec![vec![SqlValue::SqlInteger(0)]]
            );

            migrate_schema(
                database.as_ref(),
                "core",
                "agentos_core_schema_version",
                CORE_MIGRATIONS,
            )
            .await
            .expect("core migration");
            migrate_schema(
                database.as_ref(),
                "filesystem",
                "agentos_fs_schema_version",
                FS_MIGRATIONS,
            )
            .await
            .expect("filesystem migration");

            let versions = database
                .query(SqlStatement::plain(
                    "SELECT (SELECT schema_version FROM agentos_fs_schema_version WHERE singleton = 1), (SELECT schema_version FROM agentos_core_schema_version WHERE singleton = 1), (SELECT COUNT(*) FROM sqlite_schema WHERE name = 'agentos_schema_versions')",
                ))
                .await
                .expect("owner versions");
            assert_eq!(
                versions.rows,
                vec![vec![
                    SqlValue::SqlInteger(2),
                    SqlValue::SqlInteger(1),
                    SqlValue::SqlInteger(0),
                ]]
            );

            let schemas = database
                .query(SqlStatement::plain(
                    "SELECT name, sql FROM sqlite_schema WHERE name IN ('agentos_fs_schema_version', 'agentos_fs_probe', 'agentos_fs_probe_v2', 'agentos_core_schema_version', 'agentos_core_probe') ORDER BY name",
                ))
                .await
                .expect("strict schemas");
            assert_eq!(schemas.rows.len(), 5);
            for row in schemas.rows {
                let Some(SqlValue::SqlText(name)) = row.first() else {
                    panic!("schema row was missing its table name: {row:?}");
                };
                let Some(SqlValue::SqlText(sql)) = row.get(1) else {
                    panic!("schema row for {name} was missing SQL: {row:?}");
                };
                assert!(
                    sql.trim_end().ends_with("STRICT"),
                    "owner table {name} was not STRICT: {sql}"
                );
            }

            let invalid_fs_type = database
                .query(SqlStatement::plain(
                    "INSERT INTO agentos_fs_probe (value) VALUES ('text')",
                ))
                .await;
            assert!(invalid_fs_type.is_err(), "STRICT fs table accepted TEXT");
            let invalid_core_type = database
                .query(SqlStatement::plain(
                    "INSERT INTO agentos_core_probe (value) VALUES (1)",
                ))
                .await;
            assert!(
                invalid_core_type.is_err(),
                "STRICT core table accepted INTEGER"
            );
        });
    }

    #[test]
    fn migration_validation_rejects_malformed_and_future_versions_without_changes() {
        const MALFORMED_MIGRATIONS: &[VmSqliteMigration] = &[
            VmSqliteMigration {
                version: 1,
                statements: &["CREATE TABLE agentos_fs_never_created (value INTEGER) STRICT"],
            },
            VmSqliteMigration {
                version: 3,
                statements: &["CREATE TABLE agentos_fs_also_never_created (value INTEGER) STRICT"],
            },
        ];
        const SUPPORTED_MIGRATIONS: &[VmSqliteMigration] = &[VmSqliteMigration {
            version: 1,
            statements: &["CREATE TABLE agentos_core_never_created (value INTEGER) STRICT"],
        }];

        let runtime = runtime();
        let context = runtime.handle();
        runtime.block_on(async move {
            let dir = tempfile::tempdir().expect("tempdir");
            let database = resolve_vm_sqlite(
                &VmSqliteDescriptor::SqliteFile {
                    path: dir.path().join("invalid-versions.sqlite").display().to_string(),
                },
                context,
                crate::core::limits::DEFAULT_SQLITE_MAX_RESULT_BYTES,
                None,
            )
            .await
            .expect("database");

            let malformed = migrate_schema(
                database.as_ref(),
                "filesystem",
                "agentos_fs_schema_version",
                MALFORMED_MIGRATIONS,
            )
            .await;
            assert!(matches!(
                malformed,
                Err(VmSqliteError::InvalidMigrationLadder {
                    ref component,
                    expected: 2,
                }) if component == "filesystem"
            ));
            let malformed_changes = database
                .query(SqlStatement::plain(
                    "SELECT COUNT(*) FROM sqlite_schema WHERE name LIKE 'agentos_fs_%'",
                ))
                .await
                .expect("malformed ladder side effects");
            assert_eq!(
                malformed_changes.rows,
                vec![vec![SqlValue::SqlInteger(0)]]
            );

            database
                .transaction(vec![
                    SqlStatement::plain(
                        "CREATE TABLE agentos_core_schema_version (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), schema_version INTEGER NOT NULL CHECK (schema_version >= 0)) STRICT",
                    ),
                    SqlStatement::plain(
                        "INSERT INTO agentos_core_schema_version (singleton, schema_version) VALUES (1, 2)",
                    ),
                ])
                .await
                .expect("future version fixture");
            let future = migrate_schema(
                database.as_ref(),
                "core",
                "agentos_core_schema_version",
                SUPPORTED_MIGRATIONS,
            )
            .await;
            assert!(matches!(
                future,
                Err(VmSqliteError::FutureSchema {
                    ref component,
                    found: 2,
                    supported: 1,
                }) if component == "core"
            ));
            let future_state = database
                .query(SqlStatement::plain(
                    "SELECT (SELECT schema_version FROM agentos_core_schema_version WHERE singleton = 1), (SELECT COUNT(*) FROM sqlite_schema WHERE name = 'agentos_core_never_created')",
                ))
                .await
                .expect("future version state");
            assert_eq!(
                future_state.rows,
                vec![vec![SqlValue::SqlInteger(2), SqlValue::SqlInteger(0)]]
            );
        });
    }

    #[test]
    fn local_migration_rolls_back_schema_and_version_atomically_on_strict_failure() {
        const MIGRATIONS: &[VmSqliteMigration] = &[VmSqliteMigration {
            version: 1,
            statements: &[
                "CREATE TABLE agentos_fs_atomic_probe (value INTEGER NOT NULL) STRICT",
                "INSERT INTO agentos_fs_atomic_probe (value) VALUES ('not-an-integer')",
            ],
        }];

        let runtime = runtime();
        let context = runtime.handle();
        runtime.block_on(async move {
            let dir = tempfile::tempdir().expect("tempdir");
            let database = resolve_vm_sqlite(
                &VmSqliteDescriptor::SqliteFile {
                    path: dir.path().join("atomic-migration.sqlite").display().to_string(),
                },
                context,
                crate::core::limits::DEFAULT_SQLITE_MAX_RESULT_BYTES,
                None,
            )
            .await
            .expect("database");

            assert!(
                migrate_schema(
                    database.as_ref(),
                    "filesystem",
                    "agentos_fs_schema_version",
                    MIGRATIONS,
                )
                .await
                .is_err(),
                "STRICT type violation unexpectedly migrated"
            );
            let state = database
                .query(SqlStatement::plain(
                    "SELECT (SELECT COUNT(*) FROM sqlite_schema WHERE name = 'agentos_fs_atomic_probe'), (SELECT COUNT(*) FROM agentos_fs_schema_version)",
                ))
                .await
                .expect("atomic rollback state");
            assert_eq!(
                state.rows,
                vec![vec![SqlValue::SqlInteger(0), SqlValue::SqlInteger(0)]]
            );
        });
    }

    #[test]
    fn local_result_limit_rejects_queries_and_rolls_back_transactions() {
        let runtime = runtime();
        let context = runtime.handle();
        runtime.block_on(async move {
            let dir = tempfile::tempdir().expect("tempdir");
            let database = resolve_vm_sqlite(
                &VmSqliteDescriptor::SqliteFile {
                    path: dir.path().join("result-limit.sqlite").display().to_string(),
                },
                context,
                32,
                None,
            )
            .await
            .expect("database");
            database
                .transaction(vec![
                    SqlStatement::plain(
                        "CREATE TABLE values_table (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
                    ),
                    SqlStatement::new(
                        "INSERT INTO values_table (id, value) VALUES (?, ?)",
                        vec![SqlValue::SqlInteger(1), SqlValue::SqlText("x".repeat(64))],
                    ),
                ])
                .await
                .expect("setup");

            let oversized = database
                .query(SqlStatement::plain(
                    "SELECT value FROM values_table WHERE id = 1",
                ))
                .await;
            assert!(matches!(
                oversized,
                Err(VmSqliteError::ResultTooLarge { limit: 32, .. })
            ));

            let failed = database
                .transaction(vec![
                    SqlStatement::new(
                        "INSERT INTO values_table (id, value) VALUES (?, ?)",
                        vec![SqlValue::SqlInteger(2), SqlValue::SqlText("y".repeat(64))],
                    ),
                    SqlStatement::plain("SELECT value FROM values_table WHERE id = 2"),
                ])
                .await;
            assert!(matches!(
                failed,
                Err(VmSqliteError::ResultTooLarge { limit: 32, .. })
            ));
            let result = database
                .query(SqlStatement::plain("SELECT COUNT(*) FROM values_table"))
                .await
                .expect("count after rollback");
            assert_eq!(result.rows, vec![vec![SqlValue::SqlInteger(1)]]);
        });
    }
}
