//! Node's default V8 wire format, evaluated entirely on the owning isolate.
use std::cell::Cell;
use v8::{ValueDeserializerHelper, ValueSerializerHelper};

struct SerializerDelegate {
    remaining: Cell<usize>,
    limit: usize,
    path: &'static str,
}

pub(crate) fn throw_limit(scope: &mut v8::HandleScope, limit: usize, requested: usize, path: &'static str) {
    let message = v8::String::new(
        scope,
        &format!("V8 serialization requested {requested} bytes, exceeding {limit}; raise {path}"),
    )
    .unwrap();
    let error = v8::Exception::range_error(scope, message);
    let object = error.to_object(scope).unwrap();
    for (name, text) in [
        (
            "code",
            if path == "limits.reactor.maxBridgeResponseBytes" {
                "ERR_AGENTOS_BRIDGE_RESPONSE_LIMIT"
            } else {
                "ERR_AGENTOS_V8_SERIALIZATION_LIMIT"
            },
        ),
        ("configurationPath", path),
    ] {
        let key = v8::String::new(scope, name).unwrap();
        let value = v8::String::new(scope, text).unwrap();
        object.set(scope, key.into(), value.into());
    }
    scope.throw_exception(error);
}

impl v8::ValueSerializerImpl for SerializerDelegate {
    fn throw_data_clone_error<'s>(
        &self,
        scope: &mut v8::HandleScope<'s>,
        message: v8::Local<'s, v8::String>,
    ) {
        let error = v8::Exception::error(scope, message);
        scope.throw_exception(error);
    }

    fn write_host_object<'s>(
        &self,
        scope: &mut v8::HandleScope<'s>,
        object: v8::Local<'s, v8::Object>,
        serializer: &dyn ValueSerializerHelper,
    ) -> Option<bool> {
        let view = v8::Local::<v8::ArrayBufferView>::try_from(object).ok()?;
        let length = view.byte_length();
        if length > self.remaining.get() {
            throw_limit(
                scope,
                self.limit,
                (self.limit - self.remaining.get()).saturating_add(length),
                self.path,
            );
            return None;
        }
        self.remaining.set(self.remaining.get() - length);
        // Node lib/v8.js uses these stable type indexes and serializes only the
        // visible view. Serializing a pooled Buffer's entire backing store leaks
        // unrelated bytes and must never be used as a shortcut.
        let constructor_key = v8::String::new(scope, "constructor")?;
        let constructor = object.get(scope, constructor_key.into())?;
        let buffer_key = v8::String::new(scope, "Buffer")?;
        let global = scope.get_current_context().global(scope);
        let buffer = global.get(scope, buffer_key.into())?;
        let kind = if object.is_uint8_array() && constructor.strict_equals(buffer) {
            10
        } else if object.is_int8_array() {
            0
        } else if object.is_uint8_array() {
            1
        } else if object.is_uint8_clamped_array() {
            2
        } else if object.is_int16_array() {
            3
        } else if object.is_uint16_array() {
            4
        } else if object.is_int32_array() {
            5
        } else if object.is_uint32_array() {
            6
        } else if object.is_float32_array() {
            7
        } else if object.is_float64_array() {
            8
        } else if object.is_data_view() {
            9
        } else if object.is_big_int64_array() {
            11
        } else if object.is_big_uint64_array() {
            12
        } else {
            return None;
        };
        serializer.write_uint32(kind);
        serializer.write_uint32(u32::try_from(length).ok()?);
        let mut bytes = vec![0; length];
        view.copy_contents(&mut bytes);
        serializer.write_raw_bytes(&bytes);
        Some(true)
    }
}

struct DeserializerDelegate;
impl v8::ValueDeserializerImpl for DeserializerDelegate {
    fn read_host_object<'s>(
        &self,
        scope: &mut v8::HandleScope<'s>,
        deserializer: &dyn ValueDeserializerHelper,
    ) -> Option<v8::Local<'s, v8::Object>> {
        let (mut kind, mut length) = (0, 0);
        if !deserializer.read_uint32(&mut kind) || !deserializer.read_uint32(&mut length) {
            return None;
        }
        let bytes = deserializer.read_raw_bytes(length as usize)?;
        let backing = v8::ArrayBuffer::new_backing_store_from_vec(bytes.to_vec()).make_shared();
        let buffer = v8::ArrayBuffer::with_backing_store(scope, &backing);
        let length = length as usize;
        macro_rules! view {
            ($name:ident, $size:expr) => {{
                if length % $size != 0 {
                    return None;
                }
                v8::$name::new(scope, buffer, 0, length / $size).map(Into::into)
            }};
        }
        match kind {
            0 => view!(Int8Array, 1),
            1 => view!(Uint8Array, 1),
            2 => view!(Uint8ClampedArray, 1),
            3 => view!(Int16Array, 2),
            4 => view!(Uint16Array, 2),
            5 => view!(Int32Array, 4),
            6 => view!(Uint32Array, 4),
            7 => view!(Float32Array, 4),
            8 => view!(Float64Array, 8),
            9 => Some(v8::DataView::new(scope, buffer, 0, length).into()),
            10 => {
                let view: v8::Local<v8::Object> =
                    v8::Uint8Array::new(scope, buffer, 0, length)?.into();
                let key = v8::String::new(scope, "Buffer")?;
                let global = scope.get_current_context().global(scope);
                let constructor = global.get(scope, key.into())?.to_object(scope)?;
                let key = v8::String::new(scope, "prototype")?;
                let prototype = constructor.get(scope, key.into())?;
                view.set_prototype(scope, prototype)?;
                Some(view)
            }
            11 => view!(BigInt64Array, 8),
            12 => view!(BigUint64Array, 8),
            _ => None,
        }
    }
}

pub(crate) fn serialize<'s>(
    scope: &mut v8::HandleScope<'s>,
    value: v8::Local<'s, v8::Value>,
    limit: usize,
    path: &'static str,
) -> Result<v8::Local<'s, v8::Value>, String> {
    let context = scope.get_current_context();
    let serializer = v8::ValueSerializer::new(
        scope,
        Box::new(SerializerDelegate {
            remaining: Cell::new(limit),
            limit,
            path,
        }),
    );
    serializer.set_treat_array_buffer_views_as_host_objects(true);
    serializer.write_header();
    serializer
        .write_value(context, value)
        .filter(|ok| *ok)
        .ok_or("Unable to serialize value")?;
    let bytes = serializer.release();
    if bytes.len() > limit {
        throw_limit(scope, limit, bytes.len(), path);
        return Err(format!("Serialized value exceeds {path}"));
    }
    if bytes.len() >= limit.saturating_mul(4) / 5 {
        tracing::warn!(
            bytes = bytes.len(),
            limit,
            configuration_path = path,
            "V8 serialization approaches its configured byte limit"
        );
    }
    let length = bytes.len();
    let backing = v8::ArrayBuffer::new_backing_store_from_vec(bytes).make_shared();
    let buffer = v8::ArrayBuffer::with_backing_store(scope, &backing);
    v8::Uint8Array::new(scope, buffer, 0, length)
        .map(Into::into)
        .ok_or_else(|| "Unable to allocate serialized value".into())
}

pub(crate) fn deserialize<'s>(
    scope: &mut v8::HandleScope<'s>,
    value: v8::Local<'s, v8::Value>,
    limit: usize,
    path: &'static str,
) -> Result<v8::Local<'s, v8::Value>, String> {
    let view = v8::Local::<v8::ArrayBufferView>::try_from(value)
        .map_err(|_| "buffer must be a TypedArray or DataView")?;
    if view.byte_length() > limit {
        throw_limit(scope, limit, view.byte_length(), path);
        return Err(format!("Serialized input exceeds {path}"));
    }
    let mut bytes = vec![0; view.byte_length()];
    view.copy_contents(&mut bytes);
    let context = scope.get_current_context();
    let deserializer = v8::ValueDeserializer::new(scope, Box::new(DeserializerDelegate), &bytes);
    deserializer
        .read_header(context)
        .filter(|ok| *ok)
        .ok_or("Unable to deserialize cloned data due to invalid or unsupported version")?;
    deserializer
        .read_value(context)
        .ok_or_else(|| "Unable to deserialize cloned data".into())
}

pub(crate) fn read_default_host_object<'s>(scope: &mut v8::HandleScope<'s>, reader: &dyn ValueDeserializerHelper) -> Option<v8::Local<'s, v8::Object>> {
    v8::ValueDeserializerImpl::read_host_object(&DeserializerDelegate, scope, reader)
}
