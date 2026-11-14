//! Bounded, database-owned transfer of one synchronous SQLite operation.
use super::*;
use crate::state::ActiveSqliteTransfer;
use agentos_driver_tokio::accounting::{Reservation, ResourceClass};
use serde::ser::{SerializeMap, SerializeSeq};
use std::borrow::Cow;
use std::io::{Read, Write};

const CHUNK: usize = 32 * 1024;
const INLINE: usize = 256 * 1024;

fn error(message: impl Into<String>) -> VmError {
    VmError::host("EINVAL", message.into())
}
fn next_generation(database: &mut ActiveSqliteDatabase) -> Result<u64, VmError> {
    database.next_transfer_generation = database
        .next_transfer_generation
        .checked_add(1)
        .ok_or_else(|| error("SQLite transfer generations exhausted"))?;
    Ok(database.next_transfer_generation)
}

impl ActiveSqliteTransfer {
    fn new(runtime: agentos_driver_tokio::DriverHandle, maximum: usize) -> Result<Self, VmError> {
        Ok(Self {
            generation: 0,
            maximum,
            length: 0,
            chunks: Vec::new(),
            reservation: None,
            runtime,
            fault: None,
        })
    }
    fn failure(&mut self, fallback: impl std::fmt::Display) -> VmError {
        self.fault
            .take()
            .unwrap_or_else(|| error(fallback.to_string()))
    }
}

impl Write for ActiveSqliteTransfer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let requested = self.length.checked_add(bytes.len());
        if requested.is_none_or(|requested| requested > self.maximum) {
            self.fault = Some(VmError::host(
                "ERR_AGENTOS_RESOURCE_LIMIT",
                format!(
                    "limits.sqlite.maxResultBytes limit {} exceeded by SQLite transfer; configure an explicit larger SQLite result budget",
                    self.maximum
                ),
            ));
            return Err(std::io::Error::other("SQLite transfer byte limit"));
        }
        let mut remaining = bytes;
        while !remaining.is_empty() {
            if self
                .chunks
                .last()
                .is_none_or(|chunk| chunk.len() == chunk.capacity())
            {
                let capacity = CHUNK.min(self.maximum - self.length);
                let reservation = match self
                    .runtime
                    .resources()
                    .reserve(ResourceClass::ExecutorBytes, capacity)
                {
                    Ok(reservation) => reservation,
                    Err(error) => {
                        self.fault = Some(VmError::from(error));
                        return Err(std::io::Error::other("SQLite transfer admission"));
                    }
                };
                if let Some(existing) = &mut self.reservation {
                    if existing.merge(reservation).is_err() {
                        self.fault = Some(error("SQLite transfer reservation scopes differ"));
                        return Err(std::io::Error::other("SQLite transfer reservation"));
                    }
                } else {
                    self.reservation = Some(reservation);
                }
                self.chunks.push(Vec::with_capacity(capacity));
            }
            let chunk = self.chunks.last_mut().expect("chunk admitted above");
            let count = remaining.len().min(chunk.capacity() - chunk.len());
            chunk.extend_from_slice(&remaining[..count]);
            self.length += count;
            remaining = &remaining[count..];
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Reader<'a> {
    transfer: &'a ActiveSqliteTransfer,
    position: usize,
}
impl Read for Reader<'_> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if self.position == self.transfer.length || output.is_empty() {
            return Ok(0);
        }
        let chunk = &self.transfer.chunks[self.position / CHUNK];
        let within = self.position % CHUNK;
        let count = output.len().min(chunk.len() - within);
        output[..count].copy_from_slice(&chunk[within..within + count]);
        self.position += count;
        Ok(count)
    }
}

pub(super) fn method(
    process: &mut ActiveProcess,
    request: &HostRpcRequest,
) -> Result<Value, VmError> {
    let database_id = javascript_sync_rpc_arg_u64(&request.args, 0, "SQLite transfer database")?;
    if request.method == "sqlite.transfer.begin" {
        let length = usize::try_from(javascript_sync_rpc_arg_u64(
            &request.args,
            1,
            "SQLite parameter bytes",
        )?)
        .map_err(|_| error("SQLite parameter size does not fit usize"))?;
        if length == 0 || length > process.limits.reactor.max_blocking_bytes {
            return Err(error(
                "SQLite parameter transfer exceeds limits.reactor.maxBlockingBytes",
            ));
        }
        let mut transfer = ActiveSqliteTransfer::new(process.runtime_context.clone(), length)?;
        let database = sqlite_database_mut(process, database_id)?;
        transfer.generation = next_generation(database)?;
        let generation = transfer.generation;
        // A new synchronous operation retires an abandoned prior upload.
        database.input_transfer = Some(transfer);
        return Ok(json!({"generation":generation}));
    }
    let generation = javascript_sync_rpc_arg_u64(&request.args, 1, "SQLite transfer generation")?;
    let database = sqlite_database_mut(process, database_id)?;
    if matches!(
        request.method.as_str(),
        "sqlite.transfer.cancel" | "sqlite.transfer.release"
    ) {
        let slot = if request.method == "sqlite.transfer.cancel" {
            &mut database.input_transfer
        } else {
            &mut database.output_transfer
        };
        if slot
            .as_ref()
            .is_some_and(|transfer| transfer.generation != generation)
        {
            return Err(error("stale SQLite transfer generation"));
        }
        *slot = None;
        return Ok(Value::Null);
    }
    let transfer = if request.method == "sqlite.transfer.append" {
        database.input_transfer.as_mut()
    } else {
        database.output_transfer.as_mut()
    }
    .ok_or_else(|| error("SQLite transfer is no longer active"))?;
    if transfer.generation != generation {
        return Err(error("stale SQLite transfer generation"));
    }
    let offset = usize::try_from(javascript_sync_rpc_arg_u64(
        &request.args,
        2,
        "SQLite transfer offset",
    )?)
    .map_err(|_| error("SQLite offset does not fit usize"))?;
    match request.method.as_str() {
        "sqlite.transfer.append" => {
            let encoded = javascript_sync_rpc_arg_str(&request.args, 3, "SQLite parameter chunk")?;
            if encoded.len() > CHUNK.div_ceil(3) * 4 || offset != transfer.length {
                return Err(error("invalid SQLite upload offset or chunk size"));
            }
            let _decoded_charge = transfer
                .runtime
                .resources()
                .reserve(ResourceClass::BridgeRequestBytes, CHUNK)
                .map_err(VmError::from)?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| error("invalid SQLite upload base64"))?;
            if bytes.is_empty()
                || bytes.len() > CHUNK
                || bytes.len() > transfer.maximum - transfer.length
            {
                return Err(error("invalid SQLite upload chunk size"));
            }
            if let Err(error) = transfer.write_all(&bytes) {
                return Err(transfer.failure(error));
            }
            Ok(Value::Null)
        }
        "sqlite.transfer.read" => {
            if offset >= transfer.length {
                return Err(error("SQLite result offset is outside its result"));
            }
            let chunk = &transfer.chunks[offset / CHUNK];
            let within = offset % CHUNK;
            Ok(Value::String(
                base64::engine::general_purpose::STANDARD.encode(&chunk[within..]),
            ))
        }
        _ => Err(error("unknown SQLite transfer operation")),
    }
}

pub(super) struct Parameters<'a> {
    value: Option<Cow<'a, Value>>,
    _charge: Option<Reservation>,
}
impl Parameters<'_> {
    pub(super) fn value(&self) -> Option<&Value> {
        self.value.as_deref()
    }
}
pub(super) fn parameters<'a>(
    process: &mut ActiveProcess,
    database_id: u64,
    request: &'a HostRpcRequest,
    index: usize,
    descriptor_index: usize,
) -> Result<Parameters<'a>, VmError> {
    let descriptor = request
        .args
        .get(descriptor_index)
        .filter(|value| !value.is_null());
    if !request.method.ends_with("Chunked") || descriptor.is_none() {
        return Ok(Parameters {
            value: request.args.get(index).map(Cow::Borrowed),
            _charge: None,
        });
    }
    let descriptor = descriptor.expect("checked above");
    if descriptor.get("database").and_then(Value::as_u64) != Some(database_id) {
        return Err(error("SQLite upload belongs to another database"));
    }
    let generation = descriptor
        .get("generation")
        .and_then(Value::as_u64)
        .ok_or_else(|| error("invalid SQLite parameter generation"))?;
    let runtime = process.runtime_context.clone();
    let database = sqlite_database_mut(process, database_id)?;
    let transfer = database
        .input_transfer
        .as_ref()
        .ok_or_else(|| error("SQLite parameter upload is missing"))?;
    if transfer.generation != generation || transfer.length != transfer.maximum {
        return Err(error("stale or incomplete SQLite parameter upload"));
    }
    let charge = runtime
        .resources()
        .reserve(ResourceClass::ExecutorBytes, transfer.length)
        .map_err(VmError::from)?;
    let transfer = database
        .input_transfer
        .take()
        .expect("validated upload remains present");
    let value = serde_json::from_reader(Reader {
        transfer: &transfer,
        position: 0,
    })
    .map_err(|error| self::error(format!("invalid SQLite parameter JSON: {error}")))?;
    Ok(Parameters {
        value: Some(Cow::Owned(value)),
        _charge: Some(charge),
    })
}

struct LossyText<'a>(&'a [u8]);
impl std::fmt::Display for LossyText<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut bytes = self.0;
        while !bytes.is_empty() {
            match std::str::from_utf8(bytes) {
                Ok(text) => {
                    formatter.write_str(text)?;
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    formatter.write_str(
                        std::str::from_utf8(&bytes[..valid]).expect("validated UTF-8 prefix"),
                    )?;
                    formatter.write_str("\u{fffd}")?;
                    bytes = &bytes[valid + error.error_len().unwrap_or(bytes.len() - valid)..];
                }
            }
        }
        Ok(())
    }
}
struct BlobJson<'a>(&'a [u8]);
impl serde::Serialize for BlobJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&base64::display::Base64Display::new(
            self.0,
            &base64::engine::general_purpose::STANDARD,
        ))
    }
}
struct CellJson<'a> {
    value: SqliteValueRef<'a>,
    bigints: bool,
}
impl serde::Serialize for CellJson<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.value {
            SqliteValueRef::Null => serializer.serialize_none(),
            SqliteValueRef::Integer(value)
                if self.bigints || value.unsigned_abs() > SQLITE_JS_SAFE_INTEGER_MAX as u64 =>
            {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("__agentosSqliteType", "bigint")?;
                map.serialize_entry("value", &value.to_string())?;
                map.end()
            }
            SqliteValueRef::Integer(value) => serializer.serialize_i64(value),
            SqliteValueRef::Real(value) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("__agentosSqliteType", "double")?;
                map.serialize_entry("value", &format!("{:016x}", value.to_bits()))?;
                map.end()
            }
            SqliteValueRef::Text(bytes) => serializer.collect_str(&LossyText(bytes)),
            SqliteValueRef::Blob(bytes) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("__agentosSqliteType", "uint8array")?;
                map.serialize_entry("value", &BlobJson(bytes))?;
                map.end()
            }
        }
    }
}
struct RowJson<'a, 'b> {
    row: &'a rusqlite::Row<'b>,
    names: &'a [String],
    columns: &'a [(usize, usize)],
    arrays: bool,
    bigints: bool,
}
impl serde::Serialize for RowJson<'_, '_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.arrays {
            let mut sequence = serializer.serialize_seq(Some(self.names.len()))?;
            for index in 0..self.names.len() {
                let value = self.row.get_ref(index).map_err(serde::ser::Error::custom)?;
                sequence.serialize_element(&CellJson {
                    value,
                    bigints: self.bigints,
                })?;
            }
            sequence.end()
        } else {
            let mut map = serializer.serialize_map(Some(self.columns.len()))?;
            for &(name, index) in self.columns {
                let value = self.row.get_ref(index).map_err(serde::ser::Error::custom)?;
                map.serialize_entry(
                    &self.names[name],
                    &CellJson {
                        value,
                        bigints: self.bigints,
                    },
                )?;
            }
            map.end()
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn query_rows(
    connection: &mut SqliteConnection,
    sql: &str,
    params: Option<&Value>,
    arrays: bool,
    bigints: bool,
    bare: bool,
    unknown: bool,
    runtime: agentos_driver_tokio::DriverHandle,
    maximum: usize,
    first: bool,
) -> Result<ActiveSqliteTransfer, VmError> {
    let mut statement = connection.prepare(sql).map_err(sqlite_error)?;
    let names = statement
        .column_names()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let mut positions: BTreeMap<&String, usize> = BTreeMap::new();
    let mut columns: Vec<(usize, usize)> = Vec::new();
    for (index, name) in names.iter().enumerate() {
        if let Some(&position) = positions.get(name) {
            columns[position].1 = index;
        } else {
            positions.insert(name, columns.len());
            columns.push((index, index));
        }
    }
    bind_sqlite_parameters(&mut statement, params, bare, unknown)?;
    let mut result = ActiveSqliteTransfer::new(runtime, maximum)?;
    let mut rows = statement.raw_query();
    let mut count = 0;
    if !first {
        if let Err(error) = result.write_all(b"[") {
            return Err(result.failure(error));
        }
    }
    while let Some(row) = rows.next().map_err(sqlite_error)? {
        if count > 0 {
            if let Err(error) = result.write_all(b",") {
                return Err(result.failure(error));
            }
        }
        if let Err(error) = serde_json::to_writer(
            &mut result,
            &RowJson {
                row,
                names: &names,
                columns: &columns,
                arrays,
                bigints,
            },
        ) {
            return Err(result.failure(error));
        }
        count += 1;
        if first {
            break;
        }
    }
    if !first {
        if let Err(error) = result.write_all(b"]") {
            return Err(result.failure(error));
        }
    } else if count == 0 {
        if let Err(error) = result.write_all(b"null") {
            return Err(result.failure(error));
        }
    }
    Ok(result)
}

pub(super) fn reply(
    process: &mut ActiveProcess,
    database_id: u64,
    mut result: ActiveSqliteTransfer,
    chunked: bool,
) -> Result<Value, VmError> {
    if !chunked || result.length <= INLINE {
        let _charge = result
            .runtime
            .resources()
            .reserve(ResourceClass::ExecutorBytes, result.length)
            .map_err(VmError::from)?;
        let value: Value = serde_json::from_reader(Reader {
            transfer: &result,
            position: 0,
        })
        .map_err(|error| self::error(format!("SQLite result JSON: {error}")))?;
        return Ok(if chunked {
            json!({"kind":"inline","value":value})
        } else {
            value
        });
    }
    let database = sqlite_database_mut(process, database_id)?;
    result.generation = next_generation(database)?;
    let metadata = json!({"kind":"transfer","database":database_id,"generation":result.generation,"length":result.length});
    database.output_transfer = Some(result);
    Ok(metadata)
}
