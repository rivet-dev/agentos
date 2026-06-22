//! Durable-storage persistence — ported from `rivetkit-agent-os::persistence`,
//! with `HostCtx` substituted for rivetkit's `Ctx`. This is the SQL substrate
//! every fs op + the preview/session logic sits on: the schema migration and
//! the `query_rows`/`run_stmt` helpers that marshal params as CBOR JSON arrays
//! and decode rows as CBOR JSON objects (the `db_*` wire contract).
//!
//! The ~24 fs-op handlers (readFile/writeFile/readDir/stat/mkdir/rename/...) are
//! ported on top of these helpers next — each is a direct substitution of
//! `host` for `ctx`.

#![allow(dead_code)]

use std::io::Cursor;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde_json::{json, Map as JsonMap, Value as JsonValue};

use crate::host_ctx::HostCtx;

const DEFAULT_FILE_MODE: i64 = 0o100644;
const DEFAULT_DIR_MODE: i64 = 0o040755;
#[allow(dead_code)]
const DEFAULT_SYMLINK_MODE: i64 = 0o120777;

pub(crate) const MIGRATION_SQL: &str = "\
CREATE TABLE IF NOT EXISTS agent_os_preview_tokens (
	token TEXT PRIMARY KEY,
	port INTEGER NOT NULL,
	created_at INTEGER NOT NULL,
	expires_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_preview_tokens_expires_at
	ON agent_os_preview_tokens(expires_at);
CREATE TABLE IF NOT EXISTS agent_os_fs_entries (
	path TEXT PRIMARY KEY,
	is_directory INTEGER NOT NULL DEFAULT 0,
	content BLOB,
	mode INTEGER NOT NULL DEFAULT 33188,
	uid INTEGER NOT NULL DEFAULT 0,
	gid INTEGER NOT NULL DEFAULT 0,
	size INTEGER NOT NULL DEFAULT 0,
	atime_ms INTEGER NOT NULL,
	mtime_ms INTEGER NOT NULL,
	ctime_ms INTEGER NOT NULL,
	birthtime_ms INTEGER NOT NULL,
	symlink_target TEXT,
	nlink INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_fs_entries_parent
	ON agent_os_fs_entries(path);
CREATE TABLE IF NOT EXISTS agent_os_sessions (
	session_id TEXT PRIMARY KEY,
	agent_type TEXT NOT NULL,
	capabilities TEXT NOT NULL,
	agent_info TEXT,
	created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS agent_os_session_events (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	session_id TEXT NOT NULL,
	seq INTEGER NOT NULL,
	event TEXT NOT NULL,
	created_at INTEGER NOT NULL,
	FOREIGN KEY (session_id) REFERENCES agent_os_sessions(session_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_session_events_session_seq
	ON agent_os_session_events(session_id, seq);
";

/// Run the agent-os schema migration against the actor's SQLite database.
/// Idempotent; called once at the top of the actor run loop.
pub(crate) async fn migrate(host: &HostCtx) -> Result<()> {
    host.db_exec(MIGRATION_SQL.as_bytes().to_vec())
        .await
        .map_err(|e| anyhow!("agent-os schema migration failed: {e}"))?;
    Ok(())
}

/// Encode positional bind params as the CBOR JSON array the `db_*` API expects.
pub(crate) fn cbor_params(values: &[JsonValue]) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    ciborium::into_writer(&JsonValue::Array(values.to_vec()), &mut buf)?;
    Ok(buf)
}

/// Decode a `db_query` CBOR result into object rows (column -> value).
pub(crate) fn decode_rows(bytes: &[u8]) -> Result<Vec<serde_json::Map<String, JsonValue>>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let value: JsonValue = ciborium::from_reader(Cursor::new(bytes))?;
    Ok(match value {
        JsonValue::Array(rows) => rows
            .into_iter()
            .filter_map(|row| match row {
                JsonValue::Object(map) => Some(map),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    })
}

/// Run a parameterized query and return decoded object rows.
pub(crate) async fn query_rows(
    host: &HostCtx,
    sql: &str,
    params: &[JsonValue],
) -> Result<Vec<serde_json::Map<String, JsonValue>>> {
    let encoded = cbor_params(params)?;
    let bytes = host
        .db_query(sql.as_bytes().to_vec(), encoded)
        .await
        .map_err(|e| anyhow!(e))?;
    decode_rows(&bytes)
}

/// Run a parameterized statement that returns no rows (INSERT/UPDATE/DELETE).
pub(crate) async fn run_stmt(host: &HostCtx, sql: &str, params: &[JsonValue]) -> Result<()> {
    let encoded = cbor_params(params)?;
    host.db_run(sql.as_bytes().to_vec(), encoded)
        .await
        .map_err(|e| anyhow!(e))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// sqlite_vfs filesystem dispatch (ported from rivetkit-agent-os::persistence,
// `ctx` -> `host`). This batch implements the read path; the write/rare ops
// are ported on top of the same helpers next.
// ---------------------------------------------------------------------------

/// Dispatch a guest fs op to actor durable storage. Returns the op's JSON
/// result (or `None` for void ops). Ops not yet ported return `ENOSYS`.
pub(crate) async fn handle_fs_call(
    host: &HostCtx,
    operation: &str,
    args: &JsonValue,
) -> Result<Option<JsonValue>> {
    ensure_fs_root(host).await?;
    match operation {
        "readFile" => Ok(Some(json!(read_file(host, required_path(args)?).await?))),
        "writeFile" => {
            write_file(
                host,
                required_path(args)?,
                required_string(args, "content")?,
                optional_i64(args, "mode").unwrap_or(DEFAULT_FILE_MODE),
            )
            .await?;
            Ok(None)
        }
        "createFileExclusive" => {
            create_file_exclusive(
                host,
                required_path(args)?,
                required_string(args, "content")?,
                optional_i64(args, "mode").unwrap_or(DEFAULT_FILE_MODE),
            )
            .await?;
            Ok(None)
        }
        "readDir" => Ok(Some(JsonValue::Array(
            read_dir(host, required_path(args)?)
                .await?
                .into_iter()
                .map(JsonValue::String)
                .collect(),
        ))),
        "readDirWithTypes" => Ok(Some(JsonValue::Array(
            read_dir_entries(host, required_path(args)?)
                .await?
                .into_iter()
                .map(|entry| {
                    json!({
                        "name": entry.name,
                        "isDirectory": entry.is_directory,
                        "isSymbolicLink": entry.symlink_target.is_some(),
                    })
                })
                .collect(),
        ))),
        "createDir" => {
            create_dir(
                host,
                required_path(args)?,
                optional_i64(args, "mode").unwrap_or(DEFAULT_DIR_MODE),
                false,
            )
            .await?;
            Ok(None)
        }
        "mkdir" => {
            let recursive = args
                .get("recursive")
                .and_then(JsonValue::as_bool)
                .unwrap_or(false);
            create_dir(
                host,
                required_path(args)?,
                optional_i64(args, "mode").unwrap_or(DEFAULT_DIR_MODE),
                recursive,
            )
            .await?;
            Ok(None)
        }
        "exists" => Ok(Some(json!(lookup_entry(host, required_path(args)?)
            .await?
            .is_some()))),
        "stat" => Ok(Some(stat_json(
            lookup_entry_required(host, required_path(args)?).await?,
        ))),
        "lstat" => Ok(Some(stat_json(
            lookup_entry_required(host, required_path(args)?).await?,
        ))),
        "realpath" => Ok(Some(json!(normalize_path(required_path(args)?)?))),
        "removeFile" => {
            remove_file(host, required_path(args)?).await?;
            Ok(None)
        }
        "removeDir" => {
            remove_dir(host, required_path(args)?).await?;
            Ok(None)
        }
        "rename" => {
            rename_entry(
                host,
                required_string(args, "oldPath")?,
                required_string(args, "newPath")?,
            )
            .await?;
            Ok(None)
        }
        "symlink" => {
            symlink_entry(
                host,
                required_string(args, "target")?,
                required_string(args, "path")?,
            )
            .await?;
            Ok(None)
        }
        "readLink" => Ok(Some(json!(read_link(host, required_path(args)?).await?))),
        "link" => {
            link_entry(
                host,
                required_string(args, "oldPath")?,
                required_string(args, "newPath")?,
            )
            .await?;
            Ok(None)
        }
        "chmod" => {
            update_one_field(
                host,
                required_path(args)?,
                "mode",
                json!(required_i64(args, "mode")?),
            )
            .await?;
            Ok(None)
        }
        "chown" => {
            update_owner(
                host,
                required_path(args)?,
                required_i64(args, "uid")?,
                required_i64(args, "gid")?,
            )
            .await?;
            Ok(None)
        }
        "utimes" => {
            update_times(
                host,
                required_path(args)?,
                required_i64(args, "atimeMs")?,
                required_i64(args, "mtimeMs")?,
            )
            .await?;
            Ok(None)
        }
        "truncate" => {
            truncate_file(host, required_path(args)?, required_len(args)?).await?;
            Ok(None)
        }
        "pread" => Ok(Some(json!(
            pread_file(
                host,
                required_path(args)?,
                required_i64(args, "offset")?,
                required_len(args)?,
            )
            .await?
        ))),
        operation => bail!("ENOSYS unsupported sqlite_vfs operation {operation}"),
    }
}

#[derive(Clone, Debug)]
struct FsEntry {
    path: String,
    name: String,
    is_directory: bool,
    content: Option<String>,
    mode: i64,
    uid: i64,
    gid: i64,
    size: i64,
    atime_ms: i64,
    mtime_ms: i64,
    ctime_ms: i64,
    birthtime_ms: i64,
    symlink_target: Option<String>,
    nlink: i64,
}

impl FsEntry {
    fn from_row(mut row: JsonMap<String, JsonValue>) -> Result<Self> {
        let path = string_col(&mut row, "path")?;
        Ok(Self {
            name: basename(&path),
            path,
            is_directory: int_col(&mut row, "is_directory")? != 0,
            content: optional_content_col(&mut row, "content")?,
            mode: int_col(&mut row, "mode")?,
            uid: int_col(&mut row, "uid")?,
            gid: int_col(&mut row, "gid")?,
            size: int_col(&mut row, "size")?,
            atime_ms: int_col(&mut row, "atime_ms")?,
            mtime_ms: int_col(&mut row, "mtime_ms")?,
            ctime_ms: int_col(&mut row, "ctime_ms")?,
            birthtime_ms: int_col(&mut row, "birthtime_ms")?,
            symlink_target: optional_string_col(&mut row, "symlink_target")?,
            nlink: int_col(&mut row, "nlink")?,
        })
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

async fn ensure_fs_root(host: &HostCtx) -> Result<()> {
    let now = now_ms();
    run_stmt(
        host,
        "INSERT OR IGNORE INTO agent_os_fs_entries
			(path, is_directory, content, mode, uid, gid, size, atime_ms, mtime_ms, ctime_ms, birthtime_ms, symlink_target, nlink)
			VALUES (?, 1, NULL, ?, 0, 0, 0, ?, ?, ?, ?, NULL, 2)",
        &[
            json!("/"),
            json!(DEFAULT_DIR_MODE),
            json!(now),
            json!(now),
            json!(now),
            json!(now),
        ],
    )
    .await
}

async fn lookup_entry(host: &HostCtx, path: &str) -> Result<Option<FsEntry>> {
    let path = normalize_path(path)?;
    let rows = query_rows(
        host,
        "SELECT path, is_directory, content, mode, uid, gid, size, atime_ms, mtime_ms, ctime_ms, birthtime_ms, symlink_target, nlink
			FROM agent_os_fs_entries WHERE path = ?",
        &[json!(path)],
    )
    .await?;
    rows.into_iter().next().map(FsEntry::from_row).transpose()
}

async fn lookup_entry_required(host: &HostCtx, path: &str) -> Result<FsEntry> {
    lookup_entry(host, path)
        .await?
        .ok_or_else(|| anyhow!("ENOENT no such file or directory: {}", path))
}

async fn read_file(host: &HostCtx, path: &str) -> Result<String> {
    let entry = lookup_entry_required(host, path).await?;
    if entry.is_directory {
        bail!("EISDIR is a directory: {}", entry.path);
    }
    Ok(entry.content.unwrap_or_default())
}

// --- ctx-free helpers (copied verbatim from rivetkit-agent-os::persistence) ---

#[allow(dead_code)]
fn parent_path(path: &str) -> Option<String> {
    if path == "/" {
        return None;
    }
    let path = path.trim_end_matches('/');
    let index = path.rfind('/')?;
    if index == 0 {
        Some("/".to_owned())
    } else {
        Some(path[..index].to_owned())
    }
}

fn normalize_path(path: &str) -> Result<String> {
    if path.is_empty() {
        bail!("ENOENT empty path");
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            parts.pop();
            continue;
        }
        parts.push(part);
    }
    if parts.is_empty() {
        Ok("/".to_owned())
    } else {
        Ok(format!("/{}", parts.join("/")))
    }
}

fn basename(path: &str) -> String {
    if path == "/" {
        return "/".to_owned();
    }
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

fn required_path(args: &JsonValue) -> Result<&str> {
    required_string_ref(args, "path")
}

fn required_string_ref<'a>(args: &'a JsonValue, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| anyhow!("EINVAL missing string arg {key}"))
}

#[allow(dead_code)]
fn decode_content(content: &str) -> Result<Vec<u8>> {
    BASE64
        .decode(content)
        .map_err(|error| anyhow!("EINVAL invalid base64 file content: {error}"))
}

fn string_col(row: &mut JsonMap<String, JsonValue>, key: &str) -> Result<String> {
    row.remove(key)
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| anyhow!("sqlite_vfs row missing string column {key}"))
}

fn optional_string_col(row: &mut JsonMap<String, JsonValue>, key: &str) -> Result<Option<String>> {
    match row.remove(key) {
        Some(JsonValue::Null) | None => Ok(None),
        Some(JsonValue::String(value)) => Ok(Some(value)),
        Some(value) => bail!("sqlite_vfs row column {key} expected string/null, got {value:?}"),
    }
}

fn optional_content_col(row: &mut JsonMap<String, JsonValue>, key: &str) -> Result<Option<String>> {
    match row.remove(key) {
        Some(JsonValue::Null) | None => Ok(None),
        Some(JsonValue::String(value)) => Ok(Some(value)),
        Some(JsonValue::Array(bytes)) => {
            let raw = bytes
                .into_iter()
                .map(|value| {
                    value
                        .as_u64()
                        .and_then(|byte| u8::try_from(byte).ok())
                        .ok_or_else(|| {
                            anyhow!("sqlite_vfs blob column {key} contains non-byte value")
                        })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Some(String::from_utf8(raw)?))
        }
        Some(value) => {
            bail!("sqlite_vfs row column {key} expected blob/string/null, got {value:?}")
        }
    }
}

fn int_col(row: &mut JsonMap<String, JsonValue>, key: &str) -> Result<i64> {
    row.remove(key)
        .and_then(|value| value.as_i64())
        .ok_or_else(|| anyhow!("sqlite_vfs row missing integer column {key}"))
}

// --- write/mkdir/readdir op handlers (ctx -> host) ---

async fn ensure_parent_dir(host: &HostCtx, path: &str) -> Result<()> {
    let Some(parent) = parent_path(path) else {
        return Ok(());
    };
    let parent = lookup_entry_required(host, &parent).await?;
    if !parent.is_directory {
        bail!("ENOTDIR parent is not a directory: {}", parent.path);
    }
    Ok(())
}

async fn write_file(host: &HostCtx, path: &str, content: String, mode: i64) -> Result<()> {
    let path = normalize_path(path)?;
    ensure_parent_dir(host, &path).await?;
    let size = decoded_len(&content)?;
    let now = now_ms();
    if let Some(existing) = lookup_entry(host, &path).await? {
        if existing.is_directory {
            bail!("EISDIR is a directory: {path}");
        }
        run_stmt(
            host,
            "UPDATE agent_os_fs_entries
				SET is_directory = 0, content = ?, mode = ?, size = ?, mtime_ms = ?, ctime_ms = ?, symlink_target = NULL, nlink = 1
				WHERE path = ?",
            &[
                json!(content),
                json!(mode),
                json!(size),
                json!(now),
                json!(now),
                json!(path),
            ],
        )
        .await?;
        return Ok(());
    }
    insert_entry(host, &path, false, Some(content), mode, size, None, 1, now).await
}

async fn create_file_exclusive(
    host: &HostCtx,
    path: &str,
    content: String,
    mode: i64,
) -> Result<()> {
    let path = normalize_path(path)?;
    if lookup_entry(host, &path).await?.is_some() {
        bail!("EEXIST file exists: {path}");
    }
    ensure_parent_dir(host, &path).await?;
    let size = decoded_len(&content)?;
    insert_entry(
        host,
        &path,
        false,
        Some(content),
        mode,
        size,
        None,
        1,
        now_ms(),
    )
    .await
}

async fn create_dir(host: &HostCtx, path: &str, mode: i64, recursive: bool) -> Result<()> {
    let path = normalize_path(path)?;
    if path == "/" {
        return Ok(());
    }
    if let Some(existing) = lookup_entry(host, &path).await? {
        if recursive && existing.is_directory {
            return Ok(());
        }
        bail!("EEXIST file exists: {path}");
    }
    if recursive {
        let mut parents = Vec::new();
        let mut cursor = parent_path(&path);
        while let Some(parent) = cursor {
            if parent == "/" {
                break;
            }
            parents.push(parent.clone());
            cursor = parent_path(&parent);
        }
        parents.reverse();
        for parent in parents {
            if let Some(existing) = lookup_entry(host, &parent).await? {
                if !existing.is_directory {
                    bail!("ENOTDIR parent is not a directory: {}", existing.path);
                }
                continue;
            }
            insert_entry(host, &parent, true, None, mode, 0, None, 2, now_ms()).await?;
        }
    } else {
        ensure_parent_dir(host, &path).await?;
    }
    insert_entry(host, &path, true, None, mode, 0, None, 2, now_ms()).await
}

async fn read_dir(host: &HostCtx, path: &str) -> Result<Vec<String>> {
    Ok(read_dir_entries(host, path)
        .await?
        .into_iter()
        .map(|entry| entry.name)
        .collect())
}

async fn read_dir_entries(host: &HostCtx, path: &str) -> Result<Vec<FsEntry>> {
    let path = normalize_path(path)?;
    let entry = lookup_entry_required(host, &path).await?;
    if !entry.is_directory {
        bail!("ENOTDIR not a directory: {path}");
    }
    let prefix = if path == "/" {
        "/".to_owned()
    } else {
        format!("{path}/")
    };
    let rows = query_rows(
        host,
        "SELECT path, is_directory, content, mode, uid, gid, size, atime_ms, mtime_ms, ctime_ms, birthtime_ms, symlink_target, nlink
			FROM agent_os_fs_entries WHERE path LIKE ? AND path != ? ORDER BY path",
        &[json!(format!("{prefix}%")), json!(path)],
    )
    .await?;
    rows.into_iter()
        .map(FsEntry::from_row)
        .filter_map(|entry| match entry {
            Ok(entry) if parent_path(&entry.path).as_deref() == Some(path.as_str()) => {
                Some(Ok(entry))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
async fn insert_entry(
    host: &HostCtx,
    path: &str,
    is_directory: bool,
    content: Option<String>,
    mode: i64,
    size: i64,
    symlink_target: Option<String>,
    nlink: i64,
    now: i64,
) -> Result<()> {
    run_stmt(
        host,
        "INSERT INTO agent_os_fs_entries
			(path, is_directory, content, mode, uid, gid, size, atime_ms, mtime_ms, ctime_ms, birthtime_ms, symlink_target, nlink)
			VALUES (?, ?, ?, ?, 0, 0, ?, ?, ?, ?, ?, ?, ?)",
        &[
            json!(path),
            json!(if is_directory { 1 } else { 0 }),
            content.map_or(JsonValue::Null, JsonValue::String),
            json!(mode),
            json!(size),
            json!(now),
            json!(now),
            json!(now),
            json!(now),
            symlink_target.map_or(JsonValue::Null, JsonValue::String),
            json!(nlink),
        ],
    )
    .await
}

fn required_string(args: &JsonValue, key: &str) -> Result<String> {
    Ok(required_string_ref(args, key)?.to_owned())
}

fn optional_i64(args: &JsonValue, key: &str) -> Option<i64> {
    args.get(key).and_then(JsonValue::as_i64)
}

fn decoded_len(content: &str) -> Result<i64> {
    Ok(decode_content(content)?.len() as i64)
}

fn required_i64(args: &JsonValue, key: &str) -> Result<i64> {
    optional_i64(args, key).ok_or_else(|| anyhow!("EINVAL missing integer arg {key}"))
}

fn required_len(args: &JsonValue) -> Result<i64> {
    optional_i64(args, "len")
        .or_else(|| optional_i64(args, "length"))
        .ok_or_else(|| anyhow!("EINVAL missing integer arg length"))
}

// --- remove/rename/symlink/link/chmod/chown/utimes/truncate/pread handlers ---

async fn remove_file(host: &HostCtx, path: &str) -> Result<()> {
    let entry = lookup_entry_required(host, path).await?;
    if entry.is_directory {
        bail!("EISDIR is a directory: {}", entry.path);
    }
    run_stmt(
        host,
        "DELETE FROM agent_os_fs_entries WHERE path = ?",
        &[json!(entry.path)],
    )
    .await
}

async fn remove_dir(host: &HostCtx, path: &str) -> Result<()> {
    let entry = lookup_entry_required(host, path).await?;
    if !entry.is_directory {
        bail!("ENOTDIR not a directory: {}", entry.path);
    }
    if entry.path == "/" {
        bail!("EBUSY cannot remove root directory");
    }
    if !read_dir_entries(host, &entry.path).await?.is_empty() {
        bail!("ENOTEMPTY directory not empty: {}", entry.path);
    }
    run_stmt(
        host,
        "DELETE FROM agent_os_fs_entries WHERE path = ?",
        &[json!(entry.path)],
    )
    .await
}

async fn rename_entry(host: &HostCtx, old_path: String, new_path: String) -> Result<()> {
    let old_path = normalize_path(&old_path)?;
    let new_path = normalize_path(&new_path)?;
    if old_path == "/" {
        bail!("EBUSY cannot rename root directory");
    }
    let entry = lookup_entry_required(host, &old_path).await?;
    ensure_parent_dir(host, &new_path).await?;
    if entry.is_directory && new_path.starts_with(&format!("{old_path}/")) {
        bail!("EINVAL cannot move directory into itself");
    }
    if let Some(existing) = lookup_entry(host, &new_path).await? {
        if existing.is_directory && !read_dir_entries(host, &existing.path).await?.is_empty() {
            bail!("ENOTEMPTY target directory not empty: {}", existing.path);
        }
        run_stmt(
            host,
            "DELETE FROM agent_os_fs_entries WHERE path = ?",
            &[json!(existing.path)],
        )
        .await?;
    }
    let old_prefix = format!("{old_path}/");
    let new_prefix = format!("{new_path}/");
    let rows = query_rows(
        host,
        "SELECT path FROM agent_os_fs_entries WHERE path = ? OR path LIKE ? ORDER BY path",
        &[json!(old_path), json!(format!("{old_prefix}%"))],
    )
    .await?;
    for row in rows {
        let path = row
            .get("path")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| anyhow!("sqlite_vfs rename row missing path"))?;
        let next_path = if path == old_path {
            new_path.clone()
        } else {
            format!("{new_prefix}{}", &path[old_prefix.len()..])
        };
        run_stmt(
            host,
            "UPDATE agent_os_fs_entries SET path = ?, ctime_ms = ? WHERE path = ?",
            &[json!(next_path), json!(now_ms()), json!(path)],
        )
        .await?;
    }
    Ok(())
}

async fn symlink_entry(host: &HostCtx, target: String, path: String) -> Result<()> {
    let path = normalize_path(&path)?;
    if lookup_entry(host, &path).await?.is_some() {
        bail!("EEXIST file exists: {path}");
    }
    ensure_parent_dir(host, &path).await?;
    insert_entry(
        host,
        &path,
        false,
        None,
        DEFAULT_SYMLINK_MODE,
        target.len() as i64,
        Some(target),
        1,
        now_ms(),
    )
    .await
}

async fn read_link(host: &HostCtx, path: &str) -> Result<String> {
    let entry = lookup_entry_required(host, path).await?;
    entry
        .symlink_target
        .ok_or_else(|| anyhow!("EINVAL not a symbolic link: {}", entry.path))
}

async fn link_entry(host: &HostCtx, old_path: String, new_path: String) -> Result<()> {
    let old_path = normalize_path(&old_path)?;
    let new_path = normalize_path(&new_path)?;
    if lookup_entry(host, &new_path).await?.is_some() {
        bail!("EEXIST file exists: {new_path}");
    }
    ensure_parent_dir(host, &new_path).await?;
    let entry = lookup_entry_required(host, &old_path).await?;
    if entry.is_directory {
        bail!("EPERM cannot hard-link directory: {old_path}");
    }
    insert_entry(
        host,
        &new_path,
        false,
        entry.content,
        entry.mode,
        entry.size,
        entry.symlink_target,
        1,
        now_ms(),
    )
    .await?;
    update_one_field(host, &old_path, "nlink", json!(entry.nlink + 1)).await
}

async fn update_owner(host: &HostCtx, path: &str, uid: i64, gid: i64) -> Result<()> {
    let path = normalize_path(path)?;
    lookup_entry_required(host, &path).await?;
    run_stmt(
        host,
        "UPDATE agent_os_fs_entries SET uid = ?, gid = ?, ctime_ms = ? WHERE path = ?",
        &[json!(uid), json!(gid), json!(now_ms()), json!(path)],
    )
    .await
}

async fn update_times(host: &HostCtx, path: &str, atime_ms: i64, mtime_ms: i64) -> Result<()> {
    let path = normalize_path(path)?;
    lookup_entry_required(host, &path).await?;
    run_stmt(
        host,
        "UPDATE agent_os_fs_entries SET atime_ms = ?, mtime_ms = ?, ctime_ms = ? WHERE path = ?",
        &[
            json!(atime_ms),
            json!(mtime_ms),
            json!(now_ms()),
            json!(path),
        ],
    )
    .await
}

async fn truncate_file(host: &HostCtx, path: &str, len: i64) -> Result<()> {
    if len < 0 {
        bail!("EINVAL negative truncate length");
    }
    let entry = lookup_entry_required(host, path).await?;
    if entry.is_directory {
        bail!("EISDIR is a directory: {}", entry.path);
    }
    let mut bytes = decode_content(entry.content.as_deref().unwrap_or_default())?;
    bytes.resize(len as usize, 0);
    let content = BASE64.encode(bytes);
    run_stmt(
        host,
        "UPDATE agent_os_fs_entries SET content = ?, size = ?, mtime_ms = ?, ctime_ms = ? WHERE path = ?",
        &[
            json!(content),
            json!(len),
            json!(now_ms()),
            json!(now_ms()),
            json!(entry.path),
        ],
    )
    .await
}

async fn pread_file(host: &HostCtx, path: &str, offset: i64, len: i64) -> Result<String> {
    if offset < 0 || len < 0 {
        bail!("EINVAL negative pread offset or length");
    }
    let entry = lookup_entry_required(host, path).await?;
    if entry.is_directory {
        bail!("EISDIR is a directory: {}", entry.path);
    }
    let bytes = decode_content(entry.content.as_deref().unwrap_or_default())?;
    let start = (offset as usize).min(bytes.len());
    let end = start.saturating_add(len as usize).min(bytes.len());
    Ok(BASE64.encode(&bytes[start..end]))
}

async fn update_one_field(host: &HostCtx, path: &str, field: &str, value: JsonValue) -> Result<()> {
    let path = normalize_path(path)?;
    lookup_entry_required(host, &path).await?;
    let sql = match field {
        "mode" => "UPDATE agent_os_fs_entries SET mode = ?, ctime_ms = ? WHERE path = ?",
        "nlink" => "UPDATE agent_os_fs_entries SET nlink = ?, ctime_ms = ? WHERE path = ?",
        _ => bail!("EINVAL unsupported update field {field}"),
    };
    run_stmt(host, sql, &[value, json!(now_ms()), json!(path)]).await
}

fn stat_json(entry: FsEntry) -> JsonValue {
    json!({
        "dev": 0,
        "ino": stable_ino(&entry.path),
        "mode": entry.mode,
        "nlink": entry.nlink,
        "uid": entry.uid,
        "gid": entry.gid,
        "rdev": 0,
        "size": entry.size,
        "blocks": if entry.size == 0 { 0 } else { (entry.size + 511) / 512 },
        "atimeMs": entry.atime_ms,
        "mtimeMs": entry.mtime_ms,
        "ctimeMs": entry.ctime_ms,
        "birthtimeMs": entry.birthtime_ms,
        "atimeNsec": (entry.atime_ms % 1000) * 1_000_000,
        "mtimeNsec": (entry.mtime_ms % 1000) * 1_000_000,
        "ctimeNsec": (entry.ctime_ms % 1000) * 1_000_000,
        "birthtimeNsec": (entry.birthtime_ms % 1000) * 1_000_000,
        "isDirectory": entry.is_directory,
        "isSymbolicLink": entry.symlink_target.is_some(),
    })
}

fn stable_ino(path: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

// ---------------------------------------------------------------------------
// Session-event persistence (spec §4/§5) — ported from `rivetkit-agent-os`'s
// `persistence.rs`, `HostCtx` substituted for rivetkit's `Ctx`.
//
// `agent_os_session_events` is the canonical append-only conversation log,
// keyed by `external_session_id`. `seq` is INTERNAL ordering only.
// ---------------------------------------------------------------------------

/// Append one captured event to `agent_os_session_events` under the stable
/// `external_session_id`, allocating the next per-session `seq` (`MAX(seq)+1`).
pub(crate) async fn insert_session_event(
    host: &HostCtx,
    external_session_id: &str,
    event_json: &str,
) -> Result<()> {
    let rows = query_rows(
        host,
        "SELECT MAX(seq) AS max_seq FROM agent_os_session_events WHERE session_id = ?",
        &[json!(external_session_id)],
    )
    .await?;
    let next_seq = rows
        .first()
        .and_then(|row| row.get("max_seq"))
        .and_then(JsonValue::as_i64)
        .map(|max| max + 1)
        .unwrap_or(0);
    run_stmt(
        host,
        "INSERT INTO agent_os_session_events (session_id, seq, event, created_at) \
         VALUES (?, ?, ?, ?)",
        &[
            json!(external_session_id),
            json!(next_seq),
            json!(event_json),
            json!(now_ms()),
        ],
    )
    .await
}

/// Render the persisted event log for `external_session_id` to a Markdown
/// transcript, write it through the same sqlite_vfs path the guest reads, and
/// return that path (spec §7). Idempotent: overwritten fresh each resume.
pub(crate) async fn reconstruct_transcript_to_file(
    host: &HostCtx,
    external_session_id: &str,
) -> Result<String> {
    let rows = query_rows(
        host,
        "SELECT event FROM agent_os_session_events WHERE session_id = ? ORDER BY seq",
        &[json!(external_session_id)],
    )
    .await?;
    let events: Vec<JsonValue> = rows
        .into_iter()
        .filter_map(|mut row| {
            row.remove("event")
                .and_then(|v| match v {
                    JsonValue::String(raw) => Some(raw),
                    _ => None,
                })
                .and_then(|raw| serde_json::from_str::<JsonValue>(&raw).ok())
        })
        .collect();

    let markdown = render_transcript_markdown(external_session_id, &events);

    let path = format!("/root/.agentos/threads/{external_session_id}.md");
    create_dir(host, "/root/.agentos/threads", DEFAULT_DIR_MODE, true).await?;
    // The callback stores base64 file content (see `decode_content`).
    write_file(host, &path, BASE64.encode(markdown), DEFAULT_FILE_MODE).await?;
    Ok(path)
}

/// Render captured ACP events to a role-labeled Markdown transcript. Pure /
/// deterministic so reconstruction is idempotent.
fn render_transcript_markdown(external_session_id: &str, events: &[JsonValue]) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Session transcript: {external_session_id}\n"));

    for event in events {
        if event.get("method").and_then(JsonValue::as_str) == Some("user_prompt") {
            if let Some(text) = event
                .get("params")
                .and_then(|p| p.get("text"))
                .and_then(JsonValue::as_str)
            {
                out.push_str(&format!("\n## User\n\n{text}\n"));
            }
            continue;
        }

        let Some(update) = event.get("params").and_then(|p| p.get("update")) else {
            continue;
        };
        let kind = update
            .get("sessionUpdate")
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        match kind {
            "agent_message_chunk" | "agent_thought_chunk" => {
                if let Some(text) = update
                    .get("content")
                    .and_then(|c| c.get("text"))
                    .and_then(JsonValue::as_str)
                {
                    if kind == "agent_thought_chunk" {
                        out.push_str(&format!("\n## Assistant (thinking)\n\n{text}\n"));
                    } else {
                        out.push_str(&format!("\n## Assistant\n\n{text}\n"));
                    }
                }
            }
            "tool_call" | "tool_call_update" => {
                let title = update
                    .get("title")
                    .and_then(JsonValue::as_str)
                    .or_else(|| update.get("kind").and_then(JsonValue::as_str))
                    .unwrap_or("tool call");
                let status = update
                    .get("status")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("");
                out.push_str(&format!("\n### Tool call: {title}"));
                if !status.is_empty() {
                    out.push_str(&format!(" ({status})"));
                }
                out.push('\n');
                if let Some(content) = update.get("content").and_then(JsonValue::as_array) {
                    for item in content {
                        if let Some(text) = item
                            .get("content")
                            .and_then(|c| c.get("text"))
                            .and_then(JsonValue::as_str)
                            .or_else(|| item.get("text").and_then(JsonValue::as_str))
                        {
                            out.push_str(&format!("\n```\n{text}\n```\n"));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    out
}
