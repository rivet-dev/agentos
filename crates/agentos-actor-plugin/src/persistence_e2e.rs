//! End-to-end test of the durable-persistence layer against a REAL SQLite.
//!
//! Drives the actual `persistence::handle_fs_call` dispatch (the storage
//! callback the VM's `sqlite_vfs` root invokes) through a mock `HostVtable`
//! whose `db_*` functions execute against an in-memory rusqlite `Connection`,
//! speaking the exact CBOR `db_*` wire contract the plugin uses. Most tests use
//! no VM or sidecar and isolate the durable-storage core (the 24 fs ops, the
//! migration, base64 content, and CBOR params/rows marshalling). The cron
//! cold-boot test intentionally launches a real sidecar and VM to prove opaque
//! scheduler-state restoration across teardown and reboot.

use std::ffi::c_void;
use std::io::Cursor;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rivet_actor_plugin_abi as abi;
use rusqlite::types::Value as SqlValue;
use rusqlite::Connection;
use serde_json::{json, Map, Value as JsonValue};

use crate::host_ctx::HostCtx;
use crate::persistence;

/// Mock host state: the actor's SQLite database.
struct MockHost {
    conn: Mutex<Connection>,
    scheduled: Mutex<Vec<abi::ScheduleActionRequest>>,
    refs: AtomicIsize,
}

extern "C" fn ctx_clone(ctx: *const c_void) -> *const c_void {
    host_of(ctx).refs.fetch_add(1, Ordering::SeqCst);
    ctx
}
extern "C" fn ctx_release(ctx: *const c_void) {
    host_of(ctx).refs.fetch_sub(1, Ordering::SeqCst);
}
extern "C" fn sql_is_enabled(_ctx: *const c_void) -> u8 {
    1
}

// Unused-by-persistence vtable stubs.
extern "C" fn next_event(_ctx: *const c_void, done: abi::CompletionFn, ud: *mut c_void) {
    done(ud, abi::AbiResult::channel_closed());
}
extern "C" fn reply_ok(_c: *const c_void, _t: u64, _p: abi::OwnedBuf) -> abi::AbiStatus {
    abi::AbiStatus::Ok
}
extern "C" fn reply_err(_c: *const c_void, _t: u64, _p: abi::OwnedBuf) -> abi::AbiStatus {
    abi::AbiStatus::Ok
}
extern "C" fn startup_ready(_c: *const c_void, _ok: u8, _e: abi::BorrowedBuf) {}
extern "C" fn broadcast(_c: *const c_void, _n: abi::OwnedBuf, _p: abi::OwnedBuf) -> abi::AbiStatus {
    abi::AbiStatus::Ok
}
extern "C" fn log(_c: *const c_void, _level: i32, _msg: abi::BorrowedBuf) {}
extern "C" fn state_get(_c: *const c_void) -> abi::OwnedBuf {
    abi::OwnedBuf::empty()
}
extern "C" fn state_set(_c: *const c_void, state: abi::OwnedBuf) -> abi::AbiStatus {
    unsafe {
        state.free_self();
    }
    abi::AbiStatus::Ok
}
extern "C" fn actor_identity(_c: *const c_void) -> abi::OwnedBuf {
    abi::OwnedBuf::empty()
}
extern "C" fn state_save(
    _c: *const c_void,
    state: abi::OwnedBuf,
    done: abi::CompletionFn,
    ud: *mut c_void,
) {
    unsafe {
        state.free_self();
    }
    done(ud, abi::AbiResult::ok(abi::OwnedBuf::empty()));
}
extern "C" fn request_save(
    _c: *const c_void,
    _immediate: u8,
    _has_max_wait: u8,
    _max_wait_ms: u32,
) -> abi::AbiStatus {
    abi::AbiStatus::Ok
}
extern "C" fn request_save_and_wait(
    _c: *const c_void,
    _immediate: u8,
    _has_max_wait: u8,
    _max_wait_ms: u32,
    done: abi::CompletionFn,
    ud: *mut c_void,
) {
    done(ud, abi::AbiResult::ok(abi::OwnedBuf::empty()));
}
extern "C" fn sleep(_c: *const c_void) -> abi::AbiResult {
    abi::AbiResult::status_only(abi::AbiStatus::Cancelled)
}
extern "C" fn actor_aborted(_c: *const c_void) -> u8 {
    0
}
extern "C" fn wait_actor_abort(_c: *const c_void, done: abi::CompletionFn, ud: *mut c_void) {
    done(ud, abi::AbiResult::ok(abi::OwnedBuf::empty()));
}
extern "C" fn keep_awake_enter(_c: *const c_void) -> abi::AbiResult {
    abi::AbiResult::status_only(abi::AbiStatus::Cancelled)
}
extern "C" fn keep_awake_exit(_c: *const c_void, _token: u64) -> abi::AbiStatus {
    abi::AbiStatus::Ok
}
extern "C" fn keep_awake_count(_c: *const c_void) -> u64 {
    0
}
extern "C" fn async_unavailable(
    _c: *const c_void,
    request: abi::OwnedBuf,
    done: abi::CompletionFn,
    ud: *mut c_void,
) {
    unsafe {
        request.free_self();
    }
    done(
        ud,
        abi::AbiResult::err(abi::OwnedBuf::from_vec(
            b"not available in persistence test".to_vec(),
        )),
    );
}
extern "C" fn schedule_at(
    ctx: *const c_void,
    request: abi::OwnedBuf,
    done: abi::CompletionFn,
    ud: *mut c_void,
) {
    let bytes = unsafe { request.into_vec() };
    let request: abi::ScheduleActionRequest =
        ciborium::from_reader(Cursor::new(bytes)).expect("decode schedule_at request");
    host_of(ctx).scheduled.lock().unwrap().push(request);
    done(ud, abi::AbiResult::ok(abi::OwnedBuf::empty()));
}
extern "C" fn hibernatable_ws_ack(
    _c: *const c_void,
    gateway_id: abi::OwnedBuf,
    request_id: abi::OwnedBuf,
    _server_message_index: u16,
) -> abi::AbiResult {
    unsafe {
        gateway_id.free_self();
        request_id.free_self();
    }
    abi::AbiResult::status_only(abi::AbiStatus::Cancelled)
}
extern "C" fn conn_send(_c: *const c_void, request: abi::OwnedBuf) -> abi::AbiResult {
    unsafe {
        request.free_self();
    }
    abi::AbiResult::status_only(abi::AbiStatus::Cancelled)
}

fn host_of<'a>(ctx: *const c_void) -> &'a MockHost {
    unsafe { &*(ctx as *const MockHost) }
}

/// CBOR `[v1, v2, ...]` (the plugin's `cbor_params`) → rusqlite bind values.
fn decode_params(bytes: &[u8]) -> Vec<SqlValue> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let value: JsonValue = ciborium::from_reader(Cursor::new(bytes)).expect("decode params cbor");
    let JsonValue::Array(items) = value else {
        return Vec::new();
    };
    items
        .into_iter()
        .map(|v| match v {
            JsonValue::Null => SqlValue::Null,
            JsonValue::Bool(b) => SqlValue::Integer(b as i64),
            JsonValue::String(s) => SqlValue::Text(s),
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    SqlValue::Integer(i)
                } else {
                    SqlValue::Real(n.as_f64().unwrap_or(0.0))
                }
            }
            other => SqlValue::Text(other.to_string()),
        })
        .collect()
}

fn sql_to_json(v: SqlValue) -> JsonValue {
    match v {
        SqlValue::Null => JsonValue::Null,
        SqlValue::Integer(i) => json!(i),
        SqlValue::Real(f) => json!(f),
        SqlValue::Text(s) => JsonValue::String(s),
        SqlValue::Blob(b) => JsonValue::String(BASE64.encode(b)),
    }
}

extern "C" fn db_exec(
    ctx: *const c_void,
    sql: abi::OwnedBuf,
    done: abi::CompletionFn,
    ud: *mut c_void,
) {
    let host = host_of(ctx);
    let sql = String::from_utf8(unsafe { sql.into_vec() }).expect("utf8 sql");
    let conn = host.conn.lock().unwrap();
    match conn.execute_batch(&sql) {
        Ok(()) => done(ud, abi::AbiResult::ok(abi::OwnedBuf::from_vec(Vec::new()))),
        Err(e) => done(
            ud,
            abi::AbiResult::err(abi::OwnedBuf::from_vec(e.to_string().into_bytes())),
        ),
    }
}

fn run_query(host: &MockHost, sql: &str, params: Vec<SqlValue>) -> rusqlite::Result<Vec<u8>> {
    let conn = host.conn.lock().unwrap();
    let mut stmt = conn.prepare(sql)?;
    let col_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let ncols = col_names.len();
    let rows = stmt.query_map(rusqlite::params_from_iter(params), move |row| {
        let mut obj = Map::new();
        for (i, name) in col_names.iter().enumerate().take(ncols) {
            let v: SqlValue = row.get(i)?;
            obj.insert(name.clone(), sql_to_json(v));
        }
        Ok(JsonValue::Object(obj))
    })?;
    let collected: Vec<JsonValue> = rows.collect::<rusqlite::Result<_>>()?;
    let mut buf = Vec::new();
    ciborium::into_writer(&JsonValue::Array(collected), &mut buf).expect("encode rows cbor");
    Ok(buf)
}

extern "C" fn db_query(
    ctx: *const c_void,
    sql: abi::OwnedBuf,
    params: abi::OwnedBuf,
    done: abi::CompletionFn,
    ud: *mut c_void,
) {
    let host = host_of(ctx);
    let sql = String::from_utf8(unsafe { sql.into_vec() }).expect("utf8 sql");
    let params = decode_params(&unsafe { params.into_vec() });
    match run_query(host, &sql, params) {
        Ok(bytes) => done(ud, abi::AbiResult::ok(abi::OwnedBuf::from_vec(bytes))),
        Err(e) => done(
            ud,
            abi::AbiResult::err(abi::OwnedBuf::from_vec(e.to_string().into_bytes())),
        ),
    }
}

extern "C" fn db_run(
    ctx: *const c_void,
    sql: abi::OwnedBuf,
    params: abi::OwnedBuf,
    done: abi::CompletionFn,
    ud: *mut c_void,
) {
    let host = host_of(ctx);
    let sql = String::from_utf8(unsafe { sql.into_vec() }).expect("utf8 sql");
    let params = decode_params(&unsafe { params.into_vec() });
    let conn = host.conn.lock().unwrap();
    match conn.execute(&sql, rusqlite::params_from_iter(params)) {
        Ok(_) => done(ud, abi::AbiResult::ok(abi::OwnedBuf::from_vec(Vec::new()))),
        Err(e) => done(
            ud,
            abi::AbiResult::err(abi::OwnedBuf::from_vec(e.to_string().into_bytes())),
        ),
    }
}

fn mock_host_ctx(host: &MockHost) -> HostCtx {
    let vtable = abi::HostVtable {
        abi_version: abi::RIVET_ACTOR_ABI_VERSION,
        ctx: host as *const MockHost as *const c_void,
        ctx_clone,
        ctx_release,
        db_exec,
        db_query,
        db_run,
        sql_is_enabled,
        state_get,
        state_set,
        actor_identity,
        state_save,
        request_save,
        request_save_and_wait,
        sleep,
        actor_aborted,
        wait_actor_abort,
        keep_awake_enter,
        keep_awake_exit,
        keep_awake_count,
        kv_get: async_unavailable,
        kv_put: async_unavailable,
        kv_delete: async_unavailable,
        kv_batch_get: async_unavailable,
        kv_batch_put: async_unavailable,
        kv_batch_delete: async_unavailable,
        kv_delete_range: async_unavailable,
        kv_list_prefix: async_unavailable,
        kv_list_range: async_unavailable,
        schedule_after: async_unavailable,
        schedule_at,
        set_alarm: async_unavailable,
        scheduled_events: async_unavailable,
        conn_list: async_unavailable,
        conn_disconnect: async_unavailable,
        hibernatable_ws_ack,
        conn_send,
        next_event,
        reply_ok,
        reply_err,
        startup_ready,
        broadcast,
        log,
    };
    HostCtx::from_vtable(vtable)
}

#[tokio::test]
async fn persistence_round_trips_fs_ops_against_real_sqlite() {
    let host_state = MockHost {
        conn: Mutex::new(Connection::open_in_memory().expect("open sqlite")),
        scheduled: Mutex::new(Vec::new()),
        refs: AtomicIsize::new(1),
    };

    {
        let host = mock_host_ctx(&host_state);
        assert_eq!(host_state.refs.load(Ordering::SeqCst), 2);

        // Schema migration (real MIGRATION_SQL through db_exec).
        persistence::migrate(&host).await.expect("migrate");

        let path = "/work/hello.txt";
        let content = b"hello durable world";
        let content_b64 = BASE64.encode(content);

        // mkdir -p /work, then writeFile.
        persistence::handle_fs_call(
            &host,
            "mkdir",
            &json!({ "path": "/work", "recursive": true }),
        )
        .await
        .expect("mkdir");
        persistence::handle_fs_call(
            &host,
            "writeFile",
            &json!({ "path": path, "content": content_b64 }),
        )
        .await
        .expect("writeFile");

        // exists → true
        let exists = persistence::handle_fs_call(&host, "exists", &json!({ "path": path }))
            .await
            .expect("exists");
        assert_eq!(exists, Some(JsonValue::Bool(true)), "file should exist");

        // readFile → the same base64 content (round-trip through SQLite).
        let read = persistence::handle_fs_call(&host, "readFile", &json!({ "path": path }))
            .await
            .expect("readFile");
        assert_eq!(
            read,
            Some(JsonValue::String(content_b64.clone())),
            "readFile must return the stored content"
        );

        // stat → object reporting the decoded byte length.
        let stat = persistence::handle_fs_call(&host, "stat", &json!({ "path": path }))
            .await
            .expect("stat")
            .expect("stat returns an object");
        assert_eq!(
            stat.get("size").and_then(JsonValue::as_i64),
            Some(content.len() as i64),
            "stat size must equal the decoded content length"
        );

        // readDir of /work lists the file.
        let entries = persistence::handle_fs_call(&host, "readDir", &json!({ "path": "/work" }))
            .await
            .expect("readDir")
            .expect("readDir array");
        let names: Vec<&str> = entries
            .as_array()
            .unwrap()
            .iter()
            .filter_map(JsonValue::as_str)
            .collect();
        assert!(
            names.contains(&"hello.txt"),
            "readDir should list hello.txt, got {names:?}"
        );

        // pread must return the real byte range. Regression: pread used to
        // read `entry.content` off the metadata-only lookup (`NULL AS
        // content`), so every guest WASM read of a mount-backed root file
        // came back empty (`cat` printed nothing, `wc -c` reported 0).
        let pread = persistence::handle_fs_call(
            &host,
            "pread",
            &json!({ "path": path, "offset": 6, "len": 7 }),
        )
        .await
        .expect("pread");
        assert_eq!(
            pread,
            Some(JsonValue::String(BASE64.encode(&content[6..13]))),
            "pread must return the stored bytes, not the metadata-only view"
        );

        // Hard link must copy real content (same metadata-only-lookup bug).
        let link_path = "/work/hello-link.txt";
        persistence::handle_fs_call(
            &host,
            "link",
            &json!({ "oldPath": path, "newPath": link_path }),
        )
        .await
        .expect("link");
        let link_read =
            persistence::handle_fs_call(&host, "readFile", &json!({ "path": link_path }))
                .await
                .expect("readFile link");
        assert_eq!(
            link_read,
            Some(JsonValue::String(content_b64.clone())),
            "hard link must carry the original content"
        );

        // Truncate must preserve the retained prefix (it used to zero-fill
        // from the metadata-only empty content).
        persistence::handle_fs_call(&host, "truncate", &json!({ "path": path, "len": 5 }))
            .await
            .expect("truncate");
        let truncated = persistence::handle_fs_call(&host, "readFile", &json!({ "path": path }))
            .await
            .expect("readFile after truncate");
        assert_eq!(
            truncated,
            Some(JsonValue::String(BASE64.encode(&content[..5]))),
            "truncate must keep the retained bytes intact"
        );

        // removeFile → exists is now false.
        persistence::handle_fs_call(&host, "removeFile", &json!({ "path": path }))
            .await
            .expect("removeFile");
        let exists_after = persistence::handle_fs_call(&host, "exists", &json!({ "path": path }))
            .await
            .expect("exists after remove");
        assert_eq!(
            exists_after,
            Some(JsonValue::Bool(false)),
            "file should be gone after removeFile"
        );
    }

    assert_eq!(host_state.refs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn persistence_stores_cron_state_as_an_opaque_value() {
    let host_state = MockHost {
        conn: Mutex::new(Connection::open_in_memory().expect("open sqlite")),
        scheduled: Mutex::new(Vec::new()),
        refs: AtomicIsize::new(1),
    };

    {
        let host = mock_host_ctx(&host_state);
        persistence::migrate(&host).await.expect("migrate");
        let state = r#"{"version":1,"sidecarOwned":true}"#;
        persistence::save_cron_state(&host, state)
            .await
            .expect("save cron state");
        assert_eq!(
            persistence::load_cron_state(&host)
                .await
                .expect("load cron state")
                .as_deref(),
            Some(state)
        );

        persistence::delete_cron_state(&host)
            .await
            .expect("delete cron state");
        assert_eq!(
            persistence::load_cron_state(&host)
                .await
                .expect("load deleted cron state"),
            None
        );

        let args = vec![1, 2, 3];
        host.schedule_at(
            1_700_000_000_123,
            crate::actions::cron::INTERNAL_CRON_WAKE_ACTION.to_string(),
            args.clone(),
        )
        .await
        .expect("schedule durable cron wake");
        let scheduled = host_state.scheduled.lock().unwrap();
        assert_eq!(scheduled.len(), 1);
        assert_eq!(scheduled[0].timestamp_ms, Some(1_700_000_000_123));
        assert_eq!(scheduled[0].delay_ms, None);
        assert_eq!(
            scheduled[0].action_name,
            crate::actions::cron::INTERNAL_CRON_WAKE_ACTION
        );
        assert_eq!(scheduled[0].args, args);
    }

    assert_eq!(host_state.refs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn actor_cold_boot_restores_sidecar_owned_cron_state() {
    let sidecar_path = std::env::var("AGENTOS_SIDECAR_BIN")
        .expect("actor cold-boot cron E2E requires AGENTOS_SIDECAR_BIN");
    let sidecar_file = std::path::Path::new(&sidecar_path);
    assert!(
        sidecar_file.is_file(),
        "AGENTOS_SIDECAR_BIN does not point to a file: {}",
        sidecar_file.display()
    );
    let host_state = MockHost {
        conn: Mutex::new(Connection::open_in_memory().expect("open sqlite")),
        scheduled: Mutex::new(Vec::new()),
        refs: AtomicIsize::new(1),
    };

    {
        let host = mock_host_ctx(&host_state);
        persistence::migrate(&host).await.expect("migrate");
        let config = crate::config::AgentOsConfigJson::default();
        let pool = format!("actor-cron-restore-test-{}", uuid::Uuid::new_v4());
        let mut vm = None;
        crate::vm::ensure_vm(&host, &sidecar_path, &config, &pool, &mut vm)
            .await
            .expect("boot first VM");
        let first = vm.as_ref().expect("first VM");
        first
            .schedule_cron(agentos_client::CronJobOptions {
                id: Some("survives-sleep".to_string()),
                schedule: (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
                action: agentos_client::CronAction::Exec {
                    command: "true".to_string(),
                    args: Vec::new(),
                },
                overlap: None,
            })
            .await
            .expect("schedule durable job");
        crate::vm::persist_cron_state(&host, first)
            .await
            .expect("persist before sleep");
        let first_sidecar = first.sidecar();
        crate::vm::shutdown_vm(&host, &mut vm, "sleep").await;
        assert!(vm.is_none(), "sleep must drop the first VM handle");
        let first_description = first_sidecar.describe();
        assert_eq!(first_description.active_vm_count, 0);
        assert_eq!(
            first_description.state,
            agentos_client::SidecarState::Disposed,
            "sleep must dispose the one-VM sidecar before cold boot"
        );

        crate::vm::ensure_vm(&host, &sidecar_path, &config, &pool, &mut vm)
            .await
            .expect("cold boot restored VM");
        let restored = vm.as_ref().expect("restored VM");
        let restored_sidecar = restored.sidecar();
        assert!(
            !std::sync::Arc::ptr_eq(&first_sidecar, &restored_sidecar),
            "cold boot must allocate a new sidecar handle"
        );
        assert_eq!(restored_sidecar.describe().active_vm_count, 1);
        assert!(
            restored
                .list_cron_jobs()
                .await
                .expect("list restored cron jobs")
                .iter()
                .any(|job| job.id == "survives-sleep"),
            "cold actor boot must restore the sidecar-owned cron registry"
        );
        restored
            .cancel_cron_job("survives-sleep")
            .await
            .expect("cancel restored job");
        crate::vm::shutdown_vm(&host, &mut vm, "destroy").await;
        assert!(vm.is_none(), "destroy must drop the restored VM handle");
        let restored_description = restored_sidecar.describe();
        assert_eq!(restored_description.active_vm_count, 0);
        assert_eq!(
            restored_description.state,
            agentos_client::SidecarState::Disposed,
            "destroy must dispose the restored one-VM sidecar"
        );
        crate::vm::delete_cron_state(&host)
            .await
            .expect("delete persisted state");
    }

    assert_eq!(host_state.refs.load(Ordering::SeqCst), 1);
}
