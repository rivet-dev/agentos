use std::sync::Arc;
use std::time::Duration;

use agentos_client::{
    SidecarSqliteCallback, VmSqliteCallbackRequest, VmSqliteCallbackResponse, VmSqliteQueryResult,
    VmSqliteStatement, VmSqliteValue,
};
use anyhow::{anyhow, bail, Context, Result};
use rivetkit::{Actor, BindParam, ColumnValue, Ctx};

use crate::{AgentOsActor, ConfigSnapshot};

const SCHEMA_VERSION: i64 = 3;
const TRANSACTION_TIMEOUT: Duration = Duration::from_secs(5);
const PREVIEW_CLEANUP_BATCH: i64 = 128;
const ALLOCATE_VM_GENERATION_SQL: &str = "UPDATE agentos_actor_vm_generation
     SET generation = generation + 1
     WHERE id = 1 AND generation < 9223372036854775807
     RETURNING generation";

pub(crate) fn rivet_sqlite_callback(
    ctx: Ctx<AgentOsActor>,
    database: String,
) -> SidecarSqliteCallback {
    Arc::new(move |request| {
        let ctx = ctx.clone();
        let database = database.clone();
        Box::pin(async move {
            match handle_rivet_sqlite_request(&ctx, &database, request).await {
                Ok(response) => Ok(response),
                Err(error) => Err(format!("{error:#}")),
            }
        })
    })
}

async fn handle_rivet_sqlite_request<A: Actor>(
    ctx: &Ctx<A>,
    expected_database: &str,
    request: VmSqliteCallbackRequest,
) -> Result<VmSqliteCallbackResponse> {
    match request {
        VmSqliteCallbackRequest::Query {
            database,
            statement,
        } => {
            require_callback_database(expected_database, &database)?;
            let result = if statement.expected_changes.is_some() {
                // A failed compare-and-set must roll back the statement,
                // including when the caller did not send an explicit batch.
                execute_rivet_transaction(ctx, vec![statement])
                    .await?
                    .into_iter()
                    .next()
                    .ok_or_else(|| {
                        anyhow!("single-statement SQLite transaction returned no result")
                    })?
            } else {
                let result = ctx
                    .sql()
                    .execute(statement.sql, Some(sqlite_bindings(statement.params)))
                    .await
                    .context("execute Rivet SQLite statement")?;
                sqlite_result(
                    result.columns,
                    result.rows,
                    result.changes,
                    result.last_insert_row_id,
                )
            };
            Ok(VmSqliteCallbackResponse::Query { result })
        }
        VmSqliteCallbackRequest::Transaction {
            database,
            statements,
        } => {
            require_callback_database(expected_database, &database)?;
            Ok(VmSqliteCallbackResponse::Transaction {
                results: execute_rivet_transaction(ctx, statements).await?,
            })
        }
        VmSqliteCallbackRequest::Close { database } => {
            require_callback_database(expected_database, &database)?;
            // Rivet owns the actor database lifetime. Closing one VM adapter
            // releases only the sidecar-side handle.
            Ok(VmSqliteCallbackResponse::Closed)
        }
    }
}

async fn execute_rivet_transaction<A: Actor>(
    ctx: &Ctx<A>,
    statements: Vec<VmSqliteStatement>,
) -> Result<Vec<VmSqliteQueryResult>> {
    let transaction = ctx
        .sql()
        .begin_named_transaction(Some("agentos-vm"), Some(TRANSACTION_TIMEOUT))
        .await
        .context("begin Rivet SQLite VM transaction")?;
    let result = async {
        let mut results = Vec::with_capacity(statements.len());
        for statement in statements {
            let expected_changes = statement.expected_changes;
            let result = transaction
                .execute(statement.sql, Some(sqlite_bindings(statement.params)))
                .await
                .context("execute Rivet SQLite transaction statement")?;
            require_expected_changes(expected_changes, result.changes)?;
            results.push(sqlite_result(
                result.columns,
                result.rows,
                result.changes,
                result.last_insert_row_id,
            ));
        }
        transaction
            .commit()
            .await
            .context("commit Rivet SQLite VM transaction")?;
        Result::<_>::Ok(results)
    }
    .await;
    match result {
        Ok(results) => Ok(results),
        Err(error) => Err(preserve_rollback_error(error, transaction.rollback().await)),
    }
}

fn preserve_rollback_error(error: anyhow::Error, rollback: Result<()>) -> anyhow::Error {
    match rollback {
        Ok(()) => error,
        Err(rollback_error) => error.context(format!(
            "rollback Rivet SQLite VM transaction failed: {rollback_error:#}"
        )),
    }
}

fn require_callback_database(expected: &str, actual: &str) -> Result<()> {
    if expected != actual {
        bail!("invalid SQLite callback database {actual:?}; expected {expected:?}");
    }
    Ok(())
}

fn require_expected_changes(expected: Option<i64>, actual: i64) -> Result<()> {
    if expected.is_some_and(|expected| expected != actual) {
        bail!(
            "SQLite compare-and-set affected {actual} rows; expected {}",
            expected.expect("checked above")
        );
    }
    Ok(())
}

fn sqlite_bindings(values: Vec<VmSqliteValue>) -> Vec<BindParam> {
    values
        .into_iter()
        .map(|value| match value {
            VmSqliteValue::Null => BindParam::Null,
            VmSqliteValue::Integer(value) => BindParam::Integer(value),
            VmSqliteValue::Real(value) => BindParam::Float(value),
            VmSqliteValue::Text(value) => BindParam::Text(value),
            VmSqliteValue::Blob(value) => BindParam::Blob(value),
        })
        .collect()
}

fn sqlite_result(
    columns: Vec<String>,
    rows: Vec<Vec<ColumnValue>>,
    changes: i64,
    last_insert_row_id: Option<i64>,
) -> VmSqliteQueryResult {
    VmSqliteQueryResult {
        columns,
        rows: rows
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|value| match value {
                        ColumnValue::Null => VmSqliteValue::Null,
                        ColumnValue::Integer(value) => VmSqliteValue::Integer(value),
                        ColumnValue::Float(value) => VmSqliteValue::Real(value),
                        ColumnValue::Text(value) => VmSqliteValue::Text(value),
                        ColumnValue::Blob(value) => VmSqliteValue::Blob(value),
                    })
                    .collect()
            })
            .collect(),
        changes,
        last_insert_row_id,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreviewLease {
    pub(crate) token: String,
    pub(crate) port: u16,
    pub(crate) expires_at_ms: i64,
}

pub(crate) async fn load_or_initialize(
    ctx: &Ctx<AgentOsActor>,
    initial: &ConfigSnapshot,
) -> Result<ConfigSnapshot> {
    migrate(ctx).await?;
    let result = ctx
        .sql()
        .query(
            "SELECT desired_config, revision, applied_revision, status, issues, created_at_ms, updated_at_ms
             FROM agentos_actor_config WHERE id = 1",
            None,
        )
        .await
        .context("load actor config")?;

    if let Some(row) = result.rows.first() {
        return decode_snapshot(row);
    }

    persist(ctx, initial).await?;
    Ok(initial.clone())
}

pub(crate) async fn persist(ctx: &Ctx<AgentOsActor>, snapshot: &ConfigSnapshot) -> Result<()> {
    let desired_config = serde_json::to_string(&snapshot.desired)
        .context("encode desired actor config for sqlite")?;
    let issues =
        serde_json::to_string(&snapshot.issues).context("encode actor config issues for sqlite")?;
    let applied_revision = snapshot
        .applied_revision
        .map(i64::try_from)
        .transpose()
        .context("applied config revision exceeds sqlite integer range")?;
    ctx.sql()
        .execute(
            "INSERT INTO agentos_actor_config (
                id, desired_config, revision, applied_revision, status, issues,
                created_at_ms, updated_at_ms
             ) VALUES (1, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                desired_config = excluded.desired_config,
                revision = excluded.revision,
                applied_revision = excluded.applied_revision,
                status = excluded.status,
                issues = excluded.issues,
                updated_at_ms = excluded.updated_at_ms",
            Some(vec![
                BindParam::Text(desired_config),
                BindParam::Integer(
                    i64::try_from(snapshot.revision)
                        .context("config revision exceeds sqlite integer range")?,
                ),
                applied_revision
                    .map(BindParam::Integer)
                    .unwrap_or(BindParam::Null),
                BindParam::Text(snapshot.status.as_str().to_owned()),
                BindParam::Text(issues),
                BindParam::Integer(snapshot.created_at_ms),
                BindParam::Integer(snapshot.updated_at_ms),
            ]),
        )
        .await
        .context("persist actor config")?;
    Ok(())
}

/// Claim a new generation before constructing a VM. The counter lives in
/// Rivet SQLite, so handles from a previous actor incarnation cannot become
/// valid again after sleep, migration, or process restart. Failed boots still
/// consume their generation.
pub(crate) async fn allocate_vm_generation(ctx: &Ctx<AgentOsActor>) -> Result<u64> {
    let result = ctx
        .sql()
        .execute(ALLOCATE_VM_GENERATION_SQL, None)
        .await
        .context("allocate durable VM generation")?;
    let generation = result
        .rows
        .first()
        .and_then(|row| row.first())
        .and_then(column_integer)
        .ok_or_else(|| anyhow!("VM generation row is missing or exhausted"))?;
    u64::try_from(generation).context("VM generation is negative")
}

pub(crate) async fn create_preview(
    ctx: &Ctx<AgentOsActor>,
    lease: &PreviewLease,
    created_at_ms: i64,
    max_previews: usize,
) -> Result<()> {
    let transaction = ctx
        .sql()
        .begin_transaction(Some(TRANSACTION_TIMEOUT))
        .await
        .context("begin preview creation")?;
    let result = async {
        transaction
            .execute(
                "DELETE FROM agentos_actor_previews
                 WHERE token IN (
                    SELECT token FROM agentos_actor_previews
                    WHERE expires_at_ms <= ? ORDER BY expires_at_ms LIMIT ?
                 )",
                Some(vec![
                    BindParam::Integer(created_at_ms),
                    BindParam::Integer(PREVIEW_CLEANUP_BATCH),
                ]),
            )
            .await
            .context("clean expired preview leases")?;
        let count = transaction
            .exec("SELECT COUNT(*) FROM agentos_actor_previews")
            .await
            .context("count preview leases")?
            .rows
            .first()
            .and_then(|row| row.first())
            .and_then(column_integer)
            .ok_or_else(|| anyhow!("preview lease count is missing or invalid"))?;
        if count >= i64::try_from(max_previews).context("preview limit exceeds sqlite integer")? {
            bail!(
                "limit_exceeded: actor has {count} active previews; maximum is {max_previews}; expire a preview before creating another"
            );
        }
        transaction
            .execute(
                "INSERT INTO agentos_actor_previews (token, port, expires_at_ms, created_at_ms)
                 VALUES (?, ?, ?, ?)",
                Some(vec![
                    BindParam::Text(lease.token.clone()),
                    BindParam::Integer(i64::from(lease.port)),
                    BindParam::Integer(lease.expires_at_ms),
                    BindParam::Integer(created_at_ms),
                ]),
            )
            .await
            .context("insert preview lease")?;
        Result::<()>::Ok(())
    }
    .await;
    match result {
        Ok(()) => transaction
            .commit()
            .await
            .context("commit preview creation"),
        Err(error) => match transaction.rollback().await {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(error.context(format!(
                "rollback preview creation failed: {rollback_error:#}"
            ))),
        },
    }
}

pub(crate) async fn load_preview(
    ctx: &Ctx<AgentOsActor>,
    token: &str,
    now_ms: i64,
) -> Result<Option<PreviewLease>> {
    let result = ctx
        .sql()
        .query(
            "SELECT token, port, expires_at_ms FROM agentos_actor_previews WHERE token = ?",
            Some(vec![BindParam::Text(token.to_owned())]),
        )
        .await
        .context("load preview lease")?;
    let Some(row) = result.rows.first() else {
        return Ok(None);
    };
    let port = u16::try_from(column_integer_required(row.get(1), "preview port")?)
        .context("preview port is outside the u16 range")?;
    let lease = PreviewLease {
        token: column_text(row.first(), "preview token")?.to_owned(),
        port,
        expires_at_ms: column_integer_required(row.get(2), "preview expiration")?,
    };
    if lease.expires_at_ms <= now_ms {
        expire_preview(ctx, token).await?;
        return Ok(None);
    }
    Ok(Some(lease))
}

pub(crate) async fn expire_preview(ctx: &Ctx<AgentOsActor>, token: &str) -> Result<bool> {
    let result = ctx
        .sql()
        .execute(
            "DELETE FROM agentos_actor_previews WHERE token = ?",
            Some(vec![BindParam::Text(token.to_owned())]),
        )
        .await
        .context("expire preview lease")?;
    Ok(result.changes > 0)
}

async fn migrate(ctx: &Ctx<AgentOsActor>) -> Result<()> {
    let transaction = ctx
        .sql()
        .begin_transaction(Some(TRANSACTION_TIMEOUT))
        .await
        .context("begin actor schema migration")?;
    let result = async {
        transaction
            .execute(
                "CREATE TABLE IF NOT EXISTS agentos_actor_schema_version (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    version INTEGER NOT NULL
                 ) STRICT",
                None,
            )
            .await
            .context("create actor schema version table")?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO agentos_actor_schema_version (id, version) VALUES (1, 0)",
                None,
            )
            .await
            .context("initialize actor schema version")?;
        let version_result = transaction
            .exec("SELECT version FROM agentos_actor_schema_version WHERE id = 1")
            .await
            .context("read actor schema version")?;
        let version = version_result
            .rows
            .first()
            .and_then(|row| row.first())
            .and_then(column_integer)
            .ok_or_else(|| anyhow!("actor schema version row is missing or invalid"))?;

        let mut version = version;
        if version == 0 {
            transaction
                .execute(
                    "CREATE TABLE agentos_actor_config (
                            id INTEGER PRIMARY KEY CHECK (id = 1),
                            desired_config TEXT NOT NULL,
                            revision INTEGER NOT NULL CHECK (revision >= 1),
                            applied_revision INTEGER,
                            status TEXT NOT NULL,
                            issues TEXT NOT NULL,
                            created_at_ms INTEGER NOT NULL,
                            updated_at_ms INTEGER NOT NULL
                         ) STRICT",
                    None,
                )
                .await
                .context("create actor config table")?;
            transaction
                .execute(
                    "UPDATE agentos_actor_schema_version SET version = 1 WHERE id = 1",
                    None,
                )
                .await
                .context("advance actor schema version")?;
            version = 1;
        }
        if version == 1 {
            transaction
                .execute(
                    "CREATE TABLE agentos_actor_previews (
                        token TEXT PRIMARY KEY,
                        port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
                        expires_at_ms INTEGER NOT NULL,
                        created_at_ms INTEGER NOT NULL
                     ) STRICT",
                    None,
                )
                .await
                .context("create actor preview table")?;
            transaction
                .execute(
                    "CREATE INDEX agentos_actor_previews_expiration
                     ON agentos_actor_previews (expires_at_ms)",
                    None,
                )
                .await
                .context("create actor preview expiration index")?;
            transaction
                .execute(
                    "UPDATE agentos_actor_schema_version SET version = 2 WHERE id = 1",
                    None,
                )
                .await
                .context("advance actor schema version")?;
            version = 2;
        }
        if version == 2 {
            transaction
                .execute(
                    "CREATE TABLE agentos_actor_vm_generation (
                        id INTEGER PRIMARY KEY CHECK (id = 1),
                        generation INTEGER NOT NULL CHECK (generation >= 0)
                     ) STRICT",
                    None,
                )
                .await
                .context("create actor VM generation table")?;
            transaction
                .execute(
                    "INSERT INTO agentos_actor_vm_generation (id, generation) VALUES (1, 0)",
                    None,
                )
                .await
                .context("initialize actor VM generation")?;
            transaction
                .execute(
                    "UPDATE agentos_actor_schema_version SET version = 3 WHERE id = 1",
                    None,
                )
                .await
                .context("advance actor schema version")?;
            version = 3;
        }
        if version != SCHEMA_VERSION {
            bail!("unsupported agentOS actor schema version {version}; expected {SCHEMA_VERSION}");
        }
        Result::<()>::Ok(())
    }
    .await;

    match result {
        Ok(()) => transaction
            .commit()
            .await
            .context("commit actor schema migration"),
        Err(error) => match transaction.rollback().await {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(error.context(format!(
                "rollback actor schema migration failed: {rollback_error:#}"
            ))),
        },
    }
}

fn decode_snapshot(row: &[ColumnValue]) -> Result<ConfigSnapshot> {
    let desired = serde_json::from_str(column_text(row.first(), "desired_config")?)
        .context("decode desired actor config from sqlite")?;
    let revision = integer_to_u64(column_integer_required(row.get(1), "revision")?, "revision")?;
    let applied_revision = match row.get(2) {
        Some(ColumnValue::Null) | None => None,
        value => Some(integer_to_u64(
            column_integer_required(value, "applied_revision")?,
            "applied_revision",
        )?),
    };
    let status = column_text(row.get(3), "status")?.parse()?;
    let issues = serde_json::from_str(column_text(row.get(4), "issues")?)
        .context("decode actor config issues from sqlite")?;
    Ok(ConfigSnapshot {
        revision,
        desired,
        applied_revision,
        status,
        issues,
        created_at_ms: column_integer_required(row.get(5), "created_at_ms")?,
        updated_at_ms: column_integer_required(row.get(6), "updated_at_ms")?,
    })
}

fn column_integer(value: &ColumnValue) -> Option<i64> {
    match value {
        ColumnValue::Integer(value) => Some(*value),
        _ => None,
    }
}

fn column_integer_required(value: Option<&ColumnValue>, name: &str) -> Result<i64> {
    value
        .and_then(column_integer)
        .ok_or_else(|| anyhow!("actor config column {name} is missing or not an integer"))
}

fn column_text<'a>(value: Option<&'a ColumnValue>, name: &str) -> Result<&'a str> {
    match value {
        Some(ColumnValue::Text(value)) => Ok(value),
        _ => bail!("actor config column {name} is missing or not text"),
    }
}

fn integer_to_u64(value: i64, name: &str) -> Result<u64> {
    u64::try_from(value).with_context(|| format!("actor config column {name} is negative"))
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::time::Duration;

    use anyhow::Context;
    use async_trait::async_trait;
    use rivetkit::{action, test, Action, Actor, Ctx, Handles, Registry};
    use serde::{Deserialize, Serialize};

    use super::{
        handle_rivet_sqlite_request, preserve_rollback_error, require_callback_database,
        require_expected_changes, sqlite_bindings, sqlite_result, VmSqliteCallbackRequest,
        VmSqliteCallbackResponse, VmSqliteStatement, VmSqliteValue, ALLOCATE_VM_GENERATION_SQL,
    };

    #[test]
    fn sqlite_callback_preserves_original_and_rollback_failures() {
        let error = preserve_rollback_error(
            anyhow::anyhow!("compare-and-set failed"),
            Err(anyhow::anyhow!("connection lost")),
        );
        let message = format!("{error:#}");
        assert!(message.contains("compare-and-set failed"));
        assert!(message.contains("rollback Rivet SQLite VM transaction failed: connection lost"));
        let error = preserve_rollback_error(anyhow::anyhow!("compare-and-set failed"), Ok(()));
        assert_eq!(error.to_string(), "compare-and-set failed");
    }

    #[test]
    fn sqlite_callback_checks_database_and_expected_changes() {
        assert!(require_callback_database("vm-1", "vm-1").is_ok());
        assert!(require_callback_database("vm-1", "vm-2").is_err());
        assert!(require_expected_changes(None, 9).is_ok());
        assert!(require_expected_changes(Some(0), 0).is_ok());
        assert!(require_expected_changes(Some(1), 1).is_ok());
        let error = require_expected_changes(Some(0), 1).expect_err("reject changed row count");
        assert!(error.to_string().contains("affected 1 rows; expected 0"));
    }

    #[test]
    fn sqlite_callback_preserves_bindings_and_result_metadata() {
        let values = vec![
            VmSqliteValue::Null,
            VmSqliteValue::Integer(i64::MAX),
            VmSqliteValue::Real(1.25),
            VmSqliteValue::Text(String::from("value")),
            VmSqliteValue::Blob(vec![0, 1, 255]),
        ];
        assert_eq!(
            sqlite_bindings(values.clone()),
            vec![
                rivetkit::BindParam::Null,
                rivetkit::BindParam::Integer(i64::MAX),
                rivetkit::BindParam::Float(1.25),
                rivetkit::BindParam::Text(String::from("value")),
                rivetkit::BindParam::Blob(vec![0, 1, 255]),
            ]
        );
        let columns = vec!["null", "integer", "real", "text", "blob"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let result = sqlite_result(
            columns.clone(),
            vec![vec![
                rivetkit::ColumnValue::Null,
                rivetkit::ColumnValue::Integer(i64::MAX),
                rivetkit::ColumnValue::Float(1.25),
                rivetkit::ColumnValue::Text(String::from("value")),
                rivetkit::ColumnValue::Blob(vec![0, 1, 255]),
            ]],
            2,
            Some(7),
        );
        assert_eq!(result.columns, columns);
        assert_eq!(result.rows, vec![values]);
        assert_eq!(result.changes, 2);
        assert_eq!(result.last_insert_row_id, Some(7));
    }

    struct SqliteProbeActor;

    #[derive(Debug, Serialize, Deserialize)]
    struct ProbeSqliteAdapter;

    impl Action for ProbeSqliteAdapter {
        type Output = bool;

        const NAME: &'static str = "probe.sqliteAdapter";
    }

    #[async_trait]
    impl Actor for SqliteProbeActor {
        type State = ();
        type Input = ();
        type Actions = (ProbeSqliteAdapter,);
        type Events = ();
        type Queue = ();
        type ConnParams = ();
        type ConnState = ();
        type Action = action::Raw;

        async fn create_state(
            _ctx: &Ctx<Self>,
            _input: Self::Input,
        ) -> anyhow::Result<Self::State> {
            Ok(())
        }

        async fn create(_ctx: &Ctx<Self>) -> anyhow::Result<Self> {
            Ok(Self)
        }
    }

    impl Handles<ProbeSqliteAdapter> for SqliteProbeActor {
        type Future = Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send>>;

        fn handle(self: Arc<Self>, ctx: Ctx<Self>, _action: ProbeSqliteAdapter) -> Self::Future {
            Box::pin(async move {
                ctx.sql()
                    .execute(
                        "CREATE TABLE agentos_actor_adapter_probe (
                            id INTEGER PRIMARY KEY,
                            value BLOB NOT NULL
                         ) STRICT",
                        None,
                    )
                    .await?;

                let query = handle_rivet_sqlite_request(
                    &ctx,
                    "probe",
                    VmSqliteCallbackRequest::Query {
                        database: String::from("probe"),
                        statement: VmSqliteStatement {
                            sql: String::from(
                                "INSERT INTO agentos_actor_adapter_probe (id, value) VALUES (?, ?)",
                            ),
                            params: vec![
                                VmSqliteValue::Integer(1),
                                VmSqliteValue::Blob(vec![0, 1, 255]),
                            ],
                            expected_changes: Some(1),
                        },
                    },
                )
                .await?;
                let VmSqliteCallbackResponse::Query { result } = query else {
                    anyhow::bail!("single-statement callback returned the wrong response variant");
                };
                anyhow::ensure!(result.changes == 1, "insert callback lost its change count");
                anyhow::ensure!(
                    result.last_insert_row_id == Some(1),
                    "insert callback lost its last inserted row ID"
                );

                let query_rolled_back = handle_rivet_sqlite_request(
                    &ctx,
                    "probe",
                    VmSqliteCallbackRequest::Query {
                        database: String::from("probe"),
                        statement: VmSqliteStatement {
                            sql: String::from(
                                "INSERT INTO agentos_actor_adapter_probe (id, value) VALUES (?, ?)",
                            ),
                            params: vec![VmSqliteValue::Integer(3), VmSqliteValue::Blob(vec![3])],
                            expected_changes: Some(0),
                        },
                    },
                )
                .await;
                anyhow::ensure!(
                    query_rolled_back.is_err(),
                    "failed single-statement compare-and-set must report an error"
                );

                let rolled_back = handle_rivet_sqlite_request(
                    &ctx,
                    "probe",
                    VmSqliteCallbackRequest::Transaction {
                        database: String::from("probe"),
                        statements: vec![
                            VmSqliteStatement {
                                sql: String::from(
                                    "INSERT INTO agentos_actor_adapter_probe (id, value) VALUES (?, ?)",
                                ),
                                params: vec![
                                    VmSqliteValue::Integer(2),
                                    VmSqliteValue::Blob(vec![2]),
                                ],
                                expected_changes: Some(1),
                            },
                            VmSqliteStatement {
                                sql: String::from(
                                    "UPDATE agentos_actor_adapter_probe SET value = ? WHERE id = 999",
                                ),
                                params: vec![VmSqliteValue::Blob(vec![3])],
                                expected_changes: Some(1),
                            },
                        ],
                    },
                )
                .await;
                anyhow::ensure!(
                    rolled_back.is_err(),
                    "failed batch compare-and-set must report an error"
                );

                let query = handle_rivet_sqlite_request(
                    &ctx,
                    "probe",
                    VmSqliteCallbackRequest::Query {
                        database: String::from("probe"),
                        statement: VmSqliteStatement {
                            sql: String::from(
                                "SELECT id, value FROM agentos_actor_adapter_probe ORDER BY id",
                            ),
                            params: vec![],
                            expected_changes: None,
                        },
                    },
                )
                .await?;
                let VmSqliteCallbackResponse::Query { result } = query else {
                    anyhow::bail!("read callback returned the wrong response variant");
                };
                anyhow::ensure!(
                    result.columns == ["id", "value"],
                    "read callback lost its columns"
                );
                anyhow::ensure!(
                    result.rows
                        == vec![vec![
                            VmSqliteValue::Integer(1),
                            VmSqliteValue::Blob(vec![0, 1, 255])
                        ]],
                    "failed compare-and-set left rows behind, or callback lost integer/blob values"
                );

                ctx.sql()
                    .execute(
                        "CREATE TABLE agentos_actor_vm_generation (
                            id INTEGER PRIMARY KEY CHECK (id = 1),
                            generation INTEGER NOT NULL CHECK (generation >= 0)
                         ) STRICT",
                        None,
                    )
                    .await?;
                ctx.sql()
                    .execute(
                        "INSERT INTO agentos_actor_vm_generation (id, generation) VALUES (1, 0)",
                        None,
                    )
                    .await?;
                for expected in [1, 2] {
                    let result = ctx.sql().execute(ALLOCATE_VM_GENERATION_SQL, None).await?;
                    anyhow::ensure!(
                        result.rows.first().and_then(|row| row.first())
                            == Some(&rivetkit::ColumnValue::Integer(expected)),
                        "generation allocation did not return {expected}"
                    );
                }
                Ok(true)
            })
        }
    }

    /// Requires a local Rivet engine; opt in with the test harness's engine
    /// resolver to exercise the actual RivetKit SQLite driver, not rusqlite.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires a local RivetKit engine binary or opt-in engine download"]
    async fn rivet_driver_rolls_back_callback_transactions_and_claims_generations(
    ) -> anyhow::Result<()> {
        let mut registry = Registry::new();
        registry.register_actor::<SqliteProbeActor>("sqliteProbe");
        let harness = test::setup(registry).await?;
        let actor = harness.actor::<SqliteProbeActor>("sqliteProbe");
        let result =
            tokio::time::timeout(Duration::from_secs(60), actor.send(ProbeSqliteAdapter)).await;
        // Drain the registry even when the probe returns an action error.
        let shutdown = tokio::time::timeout(Duration::from_secs(30), harness.shutdown()).await;
        if let Err(error) = &shutdown {
            tracing::error!(%error, "SQLite probe registry shutdown timed out");
        }
        anyhow::ensure!(
            result.context("SQLite probe action timed out")??,
            "SQLite probe returned false"
        );
        shutdown.context("SQLite probe registry shutdown timed out")?;
        Ok(())
    }

    #[test]
    fn vm_generation_never_reuses_a_handle_after_database_reopen() {
        let dir = tempfile::tempdir().expect("temporary actor database");
        let path = dir.path().join("actor.sqlite");
        {
            let db = rusqlite::Connection::open(&path).expect("open actor database");
            db.execute_batch(
                "CREATE TABLE agentos_actor_vm_generation (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    generation INTEGER NOT NULL CHECK (generation >= 0)
                 ) STRICT;
                 INSERT INTO agentos_actor_vm_generation (id, generation) VALUES (1, 0);",
            )
            .expect("initialize durable generation");
            let generation: i64 = db
                .query_row(ALLOCATE_VM_GENERATION_SQL, [], |row| row.get(0))
                .expect("allocate first generation");
            assert_eq!(generation, 1);
        }
        let db = rusqlite::Connection::open(&path).expect("reopen actor database");
        let generation: i64 = db
            .query_row(ALLOCATE_VM_GENERATION_SQL, [], |row| row.get(0))
            .expect("allocate generation after restart");
        assert_eq!(generation, 2);
    }
}
