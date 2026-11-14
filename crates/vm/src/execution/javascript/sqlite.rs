mod transfer;
use super::super::*;

use rusqlite::config::DbConfig;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};

// These settings affect every SQLite connection in the host process.
const SQLITE_PROCESS_WIDE_PRAGMAS: [&str; 4] = [
    "data_store_directory",
    "hard_heap_limit",
    "soft_heap_limit",
    "temp_store_directory",
];

fn authorize_guest_sqlite_action(context: AuthContext<'_>) -> Authorization {
    match context.action {
        // Plain VACUUM uses an unnamed temporary attachment.
        AuthAction::Attach { filename } if filename.is_empty() || filename == ":memory:" => {
            Authorization::Allow
        }
        // SQL filenames are interpreted by native SQLite, not by the VM VFS.
        AuthAction::Attach { .. } => Authorization::Deny,
        // SQLite supplies no filename for an ATTACH/VACUUM INTO expression.
        AuthAction::Unknown {
            code: rusqlite::ffi::SQLITE_ATTACH,
            ..
        } => Authorization::Deny,
        AuthAction::Pragma { pragma_name, .. }
            if SQLITE_PROCESS_WIDE_PRAGMAS
                .iter()
                .any(|name| pragma_name.eq_ignore_ascii_case(name)) =>
        {
            Authorization::Deny
        }
        _ => Authorization::Allow,
    }
}

const SQLITE_JS_SAFE_INTEGER_MAX: i64 = 9_007_199_254_740_991;

pub(in crate::execution) fn service_javascript_sqlite_sync_rpc(
    kernel: &mut SidecarKernel,
    process: &mut ActiveProcess,
    request: &HostRpcRequest,
) -> Result<Value, VmError> {
    if request.method.starts_with("sqlite.transfer.") {
        return transfer::method(process, request);
    }
    match request.method.as_str() {
        "sqlite.constants" => Ok(json!({})),
        "sqlite.open" => sqlite_open_database(kernel, process, request),
        "sqlite.close" => {
            let database_id =
                javascript_sync_rpc_arg_u64(&request.args, 0, "sqlite.close database id")?;
            close_sqlite_database(kernel, process, database_id, false)?;
            Ok(Value::Null)
        }
        "sqlite.exec" => sqlite_exec_database(kernel, process, request),
        "sqlite.query" | "sqlite.queryChunked" => sqlite_query_database(kernel, process, request),
        "sqlite.prepare" => sqlite_prepare_statement(process, request),
        "sqlite.location" => {
            let database_id =
                javascript_sync_rpc_arg_u64(&request.args, 0, "sqlite.location database id")?;
            let database = sqlite_database(process, database_id)?;
            Ok(database
                .vm_path
                .as_ref()
                .map(|path| Value::String(path.clone()))
                .unwrap_or(Value::Null))
        }
        "sqlite.isTransaction" => {
            let id =
                javascript_sync_rpc_arg_u64(&request.args, 0, "sqlite.isTransaction database id")?;
            Ok(json!(
                !sqlite_database(process, id)?.connection.is_autocommit()
            ))
        }
        "sqlite.checkpoint" => {
            let database_id =
                javascript_sync_rpc_arg_u64(&request.args, 0, "sqlite.checkpoint database id")?;
            let kernel_pid = process.kernel_pid;
            let database = sqlite_database_mut(process, database_id)?;
            sqlite_sync_database(kernel, kernel_pid, database, false)?;
            Ok(Value::Null)
        }
        "sqlite.statement.run" | "sqlite.statement.runChunked" => {
            sqlite_run_statement(kernel, process, request)
        }
        "sqlite.statement.get" | "sqlite.statement.getChunked" => {
            sqlite_get_statement(kernel, process, request)
        }
        "sqlite.statement.all" | "sqlite.statement.iterate" | "sqlite.statement.allChunked" => {
            sqlite_all_statement(kernel, process, request)
        }
        "sqlite.statement.columns" => sqlite_statement_columns(process, request),
        "sqlite.statement.setReturnArrays" => {
            let statement_id = javascript_sync_rpc_arg_u64(
                &request.args,
                0,
                "sqlite.statement.setReturnArrays statement id",
            )?;
            let enabled = javascript_sync_rpc_arg_bool(
                &request.args,
                1,
                "sqlite.statement.setReturnArrays enabled",
            )?;
            sqlite_statement_mut(process, statement_id)?.return_arrays = enabled;
            Ok(Value::Null)
        }
        "sqlite.statement.setReadBigInts" => {
            let statement_id = javascript_sync_rpc_arg_u64(
                &request.args,
                0,
                "sqlite.statement.setReadBigInts statement id",
            )?;
            let enabled = javascript_sync_rpc_arg_bool(
                &request.args,
                1,
                "sqlite.statement.setReadBigInts enabled",
            )?;
            sqlite_statement_mut(process, statement_id)?.read_bigints = enabled;
            Ok(Value::Null)
        }
        "sqlite.statement.setAllowBareNamedParameters" => {
            let statement_id = javascript_sync_rpc_arg_u64(
                &request.args,
                0,
                "sqlite.statement.setAllowBareNamedParameters statement id",
            )?;
            let enabled = javascript_sync_rpc_arg_bool(
                &request.args,
                1,
                "sqlite.statement.setAllowBareNamedParameters enabled",
            )?;
            sqlite_statement_mut(process, statement_id)?.allow_bare_named_parameters = enabled;
            Ok(Value::Null)
        }
        "sqlite.statement.setAllowUnknownNamedParameters" => {
            let statement_id = javascript_sync_rpc_arg_u64(
                &request.args,
                0,
                "sqlite.statement.setAllowUnknownNamedParameters statement id",
            )?;
            let enabled = javascript_sync_rpc_arg_bool(
                &request.args,
                1,
                "sqlite.statement.setAllowUnknownNamedParameters enabled",
            )?;
            sqlite_statement_mut(process, statement_id)?.allow_unknown_named_parameters = enabled;
            Ok(Value::Null)
        }
        "sqlite.statement.finalize" => {
            let statement_id = javascript_sync_rpc_arg_u64(
                &request.args,
                0,
                "sqlite.statement.finalize statement id",
            )?;
            process
                .sqlite_statements
                .remove(&statement_id)
                .ok_or_else(|| {
                    VmError::InvalidState(format!(
                        "sqlite statement handle not found: {statement_id}"
                    ))
                })?;
            Ok(Value::Null)
        }
        other => Err(VmError::InvalidState(format!(
            "unsupported JavaScript sqlite sync RPC method {other}"
        ))),
    }
}

fn sqlite_open_database(
    kernel: &mut SidecarKernel,
    process: &mut ActiveProcess,
    request: &HostRpcRequest,
) -> Result<Value, VmError> {
    ensure_per_process_state_handle_capacity(process.sqlite_databases.len(), "sqlite database")?;
    let path = request.args.first().and_then(Value::as_str);
    let vm_path = path.filter(|value| !value.is_empty() && *value != ":memory:");
    let options = request.args.get(1);
    let read_only = sqlite_option_bool(options, "readOnly").unwrap_or(false);
    let create = sqlite_option_bool(options, "create").unwrap_or(!read_only);
    let timeout_ms = sqlite_option_u64(options, "timeout");

    process.next_sqlite_database_id += 1;
    let database_id = process.next_sqlite_database_id;

    let host_path = vm_path.map(|vm_path| {
        let digest = Sha256::digest(vm_path.as_bytes());
        let path_key = digest
            .iter()
            .take(16)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        std::env::temp_dir()
            .join(format!(
                "agentos-vm-sqlite-{}",
                process.sqlite_host_namespace
            ))
            .join(path_key)
            .join("database.sqlite")
    });
    let host_database_exists = host_path.as_ref().is_some_and(|path| path.exists());

    if let Some(host_path) = host_path.as_ref() {
        if let Some(parent) = host_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                VmError::Io(format!(
                    "failed to prepare sqlite temp directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
    }

    if !host_database_exists {
        if let (Some(vm_path), Some(host_path)) = (vm_path, host_path.as_ref()) {
            if kernel
                .exists_for_process(EXECUTION_DRIVER_NAME, process.kernel_pid, vm_path)
                .map_err(kernel_error)?
            {
                let contents = kernel
                    .read_file_for_process(EXECUTION_DRIVER_NAME, process.kernel_pid, vm_path)
                    .map_err(kernel_error)?;
                fs::write(host_path, contents).map_err(|error| {
                    VmError::Io(format!(
                        "failed to materialize sqlite database {}: {error}",
                        host_path.display()
                    ))
                })?;
            } else if read_only && !create {
                return Err(VmError::InvalidState(format!(
                    "sqlite database does not exist: {vm_path}"
                )));
            }
        }
    }

    let target = host_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| String::from(":memory:"));
    let mut flags = if read_only {
        SqliteOpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        SqliteOpenFlags::SQLITE_OPEN_READ_WRITE
    };
    if create && !read_only {
        flags |= SqliteOpenFlags::SQLITE_OPEN_CREATE;
    }

    let mut connection = SqliteConnection::open_with_flags(&target, flags).map_err(|error| {
        VmError::InvalidState(format!(
            "sqlite database open failed for {}: {error}",
            vm_path.unwrap_or(":memory:")
        ))
    })?;
    if let Some(timeout_ms) = timeout_ms {
        connection
            .busy_timeout(Duration::from_millis(timeout_ms))
            .map_err(sqlite_error)?;
    }
    if host_path.is_some() && !read_only {
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(sqlite_error)?;
    }

    // Keep schema writes defensive before any guest statement is prepared.
    connection
        .set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
        .map_err(sqlite_error)?;
    connection
        .authorizer(Some(authorize_guest_sqlite_action))
        .map_err(sqlite_error)?;

    let mut database = ActiveSqliteDatabase {
        next_transfer_generation: 0,
        input_transfer: None,
        output_transfer: None,
        connection,
        host_path,
        vm_path: vm_path.map(String::from),
        dirty: !read_only && vm_path.is_some(),
        transaction_depth: 0,
        read_only,
    };
    // Opening a writable file must enforce VFS permissions now, not on close.
    // Committed bytes must also be visible to ordinary guest filesystem reads.
    sqlite_sync_database(kernel, process.kernel_pid, &mut database, false)?;
    process.sqlite_databases.insert(database_id, database);
    Ok(json!(database_id))
}

fn sqlite_exec_database(
    kernel: &mut SidecarKernel,
    process: &mut ActiveProcess,
    request: &HostRpcRequest,
) -> Result<Value, VmError> {
    let database_id = javascript_sync_rpc_arg_u64(&request.args, 0, "sqlite.exec database id")?;
    let sql = javascript_sync_rpc_arg_str(&request.args, 1, "sqlite.exec sql")?;
    let kernel_pid = process.kernel_pid;
    let database = sqlite_database_mut(process, database_id)?;
    let before = database.connection.total_changes();
    database
        .connection
        .execute_batch(sql)
        .map_err(sqlite_error)?;
    mark_sqlite_mutation(database, sql);
    // exec accepts a script: later statements can mutate even when the first
    // statement is read-only, and schema changes do not increment total_changes.
    database.dirty = true;
    sqlite_sync_database(kernel, kernel_pid, database, false)?;
    Ok(json!(
        database.connection.total_changes().saturating_sub(before)
    ))
}

fn sqlite_query_database(
    kernel: &mut SidecarKernel,
    process: &mut ActiveProcess,
    request: &HostRpcRequest,
) -> Result<Value, VmError> {
    let database_id = javascript_sync_rpc_arg_u64(&request.args, 0, "sqlite.query database id")?;
    let sql = javascript_sync_rpc_arg_str(&request.args, 1, "sqlite.query sql")?;
    let options = request.args.get(3);
    let return_arrays = sqlite_option_bool(options, "returnArrays").unwrap_or(false);
    let read_bigints = sqlite_option_bool(options, "readBigInts").unwrap_or(false);
    let params = transfer::parameters(process, database_id, request, 2, 4)?;
    let runtime = process.runtime_context.clone();
    let result_limit = process.limits.sqlite.max_result_bytes;
    let kernel_pid = process.kernel_pid;
    let database = sqlite_database_mut(process, database_id)?;
    database.output_transfer = None;
    let rows = transfer::query_rows(
        &mut database.connection,
        sql,
        params.value(),
        return_arrays,
        read_bigints,
        true,
        false,
        runtime,
        result_limit,
        false,
    );
    let rows = publish_sqlite_query_result(kernel, kernel_pid, database, sql, rows)?;
    transfer::reply(
        process,
        database_id,
        rows,
        request.method.ends_with("Chunked"),
    )
}

// Use SQLite's own parser through rusqlite's safe API. Prefixes end only at
// semicolons, so parameter expansion never makes a partial parameter look like
// the complete statement. Once SQLite recognizes the first statement, every
// later prefix produces that same statement. Binary search avoids a quadratic
// prepare loop for long triggers and quoted/commented semicolons.
fn sqlite_statement_source(connection: &SqliteConnection, sql: &str) -> Result<String, VmError> {
    if sql.as_bytes().contains(&0) {
        return Err(VmError::host(
            "ERR_INVALID_ARG_VALUE",
            "SQL cannot contain a NUL byte",
        ));
    }
    use rusqlite::fallible_iterator::FallibleIterator as _;
    // Batch::next prepares only the first statement, matching Node even when
    // later SQL is invalid. Connection::prepare rejects tails in rusqlite 0.43.
    let first_statement = |source: &str| rusqlite::Batch::new(connection, source).next();
    let expected = first_statement(sql)
        .map_err(sqlite_error)?
        .ok_or_else(|| VmError::host("ERR_INVALID_ARG_VALUE", "SQL contains no statement"))?;
    let expanded = expected
        .expanded_sql()
        .ok_or_else(|| VmError::host("ERR_INVALID_ARG_VALUE", "SQL contains no statement"))?;
    let count = sql.bytes().filter(|byte| *byte == b';').count();
    let end_at = |index| {
        sql.match_indices(';')
            .nth(index)
            .map(|(offset, _)| offset + 1)
            .unwrap_or(sql.len())
    };
    let mut low = 0;
    let mut high = count;
    while low < high {
        let middle = low + (high - low) / 2;
        // Incomplete quoted strings/comments/trigger bodies are expected parse
        // misses here. Errors preparing the actual request propagated above.
        let same = first_statement(&sql[..end_at(middle)]).is_ok_and(|candidate| {
            candidate.is_some_and(|candidate| {
                candidate.parameter_count() == expected.parameter_count()
                    && (1..=expected.parameter_count()).all(|index| {
                        candidate.parameter_name(index) == expected.parameter_name(index)
                    })
                    && candidate.expanded_sql().as_deref() == Some(expanded.as_str())
            })
        });
        if same {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    Ok(sql[..end_at(low)].to_owned())
}

fn sqlite_prepare_statement(
    process: &mut ActiveProcess,
    request: &HostRpcRequest,
) -> Result<Value, VmError> {
    ensure_per_process_state_handle_capacity(process.sqlite_statements.len(), "sqlite statement")?;
    let database_id = javascript_sync_rpc_arg_u64(&request.args, 0, "sqlite.prepare database id")?;
    let sql = javascript_sync_rpc_arg_str(&request.args, 1, "sqlite.prepare sql")?;
    let database = sqlite_database(process, database_id)?;
    let source_sql = sqlite_statement_source(&database.connection, sql)?;
    // The guest already owns the original SQL. Return only the first statement's
    // UTF-16 length, avoiding an unbounded echo in the small metadata reply.
    let source_sql_length = source_sql.encode_utf16().count();
    process.next_sqlite_statement_id += 1;
    let statement_id = process.next_sqlite_statement_id;
    process.sqlite_statements.insert(
        statement_id,
        ActiveSqliteStatement {
            database_id,
            sql: source_sql,
            return_arrays: false,
            read_bigints: false,
            allow_bare_named_parameters: false,
            allow_unknown_named_parameters: false,
        },
    );
    Ok(json!({"statementId":statement_id,"sourceSQLLength":source_sql_length}))
}

fn sqlite_run_statement(
    kernel: &mut SidecarKernel,
    process: &mut ActiveProcess,
    request: &HostRpcRequest,
) -> Result<Value, VmError> {
    let statement_id =
        javascript_sync_rpc_arg_u64(&request.args, 0, "sqlite.statement.run statement id")?;
    let statement_state = sqlite_statement(process, statement_id)?.clone();
    let params = transfer::parameters(process, statement_state.database_id, request, 1, 2)?;
    let kernel_pid = process.kernel_pid;
    let database = sqlite_database_mut(process, statement_state.database_id)?;
    let before = database.connection.total_changes();
    {
        let mut statement = database
            .connection
            .prepare(&statement_state.sql)
            .map_err(sqlite_error)?;
        bind_sqlite_parameters(
            &mut statement,
            params.value(),
            statement_state.allow_bare_named_parameters,
            statement_state.allow_unknown_named_parameters,
        )?;
        statement.raw_execute().map_err(sqlite_error)?;
    }
    let changes = database.connection.total_changes().saturating_sub(before);
    let last_insert_rowid = database.connection.last_insert_rowid();
    mark_sqlite_mutation(database, &statement_state.sql);
    sqlite_sync_database(kernel, kernel_pid, database, false)?;
    let result = json!({
        "changes": changes,
        "lastInsertRowid": encode_sqlite_integer(last_insert_rowid, true),
    });
    Ok(result)
}

fn sqlite_get_statement(
    kernel: &mut SidecarKernel,
    process: &mut ActiveProcess,
    request: &HostRpcRequest,
) -> Result<Value, VmError> {
    let statement_id =
        javascript_sync_rpc_arg_u64(&request.args, 0, "sqlite.statement.get statement id")?;
    let statement_state = sqlite_statement(process, statement_id)?.clone();
    let params = transfer::parameters(process, statement_state.database_id, request, 1, 2)?;
    let runtime = process.runtime_context.clone();
    let result_limit = process.limits.sqlite.max_result_bytes;
    let kernel_pid = process.kernel_pid;
    let database = sqlite_database_mut(process, statement_state.database_id)?;
    database.output_transfer = None;
    let rows = transfer::query_rows(
        &mut database.connection,
        &statement_state.sql,
        params.value(),
        statement_state.return_arrays,
        statement_state.read_bigints,
        statement_state.allow_bare_named_parameters,
        statement_state.allow_unknown_named_parameters,
        runtime,
        result_limit,
        true,
    );
    let rows =
        publish_sqlite_query_result(kernel, kernel_pid, database, &statement_state.sql, rows)?;
    transfer::reply(
        process,
        statement_state.database_id,
        rows,
        request.method.ends_with("Chunked"),
    )
}

fn sqlite_all_statement(
    kernel: &mut SidecarKernel,
    process: &mut ActiveProcess,
    request: &HostRpcRequest,
) -> Result<Value, VmError> {
    let statement_id =
        javascript_sync_rpc_arg_u64(&request.args, 0, "sqlite.statement.all statement id")?;
    let statement_state = sqlite_statement(process, statement_id)?.clone();
    let params = transfer::parameters(process, statement_state.database_id, request, 1, 2)?;
    let runtime = process.runtime_context.clone();
    let result_limit = process.limits.sqlite.max_result_bytes;
    let kernel_pid = process.kernel_pid;
    let database = sqlite_database_mut(process, statement_state.database_id)?;
    database.output_transfer = None;
    let rows = transfer::query_rows(
        &mut database.connection,
        &statement_state.sql,
        params.value(),
        statement_state.return_arrays,
        statement_state.read_bigints,
        statement_state.allow_bare_named_parameters,
        statement_state.allow_unknown_named_parameters,
        runtime,
        result_limit,
        false,
    );
    let rows =
        publish_sqlite_query_result(kernel, kernel_pid, database, &statement_state.sql, rows)?;
    transfer::reply(
        process,
        statement_state.database_id,
        rows,
        request.method.ends_with("Chunked"),
    )
}

fn sqlite_statement_columns(
    process: &mut ActiveProcess,
    request: &HostRpcRequest,
) -> Result<Value, VmError> {
    let statement_id =
        javascript_sync_rpc_arg_u64(&request.args, 0, "sqlite.statement.columns statement id")?;
    let statement_state = sqlite_statement(process, statement_id)?.clone();
    let database = sqlite_database_mut(process, statement_state.database_id)?;
    let statement = database
        .connection
        .prepare(&statement_state.sql)
        .map_err(sqlite_error)?;
    Ok(Value::Array(
        statement
            .column_names()
            .map(|name| json!({ "name": name }))
            .collect(),
    ))
}

fn encode_sqlite_integer(number: i64, read_bigints: bool) -> Value {
    if read_bigints || number.abs() > SQLITE_JS_SAFE_INTEGER_MAX {
        json!({
            "__agentosSqliteType": "bigint",
            "value": number.to_string(),
        })
    } else {
        json!(number)
    }
}

fn bind_sqlite_parameters(
    statement: &mut SqliteStatement<'_>,
    params: Option<&Value>,
    allow_bare_named_parameters: bool,
    allow_unknown_named_parameters: bool,
) -> Result<(), VmError> {
    let Some(params) = params else {
        return Ok(());
    };
    match params {
        Value::Null => Ok(()),
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                statement
                    .raw_bind_parameter(index + 1, decode_sqlite_parameter(value)?)
                    .map_err(sqlite_error)?;
            }
            Ok(())
        }
        Value::Object(map)
            if map
                .get("__agentosSqliteType")
                .and_then(Value::as_str)
                .is_none() =>
        {
            for (key, value) in map {
                let index =
                    resolve_sqlite_parameter_index(statement, key, allow_bare_named_parameters)?;
                let Some(index) = index else {
                    if allow_unknown_named_parameters {
                        continue;
                    }
                    return Err(VmError::InvalidState(format!(
                        "sqlite named parameter not found: {key}"
                    )));
                };
                statement
                    .raw_bind_parameter(index, decode_sqlite_parameter(value)?)
                    .map_err(sqlite_error)?;
            }
            Ok(())
        }
        other => statement
            .raw_bind_parameter(1, decode_sqlite_parameter(other)?)
            .map_err(sqlite_error),
    }
}

fn resolve_sqlite_parameter_index(
    statement: &mut SqliteStatement<'_>,
    key: &str,
    allow_bare_named_parameters: bool,
) -> Result<Option<usize>, VmError> {
    let mut candidates = vec![key.to_owned()];
    if allow_bare_named_parameters
        && !key.starts_with(':')
        && !key.starts_with('@')
        && !key.starts_with('$')
    {
        candidates.push(format!(":{key}"));
        candidates.push(format!("@{key}"));
        candidates.push(format!("${key}"));
    }
    for candidate in candidates {
        if let Some(index) = statement
            .parameter_index(&candidate)
            .map_err(sqlite_error)?
        {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

fn decode_sqlite_parameter(value: &Value) -> Result<rusqlite::types::Value, VmError> {
    Ok(match value {
        Value::Null => rusqlite::types::Value::Null,
        Value::Bool(value) => rusqlite::types::Value::Integer(i64::from(*value)),
        Value::Number(value) => match (value.as_i64(), value.as_f64()) {
            (Some(integer), _) => rusqlite::types::Value::Integer(integer),
            (_, Some(real)) => rusqlite::types::Value::Real(real),
            _ => {
                return Err(VmError::InvalidState(String::from(
                    "sqlite parameter number is not representable",
                )));
            }
        },
        Value::String(value) => rusqlite::types::Value::Text(value.clone()),
        Value::Array(_) => {
            return Err(VmError::InvalidState(String::from(
                "sqlite parameters do not support nested arrays",
            )));
        }
        Value::Object(map) => match map.get("__agentosSqliteType").and_then(Value::as_str) {
            Some("double") => {
                let text = map
                    .get("value")
                    .and_then(Value::as_str)
                    .filter(|text| text.len() == 16)
                    .ok_or_else(|| {
                        VmError::InvalidState(String::from(
                            "sqlite double parameter missing 64-bit hexadecimal value",
                        ))
                    })?;
                let bits = u64::from_str_radix(text, 16).map_err(|error| {
                    VmError::InvalidState(format!("invalid sqlite double parameter: {error}"))
                })?;
                rusqlite::types::Value::Real(f64::from_bits(bits))
            }
            Some("bigint") => rusqlite::types::Value::Integer(
                map.get("value")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        VmError::InvalidState(String::from(
                            "sqlite bigint parameter missing string value",
                        ))
                    })?
                    .parse::<i64>()
                    .map_err(|error| {
                        VmError::InvalidState(format!(
                            "sqlite bigint parameter is not a signed 64-bit integer: {error}"
                        ))
                    })?,
            ),
            Some("uint8array") => rusqlite::types::Value::Blob(
                base64::engine::general_purpose::STANDARD
                    .decode(map.get("value").and_then(Value::as_str).ok_or_else(|| {
                        VmError::InvalidState(String::from(
                            "sqlite blob parameter missing base64 value",
                        ))
                    })?)
                    .map_err(|error| {
                        VmError::InvalidState(format!(
                            "sqlite blob parameter contains invalid base64: {error}"
                        ))
                    })?,
            ),
            Some(other) => {
                return Err(VmError::InvalidState(format!(
                    "unsupported sqlite tagged parameter type {other}"
                )));
            }
            None => {
                return Err(VmError::InvalidState(String::from(
                    "sqlite named parameter objects must be passed as the top-level params object",
                )));
            }
        },
    })
}

pub(in crate::execution) fn close_sqlite_database(
    kernel: &mut SidecarKernel,
    process: &mut ActiveProcess,
    database_id: u64,
    process_exited: bool,
) -> Result<(), VmError> {
    let mut database = process
        .sqlite_databases
        .remove(&database_id)
        .ok_or_else(|| {
            VmError::InvalidState(format!("sqlite database handle not found: {database_id}"))
        })?;
    process
        .sqlite_statements
        .retain(|_, statement| statement.database_id != database_id);
    let host_path = database.host_path.clone();
    let writable_sibling_database_id = host_path.as_ref().and_then(|closed_path| {
        process
            .sqlite_databases
            .iter()
            .find(|(_, database)| {
                database.host_path.as_ref() == Some(closed_path) && !database.read_only
            })
            .map(|(database_id, _)| *database_id)
    });
    let host_path_still_open = host_path.as_ref().is_some_and(|closed_path| {
        process
            .sqlite_databases
            .values()
            .any(|database| database.host_path.as_ref() == Some(closed_path))
    });
    let sync_result = if database.dirty && database.transaction_depth == 0 {
        if let Some(sibling_database_id) = writable_sibling_database_id {
            process
                .sqlite_databases
                .get_mut(&sibling_database_id)
                .expect("writable sqlite sibling")
                .dirty = true;
            Ok(())
        } else {
            // A read-only sibling cannot own a later snapshot. Persist committed
            // changes from this writable handle before dropping it, even though
            // the shared host database remains open for readers.
            sqlite_sync_database(kernel, process.kernel_pid, &mut database, process_exited)
        }
    } else if !host_path_still_open {
        sqlite_sync_database(kernel, process.kernel_pid, &mut database, process_exited)
    } else {
        Ok(())
    };
    if let Err(error) = sync_result {
        // A DatabaseSync.close() can race process exit: the process identity is
        // no longer valid for the live per-process VFS write even though its
        // bridge RPC is still draining. Retain the handle so process teardown
        // can retry copy-back with its already-authorized VM teardown path.
        process.sqlite_databases.insert(database_id, database);
        return Err(error);
    }
    drop(database);
    if !host_path_still_open {
        cleanup_sqlite_host_artifacts(host_path.as_deref())?;
    }
    Ok(())
}

pub(in crate::execution) fn ensure_per_process_state_handle_capacity(
    len: usize,
    label: &str,
) -> Result<(), VmError> {
    if len >= MAX_PER_PROCESS_STATE_HANDLES {
        return Err(VmError::InvalidState(format!(
            "{label} handle limit exceeded: limit is {MAX_PER_PROCESS_STATE_HANDLES}"
        )));
    }
    Ok(())
}

// Result admission can fail after SQLite has committed a RETURNING statement.
// Persist the connection's real state before propagating that original refusal.
fn publish_sqlite_query_result<T>(
    kernel: &mut SidecarKernel,
    kernel_pid: u32,
    database: &mut ActiveSqliteDatabase,
    sql: &str,
    result: Result<T, VmError>,
) -> Result<T, VmError> {
    mark_sqlite_mutation(database, sql);
    let publication = sqlite_sync_database(kernel, kernel_pid, database, false);
    match (result, publication) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(publication_error)) => {
            // Keep the typed operation error and expose the independent failure;
            // dirty stays set so a later authorized operation can retry publication.
            tracing::error!(%error, %publication_error, "SQLite committed mutation publication failed after query refusal");
            Err(error)
        }
    }
}

fn sqlite_sync_database(
    kernel: &mut SidecarKernel,
    kernel_pid: u32,
    database: &mut ActiveSqliteDatabase,
    process_exited: bool,
) -> Result<(), VmError> {
    if !database.dirty
        || database.transaction_depth > 0
        || database.read_only
        || database.host_path.is_none()
        || database.vm_path.is_none()
    {
        return Ok(());
    }

    let (busy, wal_pages, checkpointed): (i64, i64, i64) = database
        .connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map_err(sqlite_error)?;
    // A completed checkpoint, including a non-WAL database (-1 page counts),
    // leaves the main file self-contained. SQLite operations in this VM are
    // serialized; no guest mutation can race this read. A busy reader retains
    // the existing online-backup path for committed pages still held in WAL.
    let needs_backup = busy != 0 || (wal_pages >= 0 && checkpointed < wal_pages);
    let host_path = database.host_path.as_ref().expect("sqlite host path");
    match fs::metadata(host_path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(VmError::Io(format!(
                "failed to inspect sqlite temp database {}: {error}",
                host_path.display()
            )));
        }
    }
    // The main file alone is not a consistent snapshot when the guest selected
    // WAL mode. A checkpoint can remain busy while OpenCode keeps prepared read
    // statements alive, so persisting only the main file silently loses schema
    // and session rows from the WAL. SQLite's online backup API includes committed
    // WAL pages without invalidating live statements.
    let snapshot_path = PathBuf::from(format!("{}.snapshot", host_path.display()));
    if needs_backup {
        if snapshot_path.exists() {
            fs::remove_file(&snapshot_path).map_err(|error| {
                VmError::Io(format!(
                    "failed to remove stale sqlite snapshot {}: {error}",
                    snapshot_path.display()
                ))
            })?;
        }
        let mut snapshot = SqliteConnection::open(&snapshot_path).map_err(sqlite_error)?;
        {
            let backup =
                SqliteBackup::new(&database.connection, &mut snapshot).map_err(sqlite_error)?;
            backup
                .run_to_completion(256, Duration::from_millis(1), None)
                .map_err(sqlite_error)?;
        }
        drop(snapshot);
    }
    let vm_path = database.vm_path.as_deref().expect("sqlite vm path");
    if process_exited {
        ensure_vm_parent_dir_unchecked(kernel, vm_path)?;
    } else {
        ensure_vm_parent_dir(kernel, kernel_pid, vm_path)?;
    }
    let source_path = if needs_backup { &snapshot_path } else { host_path };
    let contents = fs::read(source_path).map_err(|error| {
        VmError::Io(format!(
            "failed to read committed sqlite image {}: {error}",
            source_path.display()
        ))
    })?;
    if process_exited {
        // The process already opened this database for write while its DAC and
        // mount permissions were live. Exit cleanup runs after that process is
        // no longer a valid kernel authorization subject, so copy the completed
        // online-backup snapshot through the VM root before the VM snapshot.
        kernel.write_file(vm_path, contents).map_err(kernel_error)?;
    } else {
        kernel
            .write_file_for_process(EXECUTION_DRIVER_NAME, kernel_pid, vm_path, contents, None)
            .map_err(kernel_error)?;
    }
    if needs_backup {
        fs::remove_file(&snapshot_path).map_err(|error| {
            VmError::Io(format!(
                "failed to remove sqlite snapshot {}: {error}",
                snapshot_path.display()
            ))
        })?;
    }
    database.dirty = false;
    Ok(())
}

fn cleanup_sqlite_host_artifacts(host_path: Option<&Path>) -> Result<(), VmError> {
    let Some(host_path) = host_path else {
        return Ok(());
    };
    let parent = host_path.parent().map(PathBuf::from);
    for suffix in ["", "-wal", "-shm", ".snapshot"] {
        let path = PathBuf::from(format!("{}{}", host_path.display(), suffix));
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(VmError::Io(format!(
                    "failed to remove sqlite temp artifact {}: {error}",
                    path.display()
                )));
            }
        }
    }
    if let Some(parent) = parent {
        if let Err(error) = fs::remove_dir_all(&parent) {
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err(VmError::Io(format!(
                    "failed to remove sqlite temp directory {}: {error}",
                    parent.display()
                )));
            }
        }
    }
    Ok(())
}

fn ensure_vm_parent_dir(
    kernel: &mut SidecarKernel,
    kernel_pid: u32,
    path: &str,
) -> Result<(), VmError> {
    let parent = dirname(path);
    if parent == "/" || parent == "." {
        return Ok(());
    }
    let mut current = String::new();
    for segment in parent.split('/').filter(|segment| !segment.is_empty()) {
        current.push('/');
        current.push_str(segment);
        if !kernel
            .exists_for_process(EXECUTION_DRIVER_NAME, kernel_pid, &current)
            .map_err(kernel_error)?
        {
            kernel
                .mkdir_for_process(EXECUTION_DRIVER_NAME, kernel_pid, &current, false, None)
                .map_err(kernel_error)?;
        }
    }
    Ok(())
}

fn ensure_vm_parent_dir_unchecked(kernel: &mut SidecarKernel, path: &str) -> Result<(), VmError> {
    let parent = dirname(path);
    if parent == "/" || parent == "." {
        return Ok(());
    }
    let mut current = String::new();
    for segment in parent.split('/').filter(|segment| !segment.is_empty()) {
        current.push('/');
        current.push_str(segment);
        if !kernel.exists(&current).map_err(kernel_error)? {
            kernel.create_dir(&current).map_err(kernel_error)?;
        }
    }
    Ok(())
}

fn sqlite_database(
    process: &ActiveProcess,
    database_id: u64,
) -> Result<&ActiveSqliteDatabase, VmError> {
    process.sqlite_databases.get(&database_id).ok_or_else(|| {
        VmError::InvalidState(format!("sqlite database handle not found: {database_id}"))
    })
}

fn sqlite_database_mut(
    process: &mut ActiveProcess,
    database_id: u64,
) -> Result<&mut ActiveSqliteDatabase, VmError> {
    process
        .sqlite_databases
        .get_mut(&database_id)
        .ok_or_else(|| {
            VmError::InvalidState(format!("sqlite database handle not found: {database_id}"))
        })
}

fn sqlite_statement(
    process: &ActiveProcess,
    statement_id: u64,
) -> Result<&ActiveSqliteStatement, VmError> {
    process.sqlite_statements.get(&statement_id).ok_or_else(|| {
        VmError::InvalidState(format!("sqlite statement handle not found: {statement_id}"))
    })
}

fn sqlite_statement_mut(
    process: &mut ActiveProcess,
    statement_id: u64,
) -> Result<&mut ActiveSqliteStatement, VmError> {
    process
        .sqlite_statements
        .get_mut(&statement_id)
        .ok_or_else(|| {
            VmError::InvalidState(format!("sqlite statement handle not found: {statement_id}"))
        })
}

fn mark_sqlite_mutation(database: &mut ActiveSqliteDatabase, sql: &str) {
    // SQLite owns transaction state, including scripts, comments and savepoints.
    // Prefix counting loses COMMITs inside scripts and RELEASE without SAVEPOINT.
    database.transaction_depth = usize::from(!database.connection.is_autocommit());
    let readonly = database
        .connection
        .prepare(sql)
        .map(|statement| statement.readonly())
        .unwrap_or(false);
    if !readonly {
        database.dirty = true;
    }
}

fn sqlite_option_bool(options: Option<&Value>, key: &str) -> Option<bool> {
    options
        .and_then(|value| value.get(key))
        .and_then(Value::as_bool)
}

fn sqlite_option_u64(options: Option<&Value>, key: &str) -> Option<u64> {
    options
        .and_then(|value| value.get(key))
        .and_then(Value::as_u64)
}

fn sqlite_error(error: rusqlite::Error) -> VmError {
    let (sqlite, message) = match &error {
        rusqlite::Error::SqliteFailure(code, message) => (code, message.clone()),
        rusqlite::Error::SqlInputError { error, msg, .. } => (error, Some(msg.clone())),
        _ => return VmError::host("ERR_SQLITE_ERROR", error.to_string()),
    };
    let description = sqlite_result_description(sqlite.extended_code);
    VmError::Host(
        crate::executor::backend::HostServiceError::new(
            "ERR_SQLITE_ERROR",
            message.unwrap_or_else(|| description.to_owned()),
        )
        .with_details(json!({ "errcode": sqlite.extended_code, "errstr": description })),
    )
}

// sqlite3_errstr's stable result descriptions. Keep this pure so the VM crate
// retains forbid(unsafe_code); bundled SQLite owns the extended numeric codes.
fn sqlite_result_description(code: i32) -> &'static str {
    match code {
        rusqlite::ffi::SQLITE_ABORT_ROLLBACK => "abort due to ROLLBACK",
        rusqlite::ffi::SQLITE_ROW => "another row available",
        rusqlite::ffi::SQLITE_DONE => "no more rows available",
        _ => match code & 0xff {
            0 => "not an error",
            1 => "SQL logic error",
            3 => "access permission denied",
            4 => "query aborted",
            5 => "database is locked",
            6 => "database table is locked",
            7 => "out of memory",
            8 => "attempt to write a readonly database",
            9 => "interrupted",
            10 => "disk I/O error",
            11 => "database disk image is malformed",
            12 => "unknown operation",
            13 => "database or disk is full",
            14 => "unable to open database file",
            15 => "locking protocol",
            17 => "database schema has changed",
            18 => "string or blob too big",
            19 => "constraint failed",
            20 => "datatype mismatch",
            21 => "bad parameter or other API misuse",
            23 => "authorization denied",
            25 => "column index out of range",
            26 => "file is not a database",
            27 => "notification message",
            28 => "warning message",
            _ => "unknown error",
        },
    }
}
