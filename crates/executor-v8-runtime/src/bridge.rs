// Host function injection via v8::FunctionTemplate

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::OnceLock;

use agentos_vm_host_interface::bridge_contract;
use serde::de;
use v8::MapFnTo;
use v8::ValueDeserializerHelper;
use v8::ValueSerializerHelper;

use crate::host_call::BridgeCallContext;

// CBOR codec flag: when true, use CBOR (via ciborium) instead of V8
// ValueSerializer/ValueDeserializer for IPC payloads. Activated by
// AGENTOS_V8_CODEC=cbor for runtimes whose node:v8 module doesn't
// produce real V8 serialization format (e.g. Bun).
static USE_CBOR_CODEC: AtomicBool = AtomicBool::new(false);
static EMBEDDED_CBOR_USERS: AtomicUsize = AtomicUsize::new(0);
const MAX_CBOR_BRIDGE_DEPTH: usize = 64;
const MAX_CBOR_BRIDGE_CONTAINER_ITEMS: usize = 100_000;
const MAX_VM_CONTEXTS: usize = 1024;
pub(crate) const MAX_PENDING_PROMISES: usize = 1024;
const DEFAULT_BRIDGE_RESPONSE_MAX_BYTES: usize = 256 * 1024;
const READ_RESPONSE_ENVELOPE_BYTES: usize = 4 * 1024;

/// Initialize the codec from the AGENTOS_V8_CODEC environment variable.
/// Call once at process startup before any sessions are created.
pub fn init_codec() {
    USE_CBOR_CODEC.store(configured_cbor_codec_enabled(), Ordering::Relaxed);
}

pub fn enable_cbor_codec() {
    USE_CBOR_CODEC.store(true, Ordering::Relaxed);
}

pub fn acquire_embedded_cbor_codec() {
    EMBEDDED_CBOR_USERS.fetch_add(1, Ordering::AcqRel);
    USE_CBOR_CODEC.store(true, Ordering::Relaxed);
}

pub fn release_embedded_cbor_codec() {
    let previous = EMBEDDED_CBOR_USERS.fetch_sub(1, Ordering::AcqRel);
    if previous <= 1 {
        USE_CBOR_CODEC.store(configured_cbor_codec_enabled(), Ordering::Relaxed);
    }
}

/// Returns true if the CBOR codec is active.
pub fn is_cbor_codec() -> bool {
    USE_CBOR_CODEC.load(Ordering::Relaxed)
}

fn configured_cbor_codec_enabled() -> bool {
    std::env::var("AGENTOS_V8_CODEC")
        .map(|val| val == "cbor")
        .unwrap_or(false)
}

/// External references for V8 snapshot serialization.
/// Maps function pointer indices in the snapshot to current addresses.
/// Must be identical at snapshot creation and restore time.
pub fn external_refs() -> &'static v8::ExternalReferences {
    static REFS: OnceLock<v8::ExternalReferences> = OnceLock::new();
    REFS.get_or_init(|| {
        v8::ExternalReferences::new(&[
            v8::ExternalReference {
                function: sync_bridge_callback.map_fn_to(),
            },
            v8::ExternalReference {
                function: async_bridge_callback.map_fn_to(),
            },
            v8::ExternalReference {
                function: crate::terminal_callback::report_terminal_callback.map_fn_to(),
            },
            v8::ExternalReference {
                function: crate::terminal_callback::request_process_exit.map_fn_to(),
            },
        ])
    })
}

// Minimal delegate for V8 ValueSerializer — throws DataCloneError as a V8 exception
struct DefaultSerializerDelegate;

impl v8::ValueSerializerImpl for DefaultSerializerDelegate {
    fn throw_data_clone_error<'s>(
        &self,
        scope: &mut v8::HandleScope<'s>,
        message: v8::Local<'s, v8::String>,
    ) {
        let exc = v8::Exception::error(scope, message);
        scope.throw_exception(exc);
    }
}

// Minimal delegate for V8 ValueDeserializer — default callbacks are sufficient
struct DefaultDeserializerDelegate;

impl v8::ValueDeserializerImpl for DefaultDeserializerDelegate {}

/// Serialize a V8 value to bytes using V8's built-in ValueSerializer.
/// Handles all V8 types natively: primitives, strings, arrays, objects,
/// Uint8Array, Date, Map, Set, RegExp, Error, and circular references.
/// When CBOR codec is active, uses ciborium instead.
pub fn serialize_v8_value(
    scope: &mut v8::HandleScope,
    value: v8::Local<v8::Value>,
) -> Result<Vec<u8>, String> {
    if is_cbor_codec() {
        return serialize_cbor_value(scope, value);
    }
    serialize_v8_wire_value(scope, value)
}

/// Serialize a V8 value to bytes using V8's native wire format regardless of
/// the process-wide codec toggle.
pub fn serialize_v8_wire_value(
    scope: &mut v8::HandleScope,
    value: v8::Local<v8::Value>,
) -> Result<Vec<u8>, String> {
    let context = scope.get_current_context();
    let serializer = v8::ValueSerializer::new(scope, Box::new(DefaultSerializerDelegate));
    serializer.write_header();
    serializer
        .write_value(context, value)
        .ok_or_else(|| "V8 ValueSerializer: failed to serialize value".to_string())?;
    Ok(serializer.release())
}

/// Deserialize bytes back to a V8 value using V8's built-in ValueDeserializer.
/// The bytes must have been produced by serialize_v8_value() or node:v8.serialize().
pub fn deserialize_v8_value<'s>(
    scope: &mut v8::HandleScope<'s>,
    data: &[u8],
) -> Result<v8::Local<'s, v8::Value>, String> {
    if is_cbor_codec() {
        return deserialize_cbor_value(scope, data);
    }
    deserialize_v8_wire_value(scope, data)
}

/// Deserialize bytes from V8's native wire format regardless of the
/// process-wide codec toggle.
pub fn deserialize_v8_wire_value<'s>(
    scope: &mut v8::HandleScope<'s>,
    data: &[u8],
) -> Result<v8::Local<'s, v8::Value>, String> {
    let context = scope.get_current_context();
    let deserializer =
        v8::ValueDeserializer::new(scope, Box::new(DefaultDeserializerDelegate), data);
    deserializer
        .read_header(context)
        .ok_or_else(|| "V8 ValueDeserializer: invalid header".to_string())?;
    deserializer
        .read_value(context)
        .ok_or_else(|| "V8 ValueDeserializer: failed to deserialize value".to_string())
}

// ── CBOR codec ──

/// Convert a V8 value to a ciborium::Value for CBOR serialization.
fn v8_to_cbor(
    scope: &mut v8::HandleScope,
    value: v8::Local<v8::Value>,
) -> Result<ciborium::Value, String> {
    let mut object_stack = Vec::new();
    v8_to_cbor_inner(scope, value, 0, &mut object_stack)
}

fn v8_to_cbor_inner(
    scope: &mut v8::HandleScope,
    value: v8::Local<v8::Value>,
    depth: usize,
    object_stack: &mut Vec<v8::Global<v8::Object>>,
) -> Result<ciborium::Value, String> {
    if depth > MAX_CBOR_BRIDGE_DEPTH {
        return Err(format!(
            "CBOR encode depth exceeds limit of {MAX_CBOR_BRIDGE_DEPTH}"
        ));
    }

    if value.is_null_or_undefined() {
        return Ok(ciborium::Value::Null);
    }
    if value.is_boolean() {
        return Ok(ciborium::Value::Bool(value.boolean_value(scope)));
    }
    if value.is_int32() {
        return Ok(ciborium::Value::Integer(
            value.int32_value(scope).unwrap_or(0).into(),
        ));
    }
    if value.is_number() {
        return Ok(ciborium::Value::Float(
            value.number_value(scope).unwrap_or(0.0),
        ));
    }
    if value.is_string() {
        let s = value.to_rust_string_lossy(scope);
        return Ok(ciborium::Value::Text(s));
    }
    if value.is_array_buffer_view() {
        let view = v8::Local::<v8::ArrayBufferView>::try_from(value).unwrap();
        let len = view.byte_length();
        let mut buf = vec![0u8; len];
        view.copy_contents(&mut buf);
        return Ok(ciborium::Value::Bytes(buf));
    }
    if value.is_array() {
        let obj = value
            .to_object(scope)
            .ok_or_else(|| "CBOR encode failed to convert array to object".to_string())?;
        enter_cbor_object(scope, object_stack, obj)?;
        let arr = v8::Local::<v8::Array>::try_from(value).unwrap();
        let len = arr.length();
        let item_count = cbor_container_item_count("array", len as usize)?;
        let mut items = Vec::with_capacity(item_count);
        let result = (|| {
            for i in 0..len {
                if let Some(elem) = arr.get_index(scope, i) {
                    items.push(v8_to_cbor_inner(scope, elem, depth + 1, object_stack)?);
                } else {
                    items.push(ciborium::Value::Null);
                }
            }
            Ok(ciborium::Value::Array(items))
        })();
        object_stack.pop();
        return result;
    }
    if value.is_object() {
        let obj = value.to_object(scope).unwrap();
        enter_cbor_object(scope, object_stack, obj)?;
        let names = obj
            .get_own_property_names(scope, v8::GetPropertyNamesArgs::default())
            .unwrap_or_else(|| v8::Array::new(scope, 0));
        let len = names.length();
        let item_count = cbor_container_item_count("object", len as usize)?;
        let mut entries = Vec::with_capacity(item_count);
        let result = (|| {
            for i in 0..len {
                let key = names.get_index(scope, i).unwrap();
                let key_str = key.to_rust_string_lossy(scope);
                let val = obj
                    .get(scope, key)
                    .unwrap_or_else(|| v8::undefined(scope).into());
                entries.push((
                    ciborium::Value::Text(key_str),
                    v8_to_cbor_inner(scope, val, depth + 1, object_stack)?,
                ));
            }
            Ok(ciborium::Value::Map(entries))
        })();
        object_stack.pop();
        return result;
    }
    Ok(ciborium::Value::Null)
}

fn enter_cbor_object(
    scope: &mut v8::HandleScope,
    object_stack: &mut Vec<v8::Global<v8::Object>>,
    object: v8::Local<v8::Object>,
) -> Result<(), String> {
    for previous in object_stack.iter() {
        let previous = v8::Local::new(scope, previous);
        if previous.strict_equals(object.into()) {
            return Err("CBOR encode rejected circular object graph".to_string());
        }
    }
    object_stack.push(v8::Global::new(scope, object));
    Ok(())
}

fn cbor_container_item_count(kind: &str, item_count: usize) -> Result<usize, String> {
    if item_count > MAX_CBOR_BRIDGE_CONTAINER_ITEMS {
        return Err(format!(
            "CBOR {kind} item count {item_count} exceeds limit of {MAX_CBOR_BRIDGE_CONTAINER_ITEMS}"
        ));
    }
    Ok(item_count)
}

struct LimitedCborValue(ciborium::Value);

impl<'de> de::Deserialize<'de> for LimitedCborValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_any(LimitedCborVisitor).map(Self)
    }
}

struct LimitedCborSeed;

impl<'de> de::DeserializeSeed<'de> for LimitedCborSeed {
    type Value = ciborium::Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_any(LimitedCborVisitor)
    }
}

struct LimitedCborVisitor;

impl<'de> de::Visitor<'de> for LimitedCborVisitor {
    type Value = ciborium::Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a bounded CBOR bridge value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(ciborium::Value::Bool(value))
    }

    fn visit_f32<E>(self, value: f32) -> Result<Self::Value, E> {
        Ok(ciborium::Value::Float(value.into()))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
        Ok(ciborium::Value::Float(value))
    }

    fn visit_i8<E>(self, value: i8) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_i16<E>(self, value: i16) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_i32<E>(self, value: i32) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_i128<E>(self, value: i128) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_u8<E>(self, value: u8) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_u16<E>(self, value: u16) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_u32<E>(self, value: u32) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_char<E>(self, value: char) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(value.into())
    }

    fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(value.into())
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(value.into())
    }

    fn visit_borrowed_bytes<E>(self, value: &'de [u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(value.into())
    }

    fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(ciborium::Value::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(ciborium::Value::Null)
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        if let Some(item_count) = access.size_hint() {
            limited_cbor_item_count("array", item_count)?;
        }

        let mut items = Vec::new();
        while let Some(item) = access.next_element_seed(LimitedCborSeed)? {
            limited_cbor_item_count("array", items.len() + 1)?;
            items.push(item);
        }
        Ok(ciborium::Value::Array(items))
    }

    fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        if let Some(item_count) = access.size_hint() {
            limited_cbor_item_count("map", item_count)?;
        }

        let mut entries = Vec::new();
        while let Some(key) = access.next_key_seed(LimitedCborSeed)? {
            limited_cbor_item_count("map", entries.len() + 1)?;
            let value = access.next_value_seed(LimitedCborSeed)?;
            entries.push((key, value));
        }
        Ok(ciborium::Value::Map(entries))
    }

    fn visit_enum<A>(self, access: A) -> Result<Self::Value, A::Error>
    where
        A: de::EnumAccess<'de>,
    {
        use serde::de::VariantAccess;

        struct TaggedValueVisitor;

        impl<'de> de::Visitor<'de> for TaggedValueVisitor {
            type Value = ciborium::Value;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a tagged CBOR bridge value")
            }

            fn visit_seq<A>(self, mut access: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let tag = access
                    .next_element()?
                    .ok_or_else(|| de::Error::custom("expected tag"))?;
                let value = access
                    .next_element_seed(LimitedCborSeed)?
                    .ok_or_else(|| de::Error::custom("expected tagged value"))?;
                Ok(ciborium::Value::Tag(tag, Box::new(value)))
            }
        }

        let (name, data): (String, _) = access.variant()?;
        if name != "@@TAGGED@@" {
            return Err(de::Error::custom("expected CBOR tag"));
        }
        data.tuple_variant(2, TaggedValueVisitor)
    }
}

fn limited_cbor_item_count<E: de::Error>(kind: &str, item_count: usize) -> Result<usize, E> {
    cbor_container_item_count(kind, item_count).map_err(de::Error::custom)
}

/// Convert a ciborium::Value to a V8 value.
fn cbor_to_v8<'s>(
    scope: &mut v8::HandleScope<'s>,
    value: &ciborium::Value,
) -> Result<v8::Local<'s, v8::Value>, String> {
    cbor_to_v8_inner(scope, value, 0)
}

fn cbor_to_v8_inner<'s>(
    scope: &mut v8::HandleScope<'s>,
    value: &ciborium::Value,
    depth: usize,
) -> Result<v8::Local<'s, v8::Value>, String> {
    if depth > MAX_CBOR_BRIDGE_DEPTH {
        return Err(format!(
            "CBOR decode depth exceeds limit of {MAX_CBOR_BRIDGE_DEPTH}"
        ));
    }

    match value {
        ciborium::Value::Null => Ok(v8::null(scope).into()),
        ciborium::Value::Bool(b) => Ok(v8::Boolean::new(scope, *b).into()),
        ciborium::Value::Integer(n) => {
            let n: i128 = (*n).into();
            if n >= i32::MIN as i128 && n <= i32::MAX as i128 {
                Ok(v8::Integer::new(scope, n as i32).into())
            } else {
                Ok(v8::Number::new(scope, n as f64).into())
            }
        }
        ciborium::Value::Float(f) => Ok(v8::Number::new(scope, *f).into()),
        ciborium::Value::Text(s) => Ok(v8::String::new(scope, s)
            .ok_or_else(|| "CBOR decode failed to allocate string".to_string())?
            .into()),
        ciborium::Value::Bytes(b) => raw_bytes_to_uint8array(scope, b)
            .ok_or_else(|| "CBOR decode failed to allocate byte array".to_string()),
        ciborium::Value::Array(items) => {
            cbor_container_item_count("array", items.len())?;
            let arr = v8::Array::new(scope, items.len() as i32);
            for (i, item) in items.iter().enumerate() {
                let val = cbor_to_v8_inner(scope, item, depth + 1)?;
                arr.set_index(scope, i as u32, val);
            }
            Ok(arr.into())
        }
        ciborium::Value::Map(entries) => {
            cbor_container_item_count("map", entries.len())?;
            let obj = v8::Object::new(scope);
            for (k, v) in entries {
                let key = cbor_to_v8_inner(scope, k, depth + 1)?;
                let val = cbor_to_v8_inner(scope, v, depth + 1)?;
                obj.set(scope, key, val);
            }
            Ok(obj.into())
        }
        ciborium::Value::Tag(_, inner) => cbor_to_v8_inner(scope, inner, depth + 1),
        _ => Ok(v8::undefined(scope).into()),
    }
}

fn raw_bytes_to_uint8array<'s>(
    scope: &mut v8::HandleScope<'s>,
    bytes: &[u8],
) -> Option<v8::Local<'s, v8::Value>> {
    // Host data uses a Rust-owned backing store. `v8::ArrayBuffer::new` draws
    // from the isolate's ArrayBuffer budget and fatal-aborts the whole process
    // when that budget is spent, so a guest that fills it could then crash the
    // sidecar by reading a file.
    let len = bytes.len();
    let backing_store = v8::ArrayBuffer::new_backing_store_from_vec(bytes.to_vec());
    let ab = v8::ArrayBuffer::with_backing_store(scope, &backing_store.make_shared());
    v8::Uint8Array::new(scope, ab, 0, len).map(Into::into)
}

fn bridge_response_payload_to_v8<'s>(
    scope: &mut v8::HandleScope<'s>,
    status: u8,
    payload: &[u8],
) -> Option<v8::Local<'s, v8::Value>> {
    if status == 2 {
        return raw_bytes_to_uint8array(scope, payload);
    }

    let v8_val = {
        let tc = &mut v8::TryCatch::new(scope);
        deserialize_v8_value(tc, payload).ok()
    };
    if v8_val.is_some() {
        return v8_val;
    }

    // Preserve the historical compatibility fallback for malformed/non-native
    // status=0 responses, but never let status=2 raw bytes take this path.
    raw_bytes_to_uint8array(scope, payload)
}

struct DecodedBridgeError {
    code: String,
    message: String,
    details: Option<ciborium::Value>,
}

fn decode_bridge_error(payload: &[u8]) -> Option<DecodedBridgeError> {
    let LimitedCborValue(ciborium::Value::Map(entries)) =
        ciborium::de::from_reader_with_recursion_limit(payload, MAX_CBOR_BRIDGE_DEPTH).ok()?
    else {
        return None;
    };
    let mut code = None;
    let mut message = None;
    let mut details = None;
    for (key, value) in entries {
        let ciborium::Value::Text(key) = key else {
            continue;
        };
        match key.as_str() {
            "code" => {
                if let ciborium::Value::Text(value) = value {
                    code = Some(value);
                }
            }
            "message" => {
                if let ciborium::Value::Text(value) = value {
                    message = Some(value);
                }
            }
            "details" if value != ciborium::Value::Null => details = Some(value),
            _ => {}
        }
    }
    Some(DecodedBridgeError {
        code: code?,
        message: message?,
        details,
    })
}

fn bridge_error_exception<'s>(
    scope: &mut v8::HandleScope<'s>,
    payload: &[u8],
) -> v8::Local<'s, v8::Value> {
    let decoded = decode_bridge_error(payload);
    let message = decoded
        .as_ref()
        .map(|error| error.message.as_str())
        .unwrap_or_else(|| std::str::from_utf8(payload).unwrap_or("bridge host call failed"));
    let message = v8::String::new(scope, message).expect("bridge error message allocation");
    let exception = v8::Exception::error(scope, message);
    let Some(decoded) = decoded else {
        return exception;
    };
    let object = exception
        .to_object(scope)
        .expect("Error exceptions are objects");
    let code_key = v8::String::new(scope, "code").expect("code property key");
    let code = v8::String::new(scope, &decoded.code).expect("bridge error code allocation");
    if object.set(scope, code_key.into(), code.into()) != Some(true) {
        eprintln!(
            "ERR_AGENTOS_BRIDGE_ERROR_PROPERTY: failed to attach the typed bridge error code"
        );
    }
    if let Some(details) = decoded.details {
        match cbor_to_v8_inner(scope, &details, 0) {
            Ok(details) => {
                let details_key =
                    v8::String::new(scope, "details").expect("details property key");
                if object.set(scope, details_key.into(), details) != Some(true) {
                    eprintln!(
                        "ERR_AGENTOS_BRIDGE_ERROR_PROPERTY: failed to attach typed bridge error details"
                    );
                }
            }
            Err(error) => eprintln!(
                "ERR_AGENTOS_BRIDGE_ERROR_DETAILS: failed to decode typed bridge error details: {error}"
            ),
        }
    }
    exception
}

fn runtime_error_exception<'s>(
    scope: &mut v8::HandleScope<'s>,
    message: &str,
) -> v8::Local<'s, v8::Value> {
    runtime_error_exception_with_code(scope, message, "ERR_AGENTOS_BRIDGE_RUNTIME")
}

fn runtime_error_exception_with_code<'s>(
    scope: &mut v8::HandleScope<'s>,
    message: &str,
    code: &str,
) -> v8::Local<'s, v8::Value> {
    let message = v8::String::new(scope, message).expect("runtime error message allocation");
    let exception = v8::Exception::error(scope, message);
    let object = exception
        .to_object(scope)
        .expect("Error exceptions are objects");
    let code_key = v8::String::new(scope, "code").expect("code property key");
    let code = v8::String::new(scope, code).expect("runtime error code allocation");
    if object.set(scope, code_key.into(), code.into()) != Some(true) {
        eprintln!(
            "ERR_AGENTOS_BRIDGE_ERROR_PROPERTY: failed to attach the bridge runtime error code"
        );
    }
    exception
}

/// Serialize a V8 value to CBOR bytes.
pub fn serialize_cbor_value(
    scope: &mut v8::HandleScope,
    value: v8::Local<v8::Value>,
) -> Result<Vec<u8>, String> {
    let cbor_val = v8_to_cbor(scope, value)?;
    let mut buf = Vec::new();
    ciborium::into_writer(&cbor_val, &mut buf).map_err(|e| format!("CBOR encode failed: {}", e))?;
    Ok(buf)
}

/// Deserialize CBOR bytes to a V8 value.
pub fn deserialize_cbor_value<'s>(
    scope: &mut v8::HandleScope<'s>,
    data: &[u8],
) -> Result<v8::Local<'s, v8::Value>, String> {
    let LimitedCborValue(cbor_val) =
        ciborium::de::from_reader_with_recursion_limit(data, MAX_CBOR_BRIDGE_DEPTH)
            .map_err(|e| format!("CBOR decode failed: {}", e))?;
    cbor_to_v8(scope, &cbor_val)
}

/// The active operation is owned independently of callback invocations. A callback
/// clones this Rc before entering guest code, so serialization/reentry never holds
/// a RefCell borrow and cannot invalidate the operation it is using.
struct ActiveBridgeExecution {
    context: Rc<BridgeCallContext>,
    pending: Rc<PendingPromises>,
}

#[derive(Clone, Default)]
struct ActiveBridgeSlot(Rc<RefCell<Option<Rc<ActiveBridgeExecution>>>>);

/// Clears the isolate's operation route on every Execute exit, including init
/// failures and cancellation. The slot is empty while the retained context is idle.
pub(crate) struct ActiveBridgeExecutionGuard(ActiveBridgeSlot);

impl Drop for ActiveBridgeExecutionGuard {
    fn drop(&mut self) {
        // Release the borrow before dropping the operation and cancelling routes.
        let operation = self.0 .0.borrow_mut().take();
        drop(operation);
    }
}

pub(crate) fn activate_bridge_execution(
    scope: &mut v8::HandleScope,
    context: Rc<BridgeCallContext>,
    pending: Rc<PendingPromises>,
) -> ActiveBridgeExecutionGuard {
    let slot = scope
        .get_slot::<ActiveBridgeSlot>()
        .cloned()
        .unwrap_or_else(|| {
            let slot = ActiveBridgeSlot::default();
            scope.set_slot(slot.clone());
            slot
        });
    assert!(
        slot.0.borrow().is_none(),
        "bridge operations cannot overlap on an isolate"
    );
    *slot.0.borrow_mut() = Some(Rc::new(ActiveBridgeExecution { context, pending }));
    ActiveBridgeExecutionGuard(slot)
}

/// Install callbacks whose immutable method data is owned by V8 itself. Captured
/// functions and apply aliases therefore remain valid for the context lifetime,
/// while each invocation obtains the current operation's response/abort channels.
pub(crate) fn install_session_bridge_fns(
    scope: &mut v8::HandleScope,
    sync_methods: &[&str],
    async_methods: &[&str],
) -> Result<(), String> {
    let scope = &mut v8::TryCatch::new(scope);
    reset_vm_context_registry(scope);
    crate::next_ticks::capture(scope)?;
    let context = scope.get_current_context();
    let global = context.global(scope);
    for (methods, is_async) in [(sync_methods, false), (async_methods, true)] {
        for &method in methods {
            let name = v8::String::new(scope, method).expect("bridge method allocation");
            let template = if is_async {
                v8::FunctionTemplate::builder(async_bridge_callback)
                    .data(name.into())
                    .build(scope)
            } else {
                v8::FunctionTemplate::builder(sync_bridge_callback)
                    .data(name.into())
                    .build(scope)
            };
            let function = template
                .get_function(scope)
                .expect("bridge function allocation");
            let aliases: &[&str] = if is_async {
                &["apply"]
            } else {
                &["applySync", "applySyncPromise"]
            };
            attach_bridge_function_aliases(scope, function, aliases);
            if global.set(scope, name.into(), function.into()) != Some(true) {
                return Err(format!("cannot install bridge function {method}"));
            }
        }
    }
    Ok(())
}

// Direct registration remains available for low-level embedders/tests whose
// explicit lifetime contract is documented on register_*_bridge_fns. Production
// sessions exclusively use the owned Operation variant.
enum BridgeCallbackTarget {
    Operation(Rc<ActiveBridgeExecution>),
    Direct {
        context: *const BridgeCallContext,
        pending: *const PendingPromises,
    },
}

struct BridgeCallbackData {
    method: String,
    target: BridgeCallbackTarget,
}

impl BridgeCallbackData {
    fn context(&self) -> &BridgeCallContext {
        match &self.target {
            BridgeCallbackTarget::Operation(operation) => &operation.context,
            // SAFETY: direct registrants must uphold register_*'s lifetime contract.
            BridgeCallbackTarget::Direct { context, .. } => unsafe { &**context },
        }
    }

    fn pending(&self) -> &PendingPromises {
        match &self.target {
            BridgeCallbackTarget::Operation(operation) => &operation.pending,
            // SAFETY: only async callbacks request pending, and their direct
            // registration requires a live PendingPromises pointer.
            BridgeCallbackTarget::Direct { pending, .. } => unsafe { &**pending },
        }
    }
}

fn bridge_callback_data(
    scope: &mut v8::HandleScope,
    value: v8::Local<v8::Value>,
    is_async: bool,
) -> Result<BridgeCallbackData, (&'static str, &'static str)> {
    if value.is_string() {
        let operation = scope
            .get_slot::<ActiveBridgeSlot>()
            .and_then(|slot| slot.0.borrow().clone())
            .ok_or((
                "bridge function called without an active execution",
                "ERR_AGENTOS_BRIDGE_INACTIVE",
            ))?;
        return Ok(BridgeCallbackData {
            method: value.to_rust_string_lossy(scope),
            target: BridgeCallbackTarget::Operation(operation),
        });
    }
    let external = v8::Local::<v8::External>::try_from(value).map_err(|_| {
        (
            "internal error: missing bridge function data",
            "ERR_AGENTOS_BRIDGE_RUNTIME",
        )
    })?;
    if is_async {
        // SAFETY: the direct registration caller keeps its store alive.
        let data = unsafe { &*(external.value() as *const AsyncBridgeFnData) };
        Ok(BridgeCallbackData {
            method: data.method.clone(),
            target: BridgeCallbackTarget::Direct {
                context: data.ctx,
                pending: data.pending,
            },
        })
    } else {
        // SAFETY: the direct registration caller keeps its store alive.
        let data = unsafe { &*(external.value() as *const SyncBridgeFnData) };
        Ok(BridgeCallbackData {
            method: data.method.clone(),
            target: BridgeCallbackTarget::Direct {
                context: data.ctx,
                pending: std::ptr::null(),
            },
        })
    }
}

/// Data attached to each sync bridge function via v8::External.
/// BridgeFnStore keeps these heap allocations alive for the session.
struct SyncBridgeFnData {
    ctx: *const BridgeCallContext,
    method: String,
}

/// Opaque store that keeps bridge function data alive.
/// Must be held for the lifetime of the V8 context.
pub struct BridgeFnStore {
    // Box ensures stable pointer address for v8::External data when Vec grows
    #[allow(clippy::vec_box)]
    _data: Vec<Box<SyncBridgeFnData>>,
}

/// Data attached to each async bridge function via v8::External.
struct AsyncBridgeFnData {
    ctx: *const BridgeCallContext,
    pending: *const PendingPromises,
    method: String,
}

/// Opaque store that keeps async bridge function data alive.
/// Must be held for the lifetime of the V8 context.
pub struct AsyncBridgeFnStore {
    // Box ensures stable pointer address for v8::External data when Vec grows
    #[allow(clippy::vec_box)]
    _data: Vec<Box<AsyncBridgeFnData>>,
}

/// Stores pending promise resolvers keyed by call_id.
/// Single-threaded: only accessed from the session thread.
pub struct PendingPromises {
    map: RefCell<HashMap<u64, PendingPromise>>,
    reserved: Cell<usize>,
    refed_calls: Cell<usize>,
}

struct PendingPromise {
    resolver: v8::Global<v8::PromiseResolver>,
    keeps_execution_alive: bool,
}

impl PendingPromises {
    pub fn new() -> Self {
        PendingPromises {
            map: RefCell::new(HashMap::new()),
            reserved: Cell::new(0),
            refed_calls: Cell::new(0),
        }
    }

    fn capacity_error(&self) -> Option<String> {
        let len = self.map.borrow().len().saturating_add(self.reserved.get());
        if len >= MAX_PENDING_PROMISES {
            return Some(format!(
                "async bridge pending promise registry exceeded limit of {MAX_PENDING_PROMISES} promises"
            ));
        }
        None
    }

    fn reserve(&self) -> Result<PendingPromiseReservation<'_>, String> {
        if let Some(error) = self.capacity_error() {
            return Err(error);
        }
        self.reserved.set(self.reserved.get().saturating_add(1));
        Ok(PendingPromiseReservation {
            pending: self,
            active: true,
        })
    }

    fn release_reservation(&self) {
        self.reserved.set(self.reserved.get().saturating_sub(1));
    }

    fn insert_reserved(
        &self,
        call_id: u64,
        resolver: v8::Global<v8::PromiseResolver>,
        method: &str,
        mut reservation: PendingPromiseReservation<'_>,
    ) {
        // This bounded readiness wait may survive stdin.pause(). The JS stdin
        // handle owns process liveness; the read remains registered until data
        // arrives or the operation is cancelled. Private IPC readiness likewise uses
        // explicit JS handle refs, so its idle waiter cannot keep an unrefed
        // channel alive. Every other async call keeps
        // its existing drain semantics.
        let keeps_execution_alive = !matches!(method, "_kernelStdinRead" | "_processIpcPoll");
        let replaced = self.map.borrow_mut().insert(
            call_id,
            PendingPromise {
                resolver,
                keeps_execution_alive,
            },
        );
        self.refed_calls.set(
            self.refed_calls.get() + usize::from(keeps_execution_alive)
                - usize::from(replaced.is_some_and(|entry| entry.keeps_execution_alive)),
        );
        reservation.active = false;
        self.release_reservation();
    }

    /// Remove and return the resolver for a given call_id.
    pub fn remove(&self, call_id: u64) -> Option<v8::Global<v8::PromiseResolver>> {
        let entry = self.map.borrow_mut().remove(&call_id)?;
        if entry.keeps_execution_alive {
            self.refed_calls.set(self.refed_calls.get() - 1);
        }
        Some(entry.resolver)
    }

    /// Number of pending promises.
    pub fn len(&self) -> usize {
        self.map.borrow().len()
    }

    /// Whether there are no pending promises.
    pub fn is_empty(&self) -> bool {
        self.map.borrow().is_empty()
    }

    /// Whether an async call independently keeps execution alive. Durable stdin
    /// reads still count against capacity and retain their resolver while idle.
    pub(crate) fn has_refed_calls(&self) -> bool {
        self.refed_calls.get() != 0
    }
}

impl Default for PendingPromises {
    fn default() -> Self {
        Self::new()
    }
}

struct PendingPromiseReservation<'a> {
    pending: &'a PendingPromises,
    active: bool,
}

impl Drop for PendingPromiseReservation<'_> {
    fn drop(&mut self) {
        if self.active {
            self.pending.release_reservation();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ThreadResourceUsageSnapshot {
    user_cpu_us: u64,
    system_cpu_us: u64,
    max_rss_kib: i64,
    shared_memory_size: i64,
    unshared_data_size: i64,
    unshared_stack_size: i64,
    minor_page_faults: i64,
    major_page_faults: i64,
    swapped_out: i64,
    fs_read: i64,
    fs_write: i64,
    ipc_sent: i64,
    ipc_received: i64,
    signals_count: i64,
    voluntary_context_switches: i64,
    involuntary_context_switches: i64,
}

fn non_negative_c_long(value: libc::c_long) -> i64 {
    let normalized = i128::from(value).max(0);
    normalized.min(i128::from(i64::MAX)) as i64
}

// Used only by the non-macOS `getrusage(RUSAGE_THREAD)` path; macOS reads CPU
// time from Mach `time_value_t` instead.
#[cfg(not(target_os = "macos"))]
fn timeval_to_micros(value: libc::timeval) -> u64 {
    let seconds = i128::from(value.tv_sec).max(0);
    let micros = i128::from(value.tv_usec).max(0);
    (seconds
        .saturating_mul(1_000_000)
        .saturating_add(micros)
        .min(i128::from(u64::MAX))) as u64
}

#[cfg(not(target_os = "macos"))]
fn current_thread_resource_usage() -> Result<ThreadResourceUsageSnapshot, String> {
    let mut usage = MaybeUninit::<libc::rusage>::uninit();
    let result = unsafe { libc::getrusage(libc::RUSAGE_THREAD, usage.as_mut_ptr()) };
    if result != 0 {
        return Err(format!(
            "getrusage(RUSAGE_THREAD) failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let usage = unsafe { usage.assume_init() };
    Ok(ThreadResourceUsageSnapshot {
        user_cpu_us: timeval_to_micros(usage.ru_utime),
        system_cpu_us: timeval_to_micros(usage.ru_stime),
        max_rss_kib: non_negative_c_long(usage.ru_maxrss),
        shared_memory_size: non_negative_c_long(usage.ru_ixrss),
        unshared_data_size: non_negative_c_long(usage.ru_idrss),
        unshared_stack_size: non_negative_c_long(usage.ru_isrss),
        minor_page_faults: non_negative_c_long(usage.ru_minflt),
        major_page_faults: non_negative_c_long(usage.ru_majflt),
        swapped_out: non_negative_c_long(usage.ru_nswap),
        fs_read: non_negative_c_long(usage.ru_inblock),
        fs_write: non_negative_c_long(usage.ru_oublock),
        ipc_sent: non_negative_c_long(usage.ru_msgsnd),
        ipc_received: non_negative_c_long(usage.ru_msgrcv),
        signals_count: non_negative_c_long(usage.ru_nsignals),
        voluntary_context_switches: non_negative_c_long(usage.ru_nvcsw),
        involuntary_context_switches: non_negative_c_long(usage.ru_nivcsw),
    })
}

// macOS has no `RUSAGE_THREAD`, so per-thread CPU time comes from the Mach
// `thread_info(THREAD_BASIC_INFO)` call. The remaining rusage fields have no
// per-thread source on Apple platforms, so they are filled best-effort from the
// process-wide `getrusage(RUSAGE_SELF)` (mirroring libuv's macOS behaviour).
#[cfg(target_os = "macos")]
fn macos_thread_cpu_micros() -> Result<(u64, u64), String> {
    // SAFETY: `pthread_mach_thread_np` yields the calling thread's Mach port;
    // `thread_info` fully initialises `info` on KERN_SUCCESS.
    unsafe {
        let port = libc::pthread_mach_thread_np(libc::pthread_self());
        if port == 0 {
            return Err("pthread_mach_thread_np returned MACH_PORT_NULL".to_string());
        }
        let mut info = MaybeUninit::<libc::thread_basic_info>::zeroed();
        let mut count = (std::mem::size_of::<libc::thread_basic_info>()
            / std::mem::size_of::<libc::integer_t>())
            as libc::mach_msg_type_number_t;
        let rc = libc::thread_info(
            port,
            libc::THREAD_BASIC_INFO as libc::thread_flavor_t,
            info.as_mut_ptr() as libc::thread_info_t,
            &mut count,
        );
        if rc != libc::KERN_SUCCESS {
            return Err(format!("thread_info(THREAD_BASIC_INFO) failed: {rc}"));
        }
        let info = info.assume_init();
        let to_micros = |t: libc::time_value_t| -> u64 {
            let secs = i128::from(t.seconds).max(0);
            let micros = i128::from(t.microseconds).max(0);
            (secs
                .saturating_mul(1_000_000)
                .saturating_add(micros)
                .min(i128::from(u64::MAX))) as u64
        };
        Ok((to_micros(info.user_time), to_micros(info.system_time)))
    }
}

#[cfg(target_os = "macos")]
fn current_thread_resource_usage() -> Result<ThreadResourceUsageSnapshot, String> {
    let (user_cpu_us, system_cpu_us) = macos_thread_cpu_micros()?;

    let mut usage = MaybeUninit::<libc::rusage>::uninit();
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if result != 0 {
        return Err(format!(
            "getrusage(RUSAGE_SELF) failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let usage = unsafe { usage.assume_init() };
    Ok(ThreadResourceUsageSnapshot {
        // Per-thread CPU time (accurate, from Mach thread_info).
        user_cpu_us,
        system_cpu_us,
        // macOS reports ru_maxrss in bytes; normalise to KiB to match Linux.
        max_rss_kib: non_negative_c_long(usage.ru_maxrss) / 1024,
        // Process-wide best-effort: no per-thread source on macOS.
        shared_memory_size: non_negative_c_long(usage.ru_ixrss),
        unshared_data_size: non_negative_c_long(usage.ru_idrss),
        unshared_stack_size: non_negative_c_long(usage.ru_isrss),
        minor_page_faults: non_negative_c_long(usage.ru_minflt),
        major_page_faults: non_negative_c_long(usage.ru_majflt),
        swapped_out: non_negative_c_long(usage.ru_nswap),
        fs_read: non_negative_c_long(usage.ru_inblock),
        fs_write: non_negative_c_long(usage.ru_oublock),
        ipc_sent: non_negative_c_long(usage.ru_msgsnd),
        ipc_received: non_negative_c_long(usage.ru_msgrcv),
        signals_count: non_negative_c_long(usage.ru_nsignals),
        voluntary_context_switches: non_negative_c_long(usage.ru_nvcsw),
        involuntary_context_switches: non_negative_c_long(usage.ru_nivcsw),
    })
}

// Guest crypto is served by pure-Rust crates (RustCrypto), not OpenSSL, so there
// is no live OpenSSL library to query. We still surface a `process.versions.openssl`
// string for guest compatibility, pinned to the OpenSSL release vendored by the
// sidecar (openssl-sys 300.6.0+3.6.2). The browser executor reports the same
// constant so both runtimes present an identical identity.
pub const EMULATED_OPENSSL_VERSION: &str = "3.6.3";

fn set_object_string_property<'s>(
    scope: &mut v8::HandleScope<'s>,
    object: v8::Local<'s, v8::Object>,
    key: &str,
    value: &str,
) {
    let key = v8::String::new(scope, key).expect("V8 string key");
    let value = v8::String::new(scope, value).expect("V8 string value");
    let _ = object.set(scope, key.into(), value.into());
}

fn set_object_number_property<'s>(
    scope: &mut v8::HandleScope<'s>,
    object: v8::Local<'s, v8::Object>,
    key: &str,
    value: f64,
) {
    let key = v8::String::new(scope, key).expect("V8 string key");
    let value = v8::Number::new(scope, value);
    let _ = object.set(scope, key.into(), value.into());
}

fn number_property_or_zero<'s>(
    scope: &mut v8::HandleScope<'s>,
    object: v8::Local<'s, v8::Object>,
    key: &str,
) -> u64 {
    let key = v8::String::new(scope, key).expect("V8 string key");
    object
        .get(scope, key.into())
        .and_then(|value| value.integer_value(scope))
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or_default()
}

fn process_memory_usage_value<'s>(scope: &mut v8::HandleScope<'s>) -> v8::Local<'s, v8::Value> {
    let mut stats = v8::HeapStatistics::default();
    scope.get_heap_statistics(&mut stats);

    let object = v8::Object::new(scope);
    set_object_number_property(scope, object, "rss", stats.total_physical_size() as f64);
    set_object_number_property(scope, object, "heapTotal", stats.total_heap_size() as f64);
    set_object_number_property(scope, object, "heapUsed", stats.used_heap_size() as f64);
    set_object_number_property(scope, object, "external", stats.external_memory() as f64);
    set_object_number_property(
        scope,
        object,
        "arrayBuffers",
        stats.external_memory() as f64,
    );
    object.into()
}

// This reads only the currently executing isolate; it is not process RSS.
fn v8_heap_statistics_value<'s>(
    scope: &mut v8::HandleScope<'s>,
) -> Result<v8::Local<'s, v8::Value>, String> {
    let mut statistics = v8::HeapStatistics::default();
    scope.get_heap_statistics(&mut statistics);
    let object = v8::Object::new(scope);
    for (name, value) in [
        ("total_heap_size", statistics.total_heap_size()),
        (
            "total_heap_size_executable",
            statistics.total_heap_size_executable(),
        ),
        ("total_physical_size", statistics.total_physical_size()),
        ("total_available_size", statistics.total_available_size()),
        ("used_heap_size", statistics.used_heap_size()),
        ("heap_size_limit", statistics.heap_size_limit()),
        ("malloced_memory", statistics.malloced_memory()),
        ("peak_malloced_memory", statistics.peak_malloced_memory()),
        ("does_zap_garbage", statistics.does_zap_garbage()),
        (
            "number_of_native_contexts",
            statistics.number_of_native_contexts(),
        ),
        (
            "number_of_detached_contexts",
            statistics.number_of_detached_contexts(),
        ),
        (
            "total_global_handles_size",
            statistics.total_global_handles_size(),
        ),
        (
            "used_global_handles_size",
            statistics.used_global_handles_size(),
        ),
        ("external_memory", statistics.external_memory()),
    ] {
        let key = v8::String::new(scope, name).ok_or_else(|| {
            "ERR_AGENTOS_V8_HEAP_STATISTICS: could not allocate a field name".to_owned()
        })?;
        let value = v8::Number::new(scope, value as f64);
        // Define own data fields without calling guest prototype setters.
        if object.create_data_property(scope, key.into(), value.into()) != Some(true) {
            return Err(
                "ERR_AGENTOS_V8_HEAP_STATISTICS: could not define a statistics field".to_owned(),
            );
        }
    }
    Ok(object.into())
}

fn process_cpu_usage_value<'s>(
    scope: &mut v8::HandleScope<'s>,
    args: &v8::FunctionCallbackArguments,
) -> Result<v8::Local<'s, v8::Value>, String> {
    let usage = current_thread_resource_usage()?;
    let current_user = usage.user_cpu_us;
    let current_system = usage.system_cpu_us;

    let (user, system) = if args.length() > 0 {
        let prev = args.get(0);
        if prev.is_null_or_undefined() {
            (current_user, current_system)
        } else if let Some(prev) = prev.to_object(scope) {
            let previous_user = number_property_or_zero(scope, prev, "user");
            let previous_system = number_property_or_zero(scope, prev, "system");
            (
                current_user.saturating_sub(previous_user),
                current_system.saturating_sub(previous_system),
            )
        } else {
            (current_user, current_system)
        }
    } else {
        (current_user, current_system)
    };

    let object = v8::Object::new(scope);
    set_object_number_property(scope, object, "user", user as f64);
    set_object_number_property(scope, object, "system", system as f64);
    Ok(object.into())
}

fn process_resource_usage_value<'s>(
    scope: &mut v8::HandleScope<'s>,
) -> Result<v8::Local<'s, v8::Value>, String> {
    let usage = current_thread_resource_usage()?;
    let object = v8::Object::new(scope);
    set_object_number_property(scope, object, "userCPUTime", usage.user_cpu_us as f64);
    set_object_number_property(scope, object, "systemCPUTime", usage.system_cpu_us as f64);
    set_object_number_property(scope, object, "maxRSS", usage.max_rss_kib as f64);
    set_object_number_property(
        scope,
        object,
        "sharedMemorySize",
        usage.shared_memory_size as f64,
    );
    set_object_number_property(
        scope,
        object,
        "unsharedDataSize",
        usage.unshared_data_size as f64,
    );
    set_object_number_property(
        scope,
        object,
        "unsharedStackSize",
        usage.unshared_stack_size as f64,
    );
    set_object_number_property(
        scope,
        object,
        "minorPageFault",
        usage.minor_page_faults as f64,
    );
    set_object_number_property(
        scope,
        object,
        "majorPageFault",
        usage.major_page_faults as f64,
    );
    set_object_number_property(scope, object, "swappedOut", usage.swapped_out as f64);
    set_object_number_property(scope, object, "fsRead", usage.fs_read as f64);
    set_object_number_property(scope, object, "fsWrite", usage.fs_write as f64);
    set_object_number_property(scope, object, "ipcSent", usage.ipc_sent as f64);
    set_object_number_property(scope, object, "ipcReceived", usage.ipc_received as f64);
    set_object_number_property(scope, object, "signalsCount", usage.signals_count as f64);
    set_object_number_property(
        scope,
        object,
        "voluntaryContextSwitches",
        usage.voluntary_context_switches as f64,
    );
    set_object_number_property(
        scope,
        object,
        "involuntaryContextSwitches",
        usage.involuntary_context_switches as f64,
    );
    Ok(object.into())
}

fn process_versions_value<'s>(scope: &mut v8::HandleScope<'s>) -> v8::Local<'s, v8::Value> {
    let object = v8::Object::new(scope);
    set_object_string_property(scope, object, "v8", v8::V8::get_version());
    set_object_string_property(scope, object, "openssl", EMULATED_OPENSSL_VERSION);
    object.into()
}

#[derive(Clone)]
struct VmContextState {
    context: v8::Global<v8::Context>,
}

#[derive(Clone, Debug)]
struct VmRunOptions {
    filename: String,
    line_offset: i32,
    column_offset: i32,
    timeout_ms: Option<u32>,
}

impl Default for VmRunOptions {
    fn default() -> Self {
        Self {
            filename: String::from("evalmachine.<anonymous>"),
            line_offset: 0,
            column_offset: 0,
            timeout_ms: None,
        }
    }
}

// Registry ownership follows the isolate, not its reusable worker thread.
// Retained calls keep these IDs; explicit reset and isolate disposal release them.
struct VmContextRegistryState {
    contexts: HashMap<u32, VmContextState>,
    next_id: u32,
}

#[derive(Clone)]
struct VmContextRegistry(Rc<RefCell<VmContextRegistryState>>);

fn vm_context_registry(isolate: &mut v8::Isolate) -> VmContextRegistry {
    if let Some(registry) = isolate.get_slot::<VmContextRegistry>() {
        return registry.clone();
    }
    let registry = VmContextRegistry(Rc::new(RefCell::new(VmContextRegistryState {
        contexts: HashMap::new(),
        next_id: 1,
    })));
    isolate.set_slot(registry.clone());
    registry
}

fn vm_context_capacity_error(current_contexts: usize) -> Option<String> {
    if current_contexts >= MAX_VM_CONTEXTS {
        return Some(format!(
            "node:vm context registry exceeded limit of {MAX_VM_CONTEXTS} contexts"
        ));
    }
    None
}

fn reserve_vm_context_slot<'s>(
    scope: &mut v8::HandleScope<'s>,
    context: v8::Local<'s, v8::Context>,
) -> Result<u32, String> {
    let registry = vm_context_registry(scope);
    let mut state = registry.0.borrow_mut();
    if let Some(error) = vm_context_capacity_error(state.contexts.len()) {
        return Err(error);
    }
    let context_id = state.next_id;
    state.next_id = context_id.checked_add(1).unwrap_or(1).max(1);
    state.contexts.insert(
        context_id,
        VmContextState {
            context: v8::Global::new(scope, context),
        },
    );
    Ok(context_id)
}

fn remove_vm_context_slot(isolate: &mut v8::Isolate, context_id: u32) {
    if let Some(registry) = isolate.get_slot::<VmContextRegistry>() {
        registry.0.borrow_mut().contexts.remove(&context_id);
    }
}

/// Release this isolate's context IDs at an explicit execution boundary.
/// Retained calls do not reset: their contextified objects keep stable IDs.
fn reset_vm_context_registry(isolate: &mut v8::Isolate) {
    crate::node_v8_deserializer::reset(isolate);
    if let Some(registry) = isolate.get_slot::<VmContextRegistry>() {
        let mut state = registry.0.borrow_mut();
        state.contexts.clear();
        state.next_id = 1;
    }
}

/// Drop every registry Global while the isolate is alive, before custom queues.
/// Removing only this slot cannot disturb another isolate on the same thread.
pub(crate) fn release_vm_context_registry(isolate: &mut v8::Isolate) {
    crate::node_v8_deserializer::release(isolate);
    if let Some(registry) = isolate.remove_slot::<VmContextRegistry>() {
        registry.0.borrow_mut().contexts.clear();
    }
}

/// Optional owner for an explicit non-retained registry boundary. The weak
/// reference cannot keep the registry or its Globals alive after isolate disposal.
#[must_use = "hold the guard for its explicit registry lifetime"]
pub struct VmContextRegistryGuard {
    contexts: std::rc::Weak<RefCell<VmContextRegistryState>>,
    _readers: crate::node_v8_deserializer::RegistryGuard,
}

impl VmContextRegistryGuard {
    pub fn new(isolate: &mut v8::Isolate) -> Self {
        reset_vm_context_registry(isolate);
        Self { contexts: Rc::downgrade(&vm_context_registry(isolate).0), _readers: crate::node_v8_deserializer::RegistryGuard::new(isolate) }
    }
}

impl Drop for VmContextRegistryGuard {
    fn drop(&mut self) {
        if let Some(registry) = self.contexts.upgrade() {
            let mut state = registry.borrow_mut();
            state.contexts.clear();
            state.next_id = 1;
        }
    }
}

#[cfg(test)]
pub(crate) fn clear_vm_context_registry_for_test(isolate: &mut v8::Isolate) {
    reset_vm_context_registry(isolate);
}

#[cfg(test)]
pub(crate) fn create_registered_child_for_test<'s>(
    scope: &mut v8::HandleScope<'s>,
) -> v8::Local<'s, v8::Context> {
    let context = crate::owned_microtasks::create_child(scope, Default::default())
        .expect("owned child admission");
    reserve_vm_context_slot(scope, context).expect("child registry admission");
    context
}

#[cfg(test)]
fn fill_vm_context_registry_for_test<'s>(
    scope: &mut v8::HandleScope<'s>,
    context: v8::Local<'s, v8::Context>,
    count: usize,
) {
    clear_vm_context_registry_for_test(scope);
    for _ in 0..count {
        reserve_vm_context_slot(scope, context).expect("fill vm context test registry");
    }
}

#[cfg(test)]
fn vm_context_registry_len_for_test(isolate: &mut v8::Isolate) -> usize {
    isolate
        .get_slot::<VmContextRegistry>()
        .map_or(0, |registry| registry.0.borrow().contexts.len())
}

#[cfg(test)]
pub(crate) fn vm_context_registry_len_for_isolate_test(isolate: &mut v8::Isolate) -> usize {
    vm_context_registry_len_for_test(isolate)
}

// Contextification follows Node.js src/node_contextify.cc (MIT):
// Copyright Joyent, Inc. and other Node contributors.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit
// persons to whom the Software is furnished to do so, subject to the
// following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
// NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
// OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
// USE OR OTHER DEALINGS IN THE SOFTWARE.

// Stop property routing immediately when a guest accessor throws or terminates.
// The surrounding property callback rethrows to its original caller, never
// falling back to a builtin with an outstanding exception.
macro_rules! vm_property_step {
    ($tc:ident, $operation:expr) => {{
        let result = $operation;
        if $tc.has_caught() || $tc.has_terminated() {
            return v8::Intercepted::No;
        }
        result
    }};
}

macro_rules! vm_property_callback {
    ($scope:expr, $args:expr, |$tc:ident, $sandbox:ident, $global:ident| $body:block) => {{
        let $tc = &mut v8::TryCatch::new($scope);
        let result = (|| {
            // Only these native callbacks can reach this ordinary dense array.
            // [sandbox, ready] is traced by V8; no Rust address or Global is kept.
            let binding = v8::Local::<v8::Array>::try_from($args.data())
                .expect("native contextification binding");
            let ready = vm_property_step!($tc, binding.get_index($tc, 1));
            if !ready.is_some_and(|value| value.is_true()) {
                return v8::Intercepted::No;
            }
            let sandbox_value = vm_property_step!($tc, binding.get_index($tc, 0))
                .expect("native contextification sandbox field");
            let $sandbox = v8::Local::<v8::Object>::try_from(sandbox_value)
                .expect("native contextification sandbox object");
            let context = vm_property_step!($tc, $args.holder().get_creation_context($tc))
                .expect("contextified global creation context");
            let $global = context.global($tc);
            // Receiver accesses may originate in the parent. Route operations
            // in the child's context, as Node contextify does.
            let context_scope = &mut v8::ContextScope::new($tc, context);
            let $tc = &mut v8::TryCatch::new(context_scope);
            let result = (|| $body)();
            if $tc.has_caught() || $tc.has_terminated() {
                assert!(
                    $tc.rethrow().is_some(),
                    "property exception can be rethrown"
                );
                v8::Intercepted::Yes
            } else {
                result
            }
        })();
        if $tc.has_caught() || $tc.has_terminated() {
            assert!(
                $tc.rethrow().is_some(),
                "property exception can be rethrown"
            );
            v8::Intercepted::Yes
        } else {
            result
        }
    }};
}

fn vm_named_get<'s>(
    scope: &mut v8::HandleScope<'s>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue,
) -> v8::Intercepted {
    vm_property_callback!(scope, args, |tc, sandbox, global| {
        let value = vm_property_step!(tc, sandbox.get_real_named_property(tc, name));
        let value = match value {
            Some(value) => Some(value),
            None => vm_property_step!(tc, global.get_real_named_property(tc, name)),
        };
        if let Some(value) = value {
            rv.set(if value.strict_equals(sandbox.into()) {
                global.into()
            } else {
                value
            });
            v8::Intercepted::Yes
        } else {
            v8::Intercepted::No
        }
    })
}

fn vm_named_query<'s>(
    scope: &mut v8::HandleScope<'s>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue<v8::Integer>,
) -> v8::Intercepted {
    vm_property_callback!(scope, args, |tc, sandbox, global| {
        for object in [sandbox, global] {
            let has = vm_property_step!(tc, object.has_real_named_property(tc, name));
            if has == Some(true) {
                if let Some(attributes) =
                    vm_property_step!(tc, object.get_real_named_property_attributes(tc, name))
                {
                    rv.set_uint32(attributes.as_u32());
                    return v8::Intercepted::Yes;
                }
                return v8::Intercepted::No;
            }
        }
        v8::Intercepted::No
    })
}

fn vm_named_set<'s>(
    scope: &mut v8::HandleScope<'s>,
    name: v8::Local<'s, v8::Name>,
    value: v8::Local<'s, v8::Value>,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue<()>,
) -> v8::Intercepted {
    vm_property_callback!(scope, args, |tc, sandbox, global| {
        let global_attributes =
            vm_property_step!(tc, global.get_real_named_property_attributes(tc, name));
        let sandbox_attributes =
            vm_property_step!(tc, sandbox.get_real_named_property_attributes(tc, name));
        if global_attributes
            .as_ref()
            .is_some_and(v8::PropertyAttribute::is_read_only)
            || sandbox_attributes
                .as_ref()
                .is_some_and(v8::PropertyAttribute::is_read_only)
        {
            return v8::Intercepted::No;
        }
        let declared = global_attributes.is_some() || sandbox_attributes.is_some();
        let contextual = !global.strict_equals(args.this().into());
        if !declared && args.should_throw_on_error() && contextual && !value.is_function() {
            return v8::Intercepted::No;
        }
        if !declared && name.is_symbol() {
            return v8::Intercepted::No;
        }
        if vm_property_step!(tc, sandbox.set(tc, name.into(), value)) != Some(true) {
            // V8's normal store supplies the readonly/strict-mode outcome.
            return v8::Intercepted::No;
        }
        if sandbox_attributes.is_some() {
            if let Some(descriptor) =
                vm_property_step!(tc, sandbox.get_own_property_descriptor(tc, name))
            {
                if let Ok(descriptor) = v8::Local::<v8::Object>::try_from(descriptor) {
                    for field in ["get", "set"] {
                        let key = v8::String::new(tc, field).expect("descriptor field name");
                        if vm_property_step!(tc, descriptor.has_own_property(tc, key.into()))
                            == Some(true)
                        {
                            rv.set_bool(true);
                            return v8::Intercepted::Yes;
                        }
                    }
                }
            }
        }
        // Match Node's contextual-store/definer fallthrough, including the
        // ordinary global's declaration attributes.
        v8::Intercepted::No
    })
}

fn vm_named_descriptor<'s>(
    scope: &mut v8::HandleScope<'s>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue,
) -> v8::Intercepted {
    vm_property_callback!(scope, args, |tc, sandbox, _global| {
        if vm_property_step!(tc, sandbox.has_own_property(tc, name)) == Some(true) {
            if let Some(descriptor) =
                vm_property_step!(tc, sandbox.get_own_property_descriptor(tc, name))
            {
                rv.set(descriptor);
                return v8::Intercepted::Yes;
            }
        }
        v8::Intercepted::No
    })
}

fn vm_named_define<'s>(
    scope: &mut v8::HandleScope<'s>,
    name: v8::Local<'s, v8::Name>,
    descriptor: &v8::PropertyDescriptor,
    args: v8::PropertyCallbackArguments<'s>,
    _rv: v8::ReturnValue<()>,
) -> v8::Intercepted {
    vm_property_callback!(scope, args, |tc, sandbox, global| {
        let attributes = vm_property_step!(tc, global.get_real_named_property_attributes(tc, name));
        if attributes
            .as_ref()
            .is_some_and(|attr| attr.is_read_only() && attr.is_dont_delete())
        {
            return v8::Intercepted::No;
        }
        let undefined = v8::undefined(tc).into();
        let mut forwarded = if descriptor.has_get() || descriptor.has_set() {
            v8::PropertyDescriptor::new_from_get_set(
                if descriptor.has_get() {
                    descriptor.get()
                } else {
                    undefined
                },
                if descriptor.has_set() {
                    descriptor.set()
                } else {
                    undefined
                },
            )
        } else {
            let value = if descriptor.has_value() {
                descriptor.value()
            } else {
                undefined
            };
            if descriptor.has_writable() {
                v8::PropertyDescriptor::new_from_value_writable(value, descriptor.writable())
            } else {
                v8::PropertyDescriptor::new_from_value(value)
            }
        };
        if descriptor.has_enumerable() {
            forwarded.set_enumerable(descriptor.enumerable());
        }
        if descriptor.has_configurable() {
            forwarded.set_configurable(descriptor.configurable());
        }
        match vm_property_step!(tc, sandbox.define_property(tc, name, &forwarded)) {
            // Node intentionally allows the real global definition after this
            // operation; its current definer fallthrough is observable behavior.
            Some(true) | Some(false) | None => v8::Intercepted::No,
        }
    })
}

fn vm_named_delete<'s>(
    scope: &mut v8::HandleScope<'s>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
    vm_property_callback!(scope, args, |tc, sandbox, _global| {
        if vm_property_step!(tc, sandbox.delete(tc, name.into())) == Some(true) {
            v8::Intercepted::No
        } else {
            rv.set_bool(false);
            v8::Intercepted::Yes
        }
    })
}

fn vm_named_enumerate<'s>(
    scope: &mut v8::HandleScope<'s>,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue<v8::Array>,
) {
    vm_property_callback!(scope, args, |tc, sandbox, _global| {
        if let Some(names) = vm_property_step!(
            tc,
            sandbox.get_property_names(
                tc,
                v8::GetPropertyNamesArgs {
                    mode: v8::KeyCollectionMode::OwnOnly,
                    property_filter: v8::PropertyFilter::ALL_PROPERTIES,
                    index_filter: v8::IndexFilter::SkipIndices,
                    key_conversion: v8::KeyConversionMode::KeepNumbers,
                }
            )
        ) {
            rv.set(names);
        }
        v8::Intercepted::Yes
    });
}

fn vm_indexed_enumerate<'s>(
    scope: &mut v8::HandleScope<'s>,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue<v8::Array>,
) {
    vm_property_callback!(scope, args, |tc, sandbox, _global| {
        if let Some(names) = vm_property_step!(
            tc,
            sandbox.get_property_names(
                tc,
                v8::GetPropertyNamesArgs {
                    mode: v8::KeyCollectionMode::OwnOnly,
                    property_filter: v8::PropertyFilter::SKIP_SYMBOLS,
                    index_filter: v8::IndexFilter::IncludeIndices,
                    key_conversion: v8::KeyConversionMode::KeepNumbers,
                }
            )
        ) {
            let indices = v8::Array::new(tc, 0);
            let mut next = 0;
            for index in 0..names.length() {
                let Some(name) = vm_property_step!(tc, names.get_index(tc, index)) else {
                    return v8::Intercepted::No;
                };
                if name.is_uint32() {
                    let key = v8::Integer::new_from_unsigned(tc, next);
                    let Some(key) = vm_property_step!(tc, key.to_string(tc)) else {
                        return v8::Intercepted::No;
                    };
                    if vm_property_step!(tc, indices.create_data_property(tc, key.into(), name))
                        != Some(true)
                    {
                        return v8::Intercepted::No;
                    }
                    next += 1;
                }
            }
            rv.set(indices);
        }
        v8::Intercepted::Yes
    });
}

macro_rules! vm_indexed_callback {
    ($scope:expr, $index:expr, |$tc:ident, $name:ident| $body:expr) => {{
        let $tc = &mut v8::TryCatch::new($scope);
        let result = if let Some($name) = v8::Integer::new_from_unsigned($tc, $index).to_string($tc)
        {
            $body
        } else {
            v8::Intercepted::No
        };
        if $tc.has_caught() || $tc.has_terminated() {
            assert!(
                $tc.rethrow().is_some(),
                "indexed property exception can be rethrown"
            );
            v8::Intercepted::Yes
        } else {
            result
        }
    }};
}

fn vm_indexed_get<'s>(
    scope: &mut v8::HandleScope<'s>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue,
) -> v8::Intercepted {
    vm_indexed_callback!(scope, index, |tc, name| vm_named_get(
        tc,
        name.into(),
        args,
        rv
    ))
}
fn vm_indexed_query<'s>(
    scope: &mut v8::HandleScope<'s>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<v8::Integer>,
) -> v8::Intercepted {
    vm_indexed_callback!(scope, index, |tc, name| vm_named_query(
        tc,
        name.into(),
        args,
        rv
    ))
}
fn vm_indexed_set<'s>(
    scope: &mut v8::HandleScope<'s>,
    index: u32,
    value: v8::Local<'s, v8::Value>,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<()>,
) -> v8::Intercepted {
    vm_indexed_callback!(scope, index, |tc, name| vm_named_set(
        tc,
        name.into(),
        value,
        args,
        rv
    ))
}
fn vm_indexed_descriptor<'s>(
    scope: &mut v8::HandleScope<'s>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue,
) -> v8::Intercepted {
    vm_indexed_callback!(scope, index, |tc, name| vm_named_descriptor(
        tc,
        name.into(),
        args,
        rv
    ))
}
fn vm_indexed_define<'s>(
    scope: &mut v8::HandleScope<'s>,
    index: u32,
    descriptor: &v8::PropertyDescriptor,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<()>,
) -> v8::Intercepted {
    vm_indexed_callback!(scope, index, |tc, name| vm_named_define(
        tc,
        name.into(),
        descriptor,
        args,
        rv
    ))
}
fn vm_indexed_delete<'s>(
    scope: &mut v8::HandleScope<'s>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    rv: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
    vm_indexed_callback!(scope, index, |tc, name| vm_named_delete(
        tc,
        name.into(),
        args,
        rv
    ))
}

fn vm_context_template<'s>(
    scope: &mut v8::HandleScope<'s>,
    sandbox: v8::Local<'s, v8::Object>,
) -> (v8::Local<'s, v8::ObjectTemplate>, v8::Local<'s, v8::Array>) {
    let ready = v8::Boolean::new(scope, false);
    let binding = v8::Array::new_with_elements(scope, &[sandbox.into(), ready.into()]);
    let template = v8::ObjectTemplate::new(scope);
    template.set_named_property_handler(
        v8::NamedPropertyHandlerConfiguration::new()
            .getter(vm_named_get)
            .setter(vm_named_set)
            .query(vm_named_query)
            .descriptor(vm_named_descriptor)
            .definer(vm_named_define)
            .deleter(vm_named_delete)
            .enumerator(vm_named_enumerate)
            .data(binding.into()),
    );
    template.set_indexed_property_handler(
        v8::IndexedPropertyHandlerConfiguration::new()
            .getter(vm_indexed_get)
            .setter(vm_indexed_set)
            .query(vm_indexed_query)
            .descriptor(vm_indexed_descriptor)
            .definer(vm_indexed_define)
            .deleter(vm_indexed_delete)
            .enumerator(vm_indexed_enumerate)
            .data(binding.into()),
    );
    (template, binding)
}

fn vm_options_from_value<'s>(
    scope: &mut v8::HandleScope<'s>,
    value: v8::Local<'s, v8::Value>,
) -> VmRunOptions {
    if value.is_null_or_undefined() {
        return VmRunOptions::default();
    }
    if value.is_string() {
        return VmRunOptions {
            filename: value.to_rust_string_lossy(scope),
            ..VmRunOptions::default()
        };
    }
    let Some(options) = value.to_object(scope) else {
        return VmRunOptions::default();
    };
    let mut result = VmRunOptions::default();
    let read_string = |scope: &mut v8::HandleScope<'s>, key: &str| {
        let key_value = v8::String::new(scope, key).expect("V8 string key");
        options
            .get(scope, key_value.into())
            .filter(|value| value.is_string())
            .map(|value| value.to_rust_string_lossy(scope))
    };
    let read_i32 = |scope: &mut v8::HandleScope<'s>, key: &str| {
        let key_value = v8::String::new(scope, key).expect("V8 string key");
        options
            .get(scope, key_value.into())
            .and_then(|value| value.int32_value(scope))
    };
    let read_u32 = |scope: &mut v8::HandleScope<'s>, key: &str| {
        let key_value = v8::String::new(scope, key).expect("V8 string key");
        options
            .get(scope, key_value.into())
            .and_then(|value| value.integer_value(scope))
            .and_then(|value| u32::try_from(value).ok())
    };

    if let Some(filename) = read_string(scope, "filename") {
        result.filename = filename;
    }
    if let Some(line_offset) = read_i32(scope, "lineOffset") {
        result.line_offset = line_offset;
    }
    if let Some(column_offset) = read_i32(scope, "columnOffset") {
        result.column_offset = column_offset;
    }
    result.timeout_ms = read_u32(scope, "timeout").filter(|timeout_ms| *timeout_ms > 0);
    result
}

fn vm_throw_error<'s>(
    scope: &mut v8::HandleScope<'s>,
    message: &str,
    code: Option<&str>,
    type_error: bool,
) -> v8::Local<'s, v8::Value> {
    let message_value = v8::String::new(scope, message).expect("V8 error message");
    let exception = if type_error {
        v8::Exception::type_error(scope, message_value)
    } else {
        v8::Exception::error(scope, message_value)
    };
    if let Some(code) = code {
        if let Some(exception_object) = exception.to_object(scope) {
            let code_key = v8::String::new(scope, "code").expect("V8 code key");
            let code_value = v8::String::new(scope, code).expect("V8 code value");
            let _ = exception_object.set(scope, code_key.into(), code_value.into());
        }
    }
    scope.throw_exception(exception);
    exception
}

fn vm_run_script_in_context<'s>(
    scope: &mut v8::HandleScope<'s>,
    isolate_handle: v8::IsolateHandle,
    context: v8::Local<'s, v8::Context>,
    code: &str,
    options: &VmRunOptions,
    runtime: Option<&agentos_driver_tokio::DriverHandle>,
    task_owner: Option<agentos_driver_tokio::TaskOwner>,
) -> Result<v8::Local<'s, v8::Value>, String> {
    let mut timeout_guard = match options.timeout_ms {
        Some(timeout_ms) => {
            let runtime = runtime.ok_or_else(|| {
                format!(
                    "{}: node:vm timeout requires the session runtime context",
                    crate::timeout::TIMEOUT_GUARD_START_ERROR_CODE
                )
            })?;
            let (abort_tx, _abort_rx) = crossbeam_channel::bounded::<()>(0);
            Some(crate::timeout::TimeoutGuard::new(
                runtime,
                task_owner,
                timeout_ms,
                isolate_handle.clone(),
                abort_tx,
            )?)
        }
        None => None,
    };

    let mut result = None;
    let mut exception = None;
    {
        let context_scope = &mut v8::ContextScope::new(scope, context);
        let tc = &mut v8::TryCatch::new(context_scope);
        let source = v8::String::new(tc, code)
            .ok_or_else(|| String::from("vm source string too large for V8"))?;
        let filename = v8::String::new(tc, &options.filename)
            .ok_or_else(|| String::from("vm filename too large for V8"))?;
        let origin = v8::ScriptOrigin::new(
            tc,
            filename.into(),
            options.line_offset,
            options.column_offset,
            false,
            -1,
            None,
            false,
            false,
            false,
            None,
        );
        match v8::Script::compile(tc, source, Some(&origin)) {
            Some(script) => match script.run(tc) {
                Some(value) => {
                    crate::owned_microtasks::checkpoint(tc);
                    if tc.has_terminated() && crate::terminal_callback::pending(tc) {
                        if let Some(guard) = timeout_guard.as_mut() {
                            guard.cancel();
                        }
                        assert!(
                            tc.rethrow().is_some(),
                            "owned vm termination can be rethrown"
                        );
                        return Err("native terminal stop propagated through node:vm".into());
                    }
                    if let Some(thrown) = tc.exception() {
                        exception = Some(v8::Global::new(tc, thrown));
                    } else {
                        result = Some(v8::Global::new(tc, value));
                    }
                }
                None => {
                    if tc.has_terminated() && crate::terminal_callback::pending(tc) {
                        if let Some(guard) = timeout_guard.as_mut() {
                            guard.cancel();
                        }
                        assert!(
                            tc.rethrow().is_some(),
                            "owned vm termination can be rethrown"
                        );
                        return Err("native terminal stop propagated through node:vm".into());
                    }
                    let failure_message = v8::String::new(tc, "vm script execution failed")
                        .expect("vm failure message");
                    let thrown = tc
                        .exception()
                        .unwrap_or_else(|| v8::Exception::error(tc, failure_message));
                    exception = Some(v8::Global::new(tc, thrown));
                }
            },
            None => {
                if tc.has_terminated() && crate::terminal_callback::pending(tc) {
                    if let Some(guard) = timeout_guard.as_mut() {
                        guard.cancel();
                    }
                    assert!(
                        tc.rethrow().is_some(),
                        "owned vm termination can be rethrown"
                    );
                    return Err("native terminal stop propagated through node:vm".into());
                }
                let failure_message = v8::String::new(tc, "vm script compilation failed")
                    .expect("vm failure message");
                let thrown = tc
                    .exception()
                    .unwrap_or_else(|| v8::Exception::error(tc, failure_message));
                exception = Some(v8::Global::new(tc, thrown));
            }
        }
    }

    let timed_out = if let Some(ref mut guard) = timeout_guard {
        guard.cancel();
        guard.timed_out()
    } else {
        false
    };

    if timed_out {
        isolate_handle.cancel_terminate_execution();
        return Ok(vm_throw_error(
            scope,
            &format!(
                "Script execution timed out after {}ms",
                options.timeout_ms.unwrap_or_default()
            ),
            Some("ERR_SCRIPT_EXECUTION_TIMEOUT"),
            false,
        ));
    }

    if let Some(exception) = exception {
        // Preserve the actual thrown value, including object identity, marker
        // fields and native Error stack. Do not convert a getter failure into a
        // missing-property result or reconstruct an unrelated Error object.
        let exception = v8::Local::new(scope, &exception);
        scope.throw_exception(exception);
        return Ok(exception);
    }

    Ok(result
        .map(|result| v8::Local::new(scope, &result))
        .unwrap_or_else(|| v8::undefined(scope).into()))
}

// Context::new also restores the isolate's default snapshot when a custom
// global template is supplied. Clear only the original host-backed globals
// before activating sandbox routing; user-supplied properties still take precedence.
fn vm_scrub_child_globals(scope: &mut v8::HandleScope) -> Result<(), String> {
    let tc = &mut v8::TryCatch::new(scope);
    let global = tc.get_current_context().global(tc);
    for name in [
        "Buffer",
        "require",
        "process",
        "module",
        "exports",
        "__dirname",
        "__filename",
    ] {
        let key = v8::String::new(tc, name);
        if tc.has_caught() || tc.has_terminated() {
            assert!(
                tc.rethrow().is_some(),
                "child initialization exception can be rethrown"
            );
            return Err("node:vm child global cleanup interrupted".into());
        }
        let Some(key) = key else {
            return Err("node:vm child global name allocation failed".into());
        };
        let deleted = global.delete(tc, key.into());
        if tc.has_caught() || tc.has_terminated() {
            assert!(
                tc.rethrow().is_some(),
                "child initialization exception can be rethrown"
            );
            return Err("node:vm child global cleanup interrupted".into());
        }
        if deleted.is_none() {
            return Err(format!("node:vm child global cleanup failed for {name}"));
        }
        let undefined = v8::undefined(tc);
        let assigned = global.set(tc, key.into(), undefined.into());
        if tc.has_caught() || tc.has_terminated() {
            assert!(
                tc.rethrow().is_some(),
                "child initialization exception can be rethrown"
            );
            return Err("node:vm child global cleanup interrupted".into());
        }
        if assigned != Some(true) {
            return Err(format!("node:vm child global reset failed for {name}"));
        }
    }
    Ok(())
}

fn vm_create_context_value<'s>(
    scope: &mut v8::HandleScope<'s>,
    args: &mut v8::FunctionCallbackArguments<'s>,
) -> Result<v8::Local<'s, v8::Value>, String> {
    let sandbox_value = args.get(0);
    if !(sandbox_value.is_object() || sandbox_value.is_function()) {
        return Ok(vm_throw_error(
            scope,
            "The \"object\" argument must be of type object.",
            None,
            true,
        ));
    }
    let sandbox = sandbox_value
        .to_object(scope)
        .ok_or_else(|| String::from("vm.createContext expected an object sandbox"))?;
    let (template, binding) = vm_context_template(scope, sandbox);
    let context = match crate::owned_microtasks::create_child(
        scope,
        v8::ContextOptions {
            global_template: Some(template),
            ..Default::default()
        },
    ) {
        Ok(context) => context,
        Err(crate::owned_microtasks::ContextAdmissionError::Limit(message)) => {
            return Ok(vm_throw_error(
                scope,
                &message,
                Some("ERR_AGENTOS_VM_CONTEXT_LIMIT"),
                false,
            ));
        }
        Err(error @ crate::owned_microtasks::ContextAdmissionError::Interrupted) => {
            return Err(error.to_string())
        }
    };
    // Match Node's contextify environment token for this guest family only.
    let parent = scope.get_current_context();
    let token = parent.get_security_token(scope);
    context.set_security_token(token);
    let context_id = match reserve_vm_context_slot(scope, context) {
        Ok(context_id) => context_id,
        Err(message) => {
            return Ok(vm_throw_error(
                scope,
                &message,
                Some("ERR_AGENTOS_VM_CONTEXT_LIMIT"),
                false,
            ))
        }
    };
    let scrubbed = {
        let child_scope = &mut v8::ContextScope::new(scope, context);
        vm_scrub_child_globals(child_scope)
    };
    if let Err(error) = scrubbed {
        remove_vm_context_slot(scope, context_id);
        return Err(error);
    }
    // V8 has finished creating the real global. Future operations route live
    // through the original sandbox; no getters run just to contextify it.
    let ready = v8::Boolean::new(scope, true);
    if binding.set_index(scope, 1, ready.into()) != Some(true) {
        remove_vm_context_slot(scope, context_id);
        return Err("node:vm native sandbox binding activation failed".into());
    }
    Ok(v8::Integer::new_from_unsigned(scope, context_id).into())
}

fn vm_run_in_context_value<'s>(
    scope: &mut v8::HandleScope<'s>,
    args: &mut v8::FunctionCallbackArguments<'s>,
    bridge_ctx: &BridgeCallContext,
) -> Result<v8::Local<'s, v8::Value>, String> {
    let context_id = args
        .get(0)
        .uint32_value(scope)
        .ok_or_else(|| String::from("vm.runInContext missing context id"))?;
    let code = args.get(1).to_rust_string_lossy(scope);
    let options_value = args.get(2);
    let options = vm_options_from_value(scope, options_value);
    let _sandbox = args
        .get(3)
        .to_object(scope)
        .ok_or_else(|| String::from("vm.runInContext missing sandbox object"))?;
    let isolate_handle = unsafe { args.get_isolate() }.thread_safe_handle();
    let context_global = scope.get_slot::<VmContextRegistry>().and_then(|registry| {
        registry
            .0
            .borrow()
            .contexts
            .get(&context_id)
            .map(|state| state.context.clone())
    });
    let Some(context_global) = context_global else {
        return Ok(vm_throw_error(
            scope,
            "The \"contextifiedObject\" argument must be a vm context.",
            Some("ERR_INVALID_ARG_TYPE"),
            true,
        ));
    };
    let context = v8::Local::new(scope, &context_global);
    vm_run_script_in_context(
        scope,
        isolate_handle,
        context,
        &code,
        &options,
        bridge_ctx.runtime_context(),
        bridge_ctx.timer_task_owner(),
    )
}

fn vm_run_in_this_context_value<'s>(
    scope: &mut v8::HandleScope<'s>,
    args: &mut v8::FunctionCallbackArguments<'s>,
    bridge_ctx: &BridgeCallContext,
) -> Result<v8::Local<'s, v8::Value>, String> {
    let code = args.get(0).to_rust_string_lossy(scope);
    let options_value = args.get(1);
    let options = vm_options_from_value(scope, options_value);
    let context = scope.get_current_context();
    let isolate_handle = unsafe { args.get_isolate() }.thread_safe_handle();
    vm_run_script_in_context(
        scope,
        isolate_handle,
        context,
        &code,
        &options,
        bridge_ctx.runtime_context(),
        bridge_ctx.timer_task_owner(),
    )
}

fn handle_local_bridge_call<'s>(
    scope: &mut v8::HandleScope<'s>,
    method: &str,
    args: &mut v8::FunctionCallbackArguments<'s>,
    bridge_ctx: &BridgeCallContext,
) -> Result<Option<v8::Local<'s, v8::Value>>, String> {
    match method {
        "process.nextTickScheduled" => {
            crate::next_ticks::scheduled(scope)?;
            Ok(Some(v8::undefined(scope).into()))
        }
        "process.memoryUsage" => Ok(Some(process_memory_usage_value(scope))),
        "v8.getHeapStatistics" => v8_heap_statistics_value(scope).map(Some),
        "v8.collectGarbage" => {
            // Collect only this guest isolate. Never ask the host process or another VM to collect.
            scope.low_memory_notification();
            Ok(Some(v8::undefined(scope).into()))
        }
        "zlib.brotliCompressSync" | "zlib.brotliDecompressSync" => {
            let runtime = bridge_ctx
                .runtime_context()
                .ok_or("Brotli requires the session runtime")?;
            let limit = runtime
                .resources()
                .usage(agentos_driver_tokio::accounting::ResourceClass::BridgeResponseBytes)
                .limit
                .ok_or("Brotli requires limits.reactor.maxBridgeResponseBytes")?;
            crate::node_brotli::transform(
                scope,
                args,
                method == "zlib.brotliDecompressSync",
                limit,
                runtime.resources().clone(),
            )
            .map(Some)
        }
        "v8.deserializer.create"
        | "v8.deserializer.readHeader"
        | "v8.deserializer.readValue"
        | "v8.deserializer.readUint32"
        | "v8.deserializer.readUint64"
        | "v8.deserializer.readDouble"
        | "v8.deserializer.readRawBytes"
        | "v8.deserializer.getWireFormatVersion"
        | "v8.deserializer.transferArrayBuffer"
        | "v8.deserializer.readHostObject" => {
            let runtime = bridge_ctx.runtime_context().ok_or("V8 Deserializer requires the session runtime")?;
            let limit = runtime.resources().usage(agentos_driver_tokio::accounting::ResourceClass::BridgeResponseBytes).limit.ok_or("V8 Deserializer requires a response byte limit")?;
            let (limit, path) = bridge_ctx.serialization_limit().map(|bytes| (bytes, "limits.jsRuntime.v8SerializationLimitBytes")).unwrap_or((limit, "limits.reactor.maxBridgeResponseBytes"));
            crate::node_v8_deserializer::call(scope, args, method, limit, path, runtime.resources().clone()).map(Some)
        }
        "uint8array.encode" => crate::uint8array_encoding::encode(scope, args).map(Some),
        "v8.serialize" | "v8.deserialize" => {
            let runtime = bridge_ctx
                .runtime_context()
                .ok_or("V8 serialization requires the session runtime")?;
            let limit = runtime
                .resources()
                .usage(agentos_driver_tokio::accounting::ResourceClass::BridgeResponseBytes)
                .limit
                .ok_or("V8 serialization requires limits.reactor.maxBridgeResponseBytes")?;
            let (limit, path) = bridge_ctx
                .serialization_limit()
                .map(|bytes| (bytes, "limits.jsRuntime.v8SerializationLimitBytes"))
                .unwrap_or((limit, "limits.reactor.maxBridgeResponseBytes"));
            let value = args.get(0);
            if method == "v8.serialize" {
                crate::node_v8_serialization::serialize(scope, value, limit, path).map(Some)
            } else {
                crate::node_v8_serialization::deserialize(scope, value, limit, path).map(Some)
            }
        }
        "process.cpuUsage" => process_cpu_usage_value(scope, args).map(Some),
        "process.resourceUsage" => process_resource_usage_value(scope).map(Some),
        "process.versions" => Ok(Some(process_versions_value(scope))),
        "_vmCreateContext" => vm_create_context_value(scope, args).map(Some),
        "_vmRunInContext" => vm_run_in_context_value(scope, args, bridge_ctx).map(Some),
        "_vmRunInThisContext" => vm_run_in_this_context_value(scope, args, bridge_ctx).map(Some),
        _ => Ok(None),
    }
}

fn read_response_length_argument(
    scope: &mut v8::HandleScope,
    method: &str,
    args: &v8::FunctionCallbackArguments,
) -> Option<usize> {
    let index = match method {
        "fs.readSync" | "_fsReadRaw" => 1,
        "_fsReadFileRangeRaw" => 2,
        "_pythonStdinRead" | "_kernelStdinReadRaw" | "_kernelStdinRead" => 0,
        _ => return None,
    };
    let value = args.get(index).integer_value(scope)?;
    usize::try_from(value).ok()
}

pub(crate) fn declared_bridge_response_bytes(
    method: &str,
    requested_read_bytes: Option<usize>,
) -> usize {
    if let Some(requested_read_bytes) = requested_read_bytes {
        // Raw reads normally return the requested bytes directly, but nested
        // compatibility/deferred routes may preserve the legacy JSON byte
        // shape, whose base64 payload is larger. Admission must cover either
        // representation; concrete response bytes, not this declaration, are
        // what consume the shared response ledger at settlement.
        let payload_bytes = requested_read_bytes
            .checked_add(2)
            .and_then(|rounded| rounded.checked_div(3))
            .and_then(|groups| groups.checked_mul(4))
            .unwrap_or(usize::MAX);
        return payload_bytes.saturating_add(READ_RESPONSE_ENVELOPE_BYTES);
    }
    bridge_contract()
        .response_max_bytes
        .get(method)
        .copied()
        .unwrap_or(DEFAULT_BRIDGE_RESPONSE_MAX_BYTES)
}

fn bridge_response_declaration(
    scope: &mut v8::HandleScope,
    method: &str,
    args: &v8::FunctionCallbackArguments,
) -> usize {
    declared_bridge_response_bytes(method, read_response_length_argument(scope, method, args))
}

/// Register sync-blocking bridge functions on the V8 global object.
///
/// Each registered function, when called from V8:
/// 1. Serializes arguments as a V8 Array via ValueSerializer
/// 2. Sends a BridgeCall over IPC via BridgeCallContext
/// 3. Blocks on read() for the BridgeResponse
/// 4. Returns the V8-deserialized result or throws a V8 exception
///
/// The BridgeCallContext pointer must remain valid for the lifetime of the V8 context.
/// The returned BridgeFnStore must also be kept alive.
pub fn register_sync_bridge_fns(
    scope: &mut v8::HandleScope,
    ctx: *const BridgeCallContext,
    methods: &[&str],
) -> BridgeFnStore {
    let context = scope.get_current_context();
    let global = context.global(scope);
    let mut data = Vec::with_capacity(methods.len());

    for &method_name in methods {
        let boxed = Box::new(SyncBridgeFnData {
            ctx,
            method: method_name.to_string(),
        });
        // Pointer to heap allocation — stable while Box exists in data vec
        let ptr = &*boxed as *const SyncBridgeFnData as *mut c_void;
        data.push(boxed);

        let external = v8::External::new(scope, ptr);
        let template = v8::FunctionTemplate::builder(sync_bridge_callback)
            .data(external.into())
            .build(scope);
        let func = template.get_function(scope).unwrap();
        attach_bridge_function_aliases(scope, func, &["applySync", "applySyncPromise"]);

        let key = v8::String::new(scope, method_name).unwrap();
        global.set(scope, key.into(), func.into());
    }

    BridgeFnStore { _data: data }
}

/// V8 FunctionTemplate callback for sync-blocking bridge calls.
fn sync_bridge_callback<'s>(
    scope: &mut v8::HandleScope<'s>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue,
) {
    let mut args = args;
    let data = match bridge_callback_data(scope, args.data(), false) {
        Ok(data) => data,
        Err((message, code)) => {
            let exception = runtime_error_exception_with_code(scope, message, code);
            scope.throw_exception(exception);
            return;
        }
    };
    let ctx = data.context();

    {
        let tc = &mut v8::TryCatch::new(scope);
        match handle_local_bridge_call(tc, &data.method, &mut args, ctx) {
            Ok(Some(value)) => {
                if tc.has_caught() {
                    let _ = tc.rethrow();
                    return;
                }
                rv.set(value);
                return;
            }
            Ok(None) => {}
            Err(err) => {
                if tc.has_caught() {
                    let _ = tc.rethrow();
                    return;
                }
                let msg = v8::String::new(tc, &format!("bridge runtime error: {err}")).unwrap();
                let exc = v8::Exception::error(tc, msg);
                tc.throw_exception(exc);
                return;
            }
        }
    }

    // Serialize V8 arguments using the Vec released by V8's serializer directly.
    let encoded_args = match serialize_v8_args(scope, &args) {
        Ok(encoded_args) => encoded_args,
        Err(err) => {
            let msg =
                v8::String::new(scope, &format!("bridge serialization error: {}", err)).unwrap();
            let exc = v8::Exception::error(scope, msg);
            scope.throw_exception(exc);
            return;
        }
    };

    // Perform sync-blocking bridge call
    let max_response_bytes = bridge_response_declaration(scope, &data.method, &args);
    match ctx.sync_call_frame_with_max_response_bytes(
        &data.method,
        encoded_args,
        max_response_bytes,
    ) {
        Ok(Some(response)) => {
            if response.status == 1 {
                let exception = bridge_error_exception(scope, &response.payload);
                scope.throw_exception(exception);
                return;
            }
            let v8_val = bridge_response_payload_to_v8(scope, response.status, &response.payload);
            if let Some(val) = v8_val {
                rv.set(val);
            } else {
                let msg = v8::String::new(scope, "bridge response conversion failed").unwrap();
                let exc = v8::Exception::error(scope, msg);
                scope.throw_exception(exc);
            }
        }
        Ok(None) => {
            rv.set_undefined();
        }
        Err(err_msg) => {
            let exc = runtime_error_exception(scope, &err_msg);
            scope.throw_exception(exc);
        }
    }
}

/// Register async promise-returning bridge functions on the V8 global object.
///
/// Each registered function, when called from V8:
/// 1. Creates a v8::PromiseResolver
/// 2. Stores the resolver + call_id in PendingPromises
/// 3. Sends a BridgeCall over IPC (non-blocking write)
/// 4. Returns the promise to V8
///
/// The BridgeCallContext and PendingPromises pointers must remain valid
/// for the lifetime of the V8 context.
pub fn register_async_bridge_fns(
    scope: &mut v8::HandleScope,
    ctx: *const BridgeCallContext,
    pending: *const PendingPromises,
    methods: &[&str],
) -> AsyncBridgeFnStore {
    let context = scope.get_current_context();
    let global = context.global(scope);
    let mut data = Vec::with_capacity(methods.len());

    for &method_name in methods {
        let boxed = Box::new(AsyncBridgeFnData {
            ctx,
            pending,
            method: method_name.to_string(),
        });
        // Pointer to heap allocation — stable while Box exists in data vec
        let ptr = &*boxed as *const AsyncBridgeFnData as *mut c_void;
        data.push(boxed);

        let external = v8::External::new(scope, ptr);
        let template = v8::FunctionTemplate::builder(async_bridge_callback)
            .data(external.into())
            .build(scope);
        let func = template.get_function(scope).unwrap();
        attach_bridge_function_aliases(scope, func, &["apply"]);

        let key = v8::String::new(scope, method_name).unwrap();
        global.set(scope, key.into(), func.into());
    }

    AsyncBridgeFnStore { _data: data }
}

fn attach_bridge_function_aliases<'s>(
    scope: &mut v8::HandleScope<'s>,
    func: v8::Local<'s, v8::Function>,
    aliases: &[&str],
) {
    let func_object = func.to_object(scope).unwrap();
    for alias in aliases {
        let key = v8::String::new(scope, alias).unwrap();
        let Some(wrapper) = build_bridge_apply_wrapper(scope, func) else {
            continue;
        };
        let _ = func_object.set(scope, key.into(), wrapper.into());
    }
}

fn build_bridge_apply_wrapper<'s>(
    scope: &mut v8::HandleScope<'s>,
    func: v8::Local<'s, v8::Function>,
) -> Option<v8::Local<'s, v8::Function>> {
    let source = v8::String::new(
        scope,
        "(function (fn) { return function (_thisArg, args) { return fn(...(Array.isArray(args) ? args : [])); }; })",
    )?;
    let script = v8::Script::compile(scope, source, None)?;
    let factory = script.run(scope)?;
    let factory = v8::Local::<v8::Function>::try_from(factory).ok()?;
    let argv = [func.into()];
    let receiver = v8::undefined(scope).into();
    factory
        .call(scope, receiver, &argv)
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
}

fn reject_promise_with_error(
    scope: &mut v8::HandleScope,
    resolver: v8::Local<v8::PromiseResolver>,
    message: &str,
    code: Option<&str>,
) {
    let msg = v8::String::new(scope, message).unwrap();
    let exc = v8::Exception::error(scope, msg);
    if let Some(code) = code {
        let exc_object = exc.to_object(scope).unwrap();
        let code_key = v8::String::new(scope, "code").unwrap();
        let code_value = v8::String::new(scope, code).unwrap();
        let _ = exc_object.set(scope, code_key.into(), code_value.into());
    }
    resolver.reject(scope, exc);
}

/// V8 FunctionTemplate callback for async promise-returning bridge calls.
fn async_bridge_callback(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) {
    let data = match bridge_callback_data(scope, args.data(), true) {
        Ok(data) => data,
        Err((message, code)) => {
            let exception = runtime_error_exception_with_code(scope, message, code);
            scope.throw_exception(exception);
            return;
        }
    };
    let ctx = data.context();
    let pending = data.pending();

    // Create PromiseResolver
    let resolver = match v8::PromiseResolver::new(scope) {
        Some(r) => r,
        None => {
            let msg = v8::String::new(scope, "failed to create PromiseResolver").unwrap();
            let exc = v8::Exception::error(scope, msg);
            scope.throw_exception(exc);
            return;
        }
    };

    // Get the promise to return to V8
    let promise = resolver.get_promise(scope);

    let reservation = match pending.reserve() {
        Ok(reservation) => reservation,
        Err(err_msg) => {
            reject_promise_with_error(
                scope,
                resolver,
                &err_msg,
                Some("ERR_AGENTOS_BRIDGE_PENDING_PROMISE_LIMIT"),
            );
            rv.set(promise.into());
            return;
        }
    };

    // Serialize V8 arguments using the Vec released by V8's serializer directly.
    let encoded_args = match serialize_v8_args(scope, &args) {
        Ok(encoded_args) => encoded_args,
        Err(err) => {
            let msg =
                v8::String::new(scope, &format!("bridge serialization error: {}", err)).unwrap();
            let exc = v8::Exception::error(scope, msg);
            scope.throw_exception(exc);
            return;
        }
    };

    // Register the response target and V8 resolver before the request can
    // become host-visible. A fast response therefore cannot outrun resolver
    // installation once response ingress is concurrent with the VM thread.
    let max_response_bytes = bridge_response_declaration(scope, &data.method, &args);
    match ctx.prepare_async_call_with_max_response_bytes(
        &data.method,
        encoded_args,
        max_response_bytes,
    ) {
        Ok(prepared) => {
            let call_id = prepared.call_id;
            let global_resolver = v8::Global::new(scope, resolver);
            pending.insert_reserved(call_id, global_resolver, &data.method, reservation);
            if let Err(err_msg) = ctx.dispatch_async_call(prepared) {
                pending.remove(call_id);
                reject_promise_with_error(scope, resolver, &err_msg, None);
            }
        }
        Err(err_msg) => {
            // Reject the promise immediately if send fails
            reject_promise_with_error(scope, resolver, &err_msg, None);
        }
    }

    // Return the promise
    rv.set(promise.into());
}

/// Replace stub bridge functions on a snapshot-restored context with real
/// session-local bridge functions. Overwrites the 38 stub globals with
/// functions backed by session-local BridgeCallContext.
///
/// Returns (BridgeFnStore, AsyncBridgeFnStore) that must be kept alive
/// for the lifetime of the V8 context.
pub fn replace_bridge_fns(
    scope: &mut v8::HandleScope,
    ctx: *const BridgeCallContext,
    pending: *const PendingPromises,
    sync_fns: &[&str],
    async_fns: &[&str],
) -> (BridgeFnStore, AsyncBridgeFnStore) {
    // Bridge replacement is an explicit non-retained boundary for this isolate.
    // Retained calls skip replacement and keep their registered context IDs.
    reset_vm_context_registry(scope);
    let sync_store = register_sync_bridge_fns(scope, ctx, sync_fns);
    let async_store = register_async_bridge_fns(scope, ctx, pending, async_fns);
    (sync_store, async_store)
}

/// Register stub bridge functions on the V8 global for snapshot creation.
///
/// Uses the same sync_bridge_callback / async_bridge_callback as real
/// functions (required for ExternalReferences in snapshot serialization)
/// but WITHOUT v8::External data. If a stub is accidentally called during
/// snapshot creation, the callback gracefully throws a V8 exception
/// (args.data() is not External -> "missing bridge function data" error).
///
/// After snapshot restore, these stubs are replaced with real functions
/// that have proper External data pointing to a session-local BridgeCallContext.
pub fn register_stub_bridge_fns(
    scope: &mut v8::HandleScope,
    sync_fns: &[&str],
    async_fns: &[&str],
) {
    let context = scope.get_current_context();
    let global = context.global(scope);

    // Register sync bridge functions as stubs (no External data)
    for &method_name in sync_fns {
        let template = v8::FunctionTemplate::builder(sync_bridge_callback).build(scope);
        let func = template.get_function(scope).unwrap();
        let key = v8::String::new(scope, method_name).unwrap();
        global.set(scope, key.into(), func.into());
    }

    // Register async bridge functions as stubs (no External data)
    for &method_name in async_fns {
        let template = v8::FunctionTemplate::builder(async_bridge_callback).build(scope);
        let func = template.get_function(scope).unwrap();
        let key = v8::String::new(scope, method_name).unwrap();
        global.set(scope, key.into(), func.into());
    }
}

/// Serialize V8 function arguments as an array.
fn serialize_v8_args(
    scope: &mut v8::HandleScope,
    args: &v8::FunctionCallbackArguments,
) -> Result<Vec<u8>, String> {
    let count = args.length();
    let array = v8::Array::new(scope, count);
    for i in 0..count {
        array.set_index(scope, i as u32, args.get(i));
    }
    serialize_v8_value(scope, array.into())
}

/// Resolve or reject a pending async bridge promise by call_id.
///
/// Called when a BridgeResponse arrives during the session event loop.
/// Flushes microtasks after resolution to process .then() handlers.
pub fn resolve_pending_promise(
    scope: &mut v8::HandleScope,
    pending: &PendingPromises,
    call_id: u64,
    status: u8,
    payload: Option<Vec<u8>>,
) -> Result<(), String> {
    let resolver_global = pending
        .remove(call_id)
        .ok_or_else(|| format!("no pending promise for call_id {}", call_id))?;
    let resolver = v8::Local::new(scope, &resolver_global);

    if status == 1 {
        let payload = payload.as_deref().unwrap_or_default();
        let exc = bridge_error_exception(scope, payload);
        resolver.reject(scope, exc);
    } else if let Some(payload) = payload {
        let v8_val = bridge_response_payload_to_v8(scope, status, &payload);
        if let Some(val) = v8_val {
            resolver.resolve(scope, val);
        } else {
            let msg = v8::String::new(scope, "bridge response conversion failed").unwrap();
            let exc = v8::Exception::error(scope, msg);
            resolver.reject(scope, exc);
        }
    } else {
        let undef = v8::undefined(scope);
        resolver.resolve(scope, undef.into());
    }

    // Flush microtasks after resolution
    crate::owned_microtasks::checkpoint(scope);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        activate_bridge_execution, clear_vm_context_registry_for_test,
        declared_bridge_response_bytes, decode_bridge_error, deserialize_cbor_value,
        fill_vm_context_registry_for_test, install_session_bridge_fns, register_async_bridge_fns,
        register_sync_bridge_fns, reserve_vm_context_slot, reset_vm_context_registry,
        serialize_cbor_value, vm_context_capacity_error, vm_context_registry,
        vm_context_registry_len_for_test, PendingPromises, VmContextRegistryGuard,
        MAX_CBOR_BRIDGE_CONTAINER_ITEMS, MAX_CBOR_BRIDGE_DEPTH, MAX_PENDING_PROMISES,
        MAX_VM_CONTEXTS,
    };
    use crate::host_call::BridgeCallContext;
    use crate::ipc_binary::{self, BinaryFrame};
    use crate::isolate;
    use std::io::{Cursor, Write};
    use std::process::Command;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.0.lock().unwrap().flush()
        }
    }

    fn bridge_call_count(bytes: &[u8]) -> usize {
        let mut cursor = Cursor::new(bytes);
        let mut count = 0;
        while let Ok(frame) = ipc_binary::read_frame(&mut cursor) {
            if matches!(frame, BinaryFrame::BridgeCall { .. }) {
                count += 1;
            }
        }
        count
    }

    #[test]
    fn owned_bridge_methods_fail_closed_between_operations() {
        const CHILD: &str = "AGENTOS_BRIDGE_GUARD_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "bridge::tests::owned_bridge_methods_fail_closed_between_operations",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .spawn()
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(status.success(), "bridge guard child failed: {status}");
                    return;
                }
                if std::time::Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("bridge guard child exceeded 20 seconds");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        isolate::init_v8_platform();
        let mut isolate = isolate::create_isolate(None);
        let context = isolate::create_context(&mut isolate);
        let scope = &mut v8::HandleScope::new(&mut isolate);
        let context = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, context);
        install_session_bridge_fns(
            scope,
            &["process.versions", "v8.getHeapStatistics"],
            &["_kernelPoll"],
        )
        .unwrap();
        fn run(scope: &mut v8::HandleScope, source: &str) {
            let scope = &mut v8::TryCatch::new(scope);
            let source = v8::String::new(scope, source).unwrap();
            let script = v8::Script::compile(scope, source, None).unwrap();
            assert!(script.run(scope).is_some(), "bridge guard script failed");
        }
        run(
            scope,
            r#"
            globalThis.savedLocal = globalThis['process.versions'];
            globalThis.savedHeap = globalThis['v8.getHeapStatistics'];
            globalThis.savedAsync = _kernelPoll.apply;
            globalThis.assertIdle = () => {
                for (const invoke of [() => savedLocal(), () => savedHeap(), () => savedAsync(null, [])]) {
                    let code;
                    try { invoke(); } catch (error) { code = error.code; }
                    if (code !== 'ERR_AGENTOS_BRIDGE_INACTIVE') throw Error('idle bridge remained active: ' + code);
                }
            };
            assertIdle();
        "#,
        );
        for operation in 0..2 {
            let writer = Arc::new(Mutex::new(Vec::new()));
            let ctx = Rc::new(BridgeCallContext::new(
                Box::new(SharedWriter(Arc::clone(&writer))),
                Box::new(Cursor::new(Vec::new())),
                format!("operation-{operation}"),
            ));
            let pending = Rc::new(PendingPromises::new());
            let pending_weak = Rc::downgrade(&pending);
            let context_weak = Rc::downgrade(&ctx);
            let guard = activate_bridge_execution(scope, Rc::clone(&ctx), Rc::clone(&pending));
            run(
                scope,
                r#"
                if (!savedLocal().v8) throw Error('active local bridge failed');
                if (!(savedHeap().used_heap_size > 0)) throw Error('active local heap bridge failed');
                savedAsync(null, ['current-operation']);
            "#,
            );
            assert_eq!(
                pending.len(),
                1,
                "async resolver belongs to current operation"
            );
            let frame =
                ipc_binary::read_frame(&mut Cursor::new(writer.lock().unwrap().clone())).unwrap();
            match frame {
                BinaryFrame::BridgeCall {
                    session_id, method, ..
                } => {
                    assert_eq!(session_id, format!("operation-{operation}"));
                    assert_eq!(method, "_kernelPoll");
                }
                frame => panic!("expected current operation bridge call, got {frame:?}"),
            }
            drop(ctx);
            drop(pending);
            drop(guard);
            assert!(
                context_weak.upgrade().is_none(),
                "idle slot retained old context"
            );
            assert!(
                pending_weak.upgrade().is_none(),
                "idle slot retained unresolved resolvers"
            );
            run(scope, "assertIdle()");
        }
        run(
            scope,
            r#"
            Object.defineProperty(globalThis, '_blockedBridge', {
                set() { throw Error('installation rejected'); }, configurable: false
            });
        "#,
        );
        assert!(install_session_bridge_fns(scope, &["_blockedBridge"], &[]).is_err());
        // The failed setter was caught inside installation; no exception or
        // operation route remains to contaminate subsequent guest execution.
        run(scope, "assertIdle()");
    }

    #[test]
    fn bridge_response_declarations_use_contract_reads_and_bounded_default() {
        assert_eq!(declared_bridge_response_bytes("_log", None), 4096);
        assert_eq!(
            declared_bridge_response_bytes("_loadFileSync", None),
            16 * 1024 * 1024
        );
        assert_eq!(
            declared_bridge_response_bytes("_fsReadRaw", Some(32 * 1024)),
            47_788
        );
        assert_eq!(
            declared_bridge_response_bytes("fs.readSync", Some(16 * 1024)),
            25_944
        );
        assert_eq!(
            declared_bridge_response_bytes("_unclassifiedBridge", None),
            256 * 1024
        );
    }

    #[test]
    fn bridge_error_decoder_rejects_unstructured_diagnostics() {
        assert!(decode_bridge_error(b"user said 'EACCES: denied'").is_none());
        assert!(decode_bridge_error(b"ERR_AGENTOS_FAKE: EACCES: denied").is_none());
    }

    #[test]
    fn bridge_error_decoder_preserves_structured_fields() {
        let mut payload = Vec::new();
        ciborium::into_writer(
            &ciborium::Value::Map(vec![
                (
                    ciborium::Value::Text(String::from("code")),
                    ciborium::Value::Text(String::from("EACCES")),
                ),
                (
                    ciborium::Value::Text(String::from("message")),
                    ciborium::Value::Text(String::from("permission denied")),
                ),
                (
                    ciborium::Value::Text(String::from("details")),
                    ciborium::Value::Map(vec![(
                        ciborium::Value::Text(String::from("limitName")),
                        ciborium::Value::Text(String::from("maxBytes")),
                    )]),
                ),
            ]),
            &mut payload,
        )
        .expect("encode structured error");
        let decoded = decode_bridge_error(&payload).expect("decode structured error");
        assert_eq!(decoded.code, "EACCES");
        assert_eq!(decoded.message, "permission denied");
        assert!(decoded.details.is_some());
    }

    fn heap_statistics_fresh_and_restored_controls() {
        for restored in [false, true] {
            let mut isolate = if restored {
                let blob =
                    crate::snapshot::create_snapshot("globalThis.__heapSnapshotRoot = true;")
                        .unwrap();
                crate::snapshot::create_isolate_from_snapshot(blob, None)
            } else {
                isolate::create_isolate(None)
            };
            let root = isolate::create_context(&mut isolate);
            let scope = &mut v8::HandleScope::new(&mut isolate);
            let context = v8::Local::new(scope, &root);
            let scope = &mut v8::ContextScope::new(scope, context);
            let writer = Arc::new(Mutex::new(Vec::new()));
            let bridge_ctx = BridgeCallContext::new(
                Box::new(SharedWriter(Arc::clone(&writer))),
                Box::new(Cursor::new(Vec::new())),
                "heap-statistics-local-control".into(),
            );
            let _functions =
                register_sync_bridge_fns(scope, &bridge_ctx, &["v8.getHeapStatistics"]);
            // Owning-test collection only; production queries and memory workers do not collect.
            scope.low_memory_notification();
            let mut native = v8::HeapStatistics::default();
            scope.get_heap_statistics(&mut native);
            let source = v8::String::new(
                scope,
                r#"
                (() => {
                    const read = globalThis['v8.getHeapStatistics'];
                    const before = read();
                    const retained = [];
                    for (let index = 0; index < 25000; index += 1) {
                        retained.push({ index, text: `${index}-`.padEnd(256, String(index % 10)) });
                    }
                    globalThis.__heapStatisticsRetained = retained;
                    const after = read();
                    return JSON.stringify({ before, after, retainedCount: retained.length });
                })()
            "#,
            )
            .unwrap();
            let result = {
                let tc = &mut v8::TryCatch::new(scope);
                let script =
                    v8::Script::compile(tc, source, None).expect("compile heap-statistics control");
                let result = script.run(tc).expect("run local heap-statistics control");
                serde_json::from_str::<serde_json::Value>(&result.to_rust_string_lossy(tc))
                    .expect("parse heap-statistics control")
            };
            for sample in ["before", "after"] {
                let values = result[sample].as_object().expect("statistics object");
                assert_eq!(values.len(), 14, "{restored}/{sample}: {result}");
                for value in values.values() {
                    assert!(
                        value.as_u64().is_some(),
                        "statistics fields must be nonnegative integers: {result}"
                    );
                }
                assert_eq!(
                    values["heap_size_limit"].as_u64().unwrap(),
                    native.heap_size_limit() as u64
                );
                assert_eq!(
                    values["does_zap_garbage"].as_u64().unwrap(),
                    native.does_zap_garbage() as u64
                );
                assert_eq!(
                    values["number_of_native_contexts"].as_u64().unwrap(),
                    native.number_of_native_contexts() as u64
                );
                assert!(
                    values["used_heap_size"].as_u64().unwrap()
                        <= values["total_heap_size"].as_u64().unwrap()
                );
                assert!(
                    values["used_global_handles_size"].as_u64().unwrap()
                        <= values["total_global_handles_size"].as_u64().unwrap()
                );
            }
            assert_eq!(result["retainedCount"], 25000);
            assert!(
                result["after"]["used_heap_size"].as_u64().unwrap()
                    > result["before"]["used_heap_size"].as_u64().unwrap() + 512_000,
                "rooted allocation must change live heap use: {result}"
            );
            assert!(
                writer.lock().unwrap().is_empty(),
                "heap statistics must not emit a host RPC"
            );
        }
    }

    fn child_globals_fresh_and_restored_controls() {
        for restored in [false, true] {
            let mut isolate = if restored {
                let blob = crate::snapshot::create_snapshot(
                    r#"
                    globalThis.Buffer = { marker: true };
                    globalThis.require = function () { return 99; };
                    globalThis.process = { marker: true };
                    globalThis.module = { marker: true };
                    globalThis.exports = { marker: true };
                    globalThis.__dirname = '/snapshot-root';
                    globalThis.__filename = '/snapshot-root/entry.js';
                    globalThis.__snapshotHostGlobals = true;
                "#,
                )
                .unwrap();
                crate::snapshot::create_isolate_from_snapshot(blob, None)
            } else {
                isolate::create_isolate(None)
            };
            {
                let root = isolate::create_context(&mut isolate);
                let scope = &mut v8::HandleScope::new(&mut isolate);
                let context = v8::Local::new(scope, &root);
                let scope = &mut v8::ContextScope::new(scope, context);
                let bridge_ctx = BridgeCallContext::new(
                    Box::new(Vec::new()),
                    Box::new(Cursor::new(Vec::new())),
                    "child-global-control".into(),
                );
                let _functions = register_sync_bridge_fns(
                    scope,
                    &bridge_ctx,
                    &["_vmCreateContext", "_vmRunInContext"],
                );
                let source = v8::String::new(scope, r#"
                    (() => {
                        const names = ['Buffer','require','process','module','exports','__dirname','__filename'];
                        const rootPresent = names.every(name => typeof globalThis[name] !== 'undefined');
                        const empty = {};
                        const id = _vmCreateContext(empty);
                        const child = _vmRunInContext(id, 'globalThis', {}, empty);
                        const childAbsent = names.every(name => child[name] === undefined);
                        const ownOverride = { process: { marker: 'explicit' } };
                        const overrideId = _vmCreateContext(ownOverride);
                        const overrideValue = _vmRunInContext(overrideId, 'process.marker', {}, ownOverride);
                        const objectFailure = { marker: 'OBJECT_GETTER_MARKER' };
                        const errorFailure = new Error('ERROR_GETTER_MESSAGE');
                        errorFailure.marker = 'ERROR_GETTER_MARKER';
                        const failures = {};
                        Object.defineProperty(failures, 'objectFail', { get() { throw objectFailure; } });
                        Object.defineProperty(failures, 'errorFail', { get() { throw errorFailure; } });
                        const failureId = _vmCreateContext(failures);
                        const escaped = _vmRunInContext(failureId, 'globalThis', {}, failures);
                        const errorChecks = [];
                        for (const [read, expected] of [
                            [() => escaped.objectFail, objectFailure],
                            [() => escaped.errorFail, errorFailure],
                            [() => _vmRunInContext(failureId, 'objectFail', {}, failures), objectFailure],
                            [() => _vmRunInContext(failureId, 'errorFail', {}, failures), errorFailure],
                        ]) {
                            let caught = false;
                            try { read(); } catch (error) {
                                caught = error === expected && error.marker === expected.marker;
                            }
                            errorChecks.push(caught);
                        }
                        const origins = [
                            {name:'positive', lineOffset:3, columnOffset:0, line:6, column:7, source:'\n\nthrow new Error("ORIGIN_MARKER")'},
                            {name:'zero', lineOffset:0, columnOffset:0, line:3, column:7, source:'\n\nthrow new Error("ORIGIN_MARKER")'},
                            {name:'negative', lineOffset:-1, columnOffset:0, line:2, column:7, source:'\n\nthrow new Error("ORIGIN_MARKER")'},
                            {name:'column-zero', lineOffset:0, columnOffset:0, line:1, column:7, source:'throw new Error("ORIGIN_MARKER")'},
                            {name:'column-two', lineOffset:0, columnOffset:2, line:1, column:9, source:'throw new Error("ORIGIN_MARKER")'},
                        ];
                        const originChecks = [];
                        for (const row of origins) {
                            let correct = false;
                            try {
                                _vmRunInContext(failureId, row.source,
                                    {filename:'vm-origin-'+row.name+'.js', lineOffset:row.lineOffset,
                                     columnOffset:row.columnOffset}, failures);
                            } catch (error) {
                                const frame = String(error.stack).match(/vm-origin-[a-z-]+\.js:(-?\d+):(-?\d+)/);
                                correct = error.name === 'Error' && error.message === 'ORIGIN_MARKER' &&
                                    frame !== null && +frame[1] === row.line && +frame[2] === row.column;
                            }
                            originChecks.push(correct);
                        }
                        const errorRecovery = _vmRunInContext(failureId, '6*7', {}, failures) === 42;
                        return JSON.stringify({rootPresent, childAbsent, overrideValue,
                            rootMarker: globalThis.__snapshotHostGlobals === true,
                            errorChecks, originChecks, errorRecovery});
                    })()
                "#).unwrap();
                let script = v8::Script::compile(scope, source, None).unwrap();
                let result = script
                    .run(scope)
                    .expect("public child initialization control");
                let result = result.to_rust_string_lossy(scope);
                let expected = if restored {
                    r#"{"rootPresent":true,"childAbsent":true,"overrideValue":"explicit","rootMarker":true,"errorChecks":[true,true,true,true],"originChecks":[true,true,true,true,true],"errorRecovery":true}"#
                } else {
                    r#"{"rootPresent":false,"childAbsent":true,"overrideValue":"explicit","rootMarker":false,"errorChecks":[true,true,true,true],"originChecks":[true,true,true,true,true],"errorRecovery":true}"#
                };
                assert_eq!(
                    result, expected,
                    "fresh/restored child scrub and explicit sandbox overrides"
                );
                clear_vm_context_registry_for_test(scope);
            }
            isolate::drop_isolate(Some(isolate));
        }
    }

    #[test]
    fn bridge_v8_hardening_rejects_cbor_abuse_and_vm_context_reentry_overflow() {
        const SUBPROCESS_ENV: &str = "AGENTOS_V8_BRIDGE_HARDENING_SUBPROCESS";
        if std::env::var_os(SUBPROCESS_ENV).is_none() {
            let output = Command::new(std::env::current_exe().expect("current test binary"))
                .arg("bridge::tests::bridge_v8_hardening_rejects_cbor_abuse_and_vm_context_reentry_overflow")
                .arg("--exact")
                .arg("--nocapture")
                .env(SUBPROCESS_ENV, "1")
                .output()
                .expect("spawn bridge hardening subprocess");
            assert!(
                output.status.success(),
                "bridge hardening subprocess failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        isolate::init_v8_platform();
        child_globals_fresh_and_restored_controls();
        heap_statistics_fresh_and_restored_controls();

        let mut isolate = isolate::create_isolate(None);
        let context = isolate::create_context(&mut isolate);
        let scope = &mut v8::HandleScope::new(&mut isolate);
        let context = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, context);

        let object = v8::Object::new(scope);
        let self_key = v8::String::new(scope, "self").unwrap();
        assert!(object.set(scope, self_key.into(), object.into()).is_some());

        let error = serialize_cbor_value(scope, object.into()).expect_err("cycle rejected");
        assert!(
            error.contains("circular object graph"),
            "unexpected error: {error}"
        );

        let source = v8::String::new(
            scope,
            &format!(
                "const sparse = []; sparse.length = {}; sparse",
                MAX_CBOR_BRIDGE_CONTAINER_ITEMS + 1
            ),
        )
        .unwrap();
        let script = v8::Script::compile(scope, source, None).unwrap();
        let sparse = script.run(scope).unwrap();
        let error = serialize_cbor_value(scope, sparse).expect_err("sparse array rejected");
        assert!(
            error.contains(&format!(
                "item count {} exceeds limit",
                MAX_CBOR_BRIDGE_CONTAINER_ITEMS + 1
            )),
            "unexpected error: {error}"
        );

        let mut value = ciborium::Value::Null;
        for _ in 0..=MAX_CBOR_BRIDGE_DEPTH {
            value = ciborium::Value::Array(vec![value]);
        }
        let mut encoded = Vec::new();
        ciborium::into_writer(&value, &mut encoded).unwrap();
        let error = deserialize_cbor_value(scope, &encoded).expect_err("depth rejected");
        assert!(
            error.contains("CBOR decode failed"),
            "unexpected error: {error}"
        );

        let oversized_len = (MAX_CBOR_BRIDGE_CONTAINER_ITEMS + 1) as u32;
        let oversized_array_header = [
            0x9a,
            (oversized_len >> 24) as u8,
            (oversized_len >> 16) as u8,
            (oversized_len >> 8) as u8,
            oversized_len as u8,
        ];
        let error = deserialize_cbor_value(scope, &oversized_array_header)
            .expect_err("oversized array rejected before element allocation");
        assert!(
            error.contains(&format!(
                "item count {} exceeds limit",
                MAX_CBOR_BRIDGE_CONTAINER_ITEMS + 1
            )),
            "unexpected error: {error}"
        );

        fill_vm_context_registry_for_test(scope, context, MAX_VM_CONTEXTS - 1);
        let bridge_ctx = BridgeCallContext::new(
            Box::new(Vec::new()),
            Box::new(Cursor::new(Vec::new())),
            String::from("test-session"),
        );
        let _bridge_fns = register_sync_bridge_fns(
            scope,
            &bridge_ctx as *const BridgeCallContext,
            &["_vmCreateContext", "_vmRunInContext"],
        );

        let source = r#"
            let innerCode;
            const sandbox = {};
            Object.defineProperty(sandbox, "x", {
                get() {
                    try {
                        _vmCreateContext({});
                    } catch (error) {
                        innerCode = error && error.code;
                    }
                    return 1;
                },
                enumerable: true,
            });

            const outerId = _vmCreateContext(sandbox);
            if (innerCode !== undefined) throw Error("EAGER_CONTEXT_GETTER");
            _vmRunInContext(outerId, "x", {}, sandbox);
            let limitCode;
            try {
                _vmCreateContext({});
            } catch (error) {
                limitCode = error && error.code;
            }

            JSON.stringify({
                innerCode,
                limitCode,
                outerIsInteger: Number.isInteger(outerId),
            })
        "#;
        {
            let tc = &mut v8::TryCatch::new(scope);
            let source = v8::String::new(tc, source).unwrap();
            let script = v8::Script::compile(tc, source, None).unwrap();
            let result = script.run(tc);
            assert!(
                !tc.has_caught(),
                "unexpected exception while testing vm cap"
            );
            let details = result
                .expect("vm context cap script result")
                .to_rust_string_lossy(tc);
            assert_eq!(
                details,
                r#"{"innerCode":"ERR_AGENTOS_VM_CONTEXT_LIMIT","limitCode":"ERR_AGENTOS_VM_CONTEXT_LIMIT","outerIsInteger":true}"#,
                "vm context cap script should observe limit errors"
            );
        }
        assert_eq!(vm_context_registry_len_for_test(scope), MAX_VM_CONTEXTS);
        clear_vm_context_registry_for_test(scope);

        let source = r#"
            (() => {
                let thrownMessage;
                const sandbox = {};
                Object.defineProperty(sandbox, "x", {
                    get() {
                        throw new Error("sandbox getter failed");
                    },
                    enumerable: true,
                });
                const contextId = _vmCreateContext(sandbox);
                if (thrownMessage !== undefined) throw Error("EAGER_CONTEXT_GETTER");
                try {
                    _vmRunInContext(contextId, "x", {}, sandbox);
                } catch (error) {
                    thrownMessage = error && error.message;
                }
                const retainedValue = _vmRunInContext(contextId, "6 * 7", {}, sandbox);
                const nextId = _vmCreateContext({});
                return JSON.stringify({
                    thrownMessage,
                    retainedValue,
                    nextIsInteger: Number.isInteger(nextId),
                });
            })()
        "#;
        {
            let tc = &mut v8::TryCatch::new(scope);
            let source = v8::String::new(tc, source).unwrap();
            let script = v8::Script::compile(tc, source, None).unwrap();
            let result = script.run(tc);
            if tc.has_caught() {
                let exception = tc
                    .exception()
                    .map(|exception| exception.to_rust_string_lossy(tc))
                    .unwrap_or_else(|| String::from("<missing exception>"));
                panic!("unexpected exception while testing vm rollback: {exception}");
            }
            let details = result
                .expect("vm context rollback script result")
                .to_rust_string_lossy(tc);
            assert_eq!(
                details, r#"{"thrownMessage":"sandbox getter failed","retainedValue":42,"nextIsInteger":true}"#,
                "child access preserves the getter exception and leaves both context and registry usable"
            );
        }
        assert_eq!(vm_context_registry_len_for_test(scope), 2);
        let parent_token = context.get_security_token(scope);
        {
            let registry = vm_context_registry(scope);
            for state in registry.0.borrow().contexts.values() {
                let child = v8::Local::new(scope, &state.context);
                let token = child.get_security_token(scope);
                assert!(
                    token.strict_equals(parent_token),
                    "public node:vm child inherits only its parent token"
                );
            }
        }
        let independent = v8::Context::new(scope, Default::default());
        let independent_token = independent.get_security_token(scope);
        assert!(
            !independent_token.strict_equals(parent_token),
            "independent root token is unchanged"
        );
        clear_vm_context_registry_for_test(scope);

        // The handler/template data must not root a sandbox once the bounded
        // registry and ordinary handles release its only child context.
        let weak_sandbox = {
            let inner = &mut v8::HandleScope::new(scope);
            let sandbox = v8::Object::new(inner);
            let global = context.global(inner);
            let name = v8::String::new(inner, "_vmCreateContext").unwrap();
            let create = global.get(inner, name.into()).unwrap();
            let create = v8::Local::<v8::Function>::try_from(create).unwrap();
            assert!(create
                .call(inner, global.into(), &[sandbox.into()])
                .is_some());
            v8::Weak::new(inner, sandbox)
        };
        assert!(
            !weak_sandbox.is_empty(),
            "live registry owns context and sandbox"
        );
        clear_vm_context_registry_for_test(scope);
        crate::execution::collect_without_microtasks(scope);
        assert!(
            weak_sandbox.is_empty(),
            "unreachable context/template data releases the sandbox after collection"
        );

        let async_writer = Arc::new(Mutex::new(Vec::new()));
        let async_bridge_ctx = BridgeCallContext::new(
            Box::new(SharedWriter(Arc::clone(&async_writer))),
            Box::new(Cursor::new(Vec::new())),
            String::from("test-session"),
        );
        let async_pending = PendingPromises::new();
        let _async_bridge_fns = register_async_bridge_fns(
            scope,
            &async_bridge_ctx as *const BridgeCallContext,
            &async_pending as *const PendingPromises,
            &["_asyncFn"],
        );
        let source = format!(
            r#"
            for (let i = 0; i < {fill_count}; i++) {{
                _asyncFn(i);
            }}
            globalThis.__overflowPromise = _asyncFn("overflow");
            "#,
            fill_count = MAX_PENDING_PROMISES,
        );
        {
            let tc = &mut v8::TryCatch::new(scope);
            let source = v8::String::new(tc, &source).unwrap();
            let script = v8::Script::compile(tc, source, None).unwrap();
            assert!(script.run(tc).is_some());
            assert!(!tc.has_caught(), "async overflow should reject, not throw");
        }
        assert_eq!(async_pending.len(), MAX_PENDING_PROMISES);
        assert_eq!(
            bridge_call_count(&async_writer.lock().unwrap()),
            MAX_PENDING_PROMISES
        );
        {
            let key = v8::String::new(scope, "__overflowPromise").unwrap();
            let value = context.global(scope).get(scope, key.into()).unwrap();
            let promise = v8::Local::<v8::Promise>::try_from(value).unwrap();
            assert_eq!(promise.state(), v8::PromiseState::Rejected);
            let rejection = promise.result(scope);
            let rejection = v8::Local::<v8::Object>::try_from(rejection).unwrap();
            let code_key = v8::String::new(scope, "code").unwrap();
            let code = rejection.get(scope, code_key.into()).unwrap();
            assert_eq!(
                code.to_rust_string_lossy(scope),
                "ERR_AGENTOS_BRIDGE_PENDING_PROMISE_LIMIT"
            );
        }

        let reentrant_writer = Arc::new(Mutex::new(Vec::new()));
        let reentrant_bridge_ctx = BridgeCallContext::new(
            Box::new(SharedWriter(Arc::clone(&reentrant_writer))),
            Box::new(Cursor::new(Vec::new())),
            String::from("test-session"),
        );
        let reentrant_pending = PendingPromises::new();
        let _reentrant_async_bridge_fns = register_async_bridge_fns(
            scope,
            &reentrant_bridge_ctx as *const BridgeCallContext,
            &reentrant_pending as *const PendingPromises,
            &["_asyncFn"],
        );
        let source = format!(
            r#"
            for (let i = 0; i < {fill_count}; i++) {{
                _asyncFn(i);
            }}
            let innerPromise;
            const reentrantArg = {{}};
            Object.defineProperty(reentrantArg, "x", {{
                get() {{
                    innerPromise = _asyncFn("inner");
                    return 1;
                }},
                enumerable: true,
            }});
            globalThis.__reentrantOuterPromise = _asyncFn(reentrantArg);
            globalThis.__reentrantInnerPromise = innerPromise;
            "#,
            fill_count = MAX_PENDING_PROMISES - 1,
        );
        {
            let tc = &mut v8::TryCatch::new(scope);
            let source = v8::String::new(tc, &source).unwrap();
            let script = v8::Script::compile(tc, source, None).unwrap();
            assert!(script.run(tc).is_some());
            assert!(!tc.has_caught(), "async reentry should reject, not throw");
        }
        assert_eq!(reentrant_pending.len(), MAX_PENDING_PROMISES);
        assert_eq!(
            bridge_call_count(&reentrant_writer.lock().unwrap()),
            MAX_PENDING_PROMISES
        );
        {
            let key = v8::String::new(scope, "__reentrantInnerPromise").unwrap();
            let value = context.global(scope).get(scope, key.into()).unwrap();
            let promise = v8::Local::<v8::Promise>::try_from(value).unwrap();
            assert_eq!(promise.state(), v8::PromiseState::Rejected);
            let rejection = promise.result(scope);
            let rejection = v8::Local::<v8::Object>::try_from(rejection).unwrap();
            let code_key = v8::String::new(scope, "code").unwrap();
            let code = rejection.get(scope, code_key.into()).unwrap();
            assert_eq!(
                code.to_rust_string_lossy(scope),
                "ERR_AGENTOS_BRIDGE_PENDING_PROMISE_LIMIT"
            );
        }

        let buffer_reentry_writer = Arc::new(Mutex::new(Vec::new()));
        let buffer_reentry_bridge_ctx = BridgeCallContext::new(
            Box::new(SharedWriter(Arc::clone(&buffer_reentry_writer))),
            Box::new(Cursor::new(Vec::new())),
            String::from("test-session"),
        );
        let buffer_reentry_pending = PendingPromises::new();
        let _buffer_reentry_async_bridge_fns = register_async_bridge_fns(
            scope,
            &buffer_reentry_bridge_ctx as *const BridgeCallContext,
            &buffer_reentry_pending as *const PendingPromises,
            &["_asyncFn"],
        );
        let source = r#"
            let bufferInnerPromise;
            const bufferReentrantArg = {};
            Object.defineProperty(bufferReentrantArg, "x", {
                get() {
                    bufferInnerPromise = _asyncFn("inner");
                    return 1;
                },
                enumerable: true,
            });
            globalThis.__bufferOuterPromise = _asyncFn(bufferReentrantArg);
            globalThis.__bufferInnerPromise = bufferInnerPromise;
        "#;
        {
            let tc = &mut v8::TryCatch::new(scope);
            let source = v8::String::new(tc, source).unwrap();
            let script = v8::Script::compile(tc, source, None).unwrap();
            assert!(script.run(tc).is_some());
            assert!(
                !tc.has_caught(),
                "async serialization reentry should not panic or throw"
            );
        }
        assert_eq!(buffer_reentry_pending.len(), 2);
        assert_eq!(bridge_call_count(&buffer_reentry_writer.lock().unwrap()), 2);
    }

    #[test]
    fn vm_context_capacity_error_trips_at_registry_limit() {
        assert!(vm_context_capacity_error(MAX_VM_CONTEXTS - 1).is_none());

        let error = vm_context_capacity_error(MAX_VM_CONTEXTS).expect("limit error");
        assert!(
            error.contains(&format!("limit of {MAX_VM_CONTEXTS} contexts")),
            "unexpected error: {error}"
        );
    }

    // Exercise the optional explicit-boundary guard across repeated reservations.
    // Production isolate retirement is covered separately through SessionIsolate
    // in retained_family_controls; retained calls deliberately keep their IDs.
    // Runs in a subprocess to isolate V8 initialization from other libtest cases.
    #[test]
    fn vm_context_registry_evicts_slots_on_finalize() {
        const SUBPROCESS_ENV: &str = "AGENTOS_V8_VM_CONTEXT_FINALIZE_SUBPROCESS";
        if std::env::var_os(SUBPROCESS_ENV).is_none() {
            let output = Command::new(std::env::current_exe().expect("current test binary"))
                .arg("bridge::tests::vm_context_registry_evicts_slots_on_finalize")
                .arg("--exact")
                .arg("--nocapture")
                .env(SUBPROCESS_ENV, "1")
                .output()
                .expect("spawn vm context finalize subprocess");
            assert!(
                output.status.success(),
                "vm context finalize subprocess failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        isolate::init_v8_platform();
        let mut isolate = isolate::create_isolate(None);
        let context = isolate::create_context(&mut isolate);
        let scope = &mut v8::HandleScope::new(&mut isolate);
        let context = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, context);

        reset_vm_context_registry(scope);
        assert_eq!(
            vm_context_registry_len_for_test(scope),
            0,
            "registry starts empty"
        );

        // Simulate many executions on the same reused isolate. Each execution
        // reserves several contexts and then drops its explicit boundary guard.
        // Run far past the hard cap: a leaked slot per createContext would exhaust
        // MAX_VM_CONTEXTS within the first ~341 executions and the `.expect` on the
        // next reservation would panic.
        const CONTEXTS_PER_EXECUTION: usize = 3;
        let executions = MAX_VM_CONTEXTS * 4;
        for execution in 0..executions {
            {
                let _guard = VmContextRegistryGuard::new(scope);
                for _ in 0..CONTEXTS_PER_EXECUTION {
                    reserve_vm_context_slot(scope, context)
                        .expect("reserve vm context slot below cap; a leak would hit the cap");
                }
                assert_eq!(
                    vm_context_registry_len_for_test(scope),
                    CONTEXTS_PER_EXECUTION,
                    "slots reserved during execution {execution} must be live"
                );
            }
            // Guard dropped above -> finalize. Asserting here, before the next
            // execution's guard is constructed, proves the Drop sweep (not the
            // start-of-session sweep) reclaimed the slots.
            assert_eq!(
                vm_context_registry_len_for_test(scope),
                0,
                "registry must be empty after execution {execution} finalizes"
            );
        }

        // After 4x the cap worth of reservations, a fresh createContext still
        // succeeds because nothing leaked across executions.
        let _guard = VmContextRegistryGuard::new(scope);
        assert!(
            reserve_vm_context_slot(scope, context).is_ok(),
            "createContext must keep succeeding; leaked slots would have hit MAX_VM_CONTEXTS"
        );
    }
}
