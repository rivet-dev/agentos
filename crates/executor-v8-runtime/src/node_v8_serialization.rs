//! Node's default V8 wire format, evaluated entirely on the owning isolate.
use agentos_driver_tokio::accounting::{Reservation, ResourceClass, ResourceLedger};
use std::{ffi::c_void, sync::Arc};
use v8::ValueDeserializerHelper;

pub(crate) fn throw_limit(
    scope: &mut v8::PinScope<'_, '_>,
    limit: usize,
    requested: usize,
    path: &'static str,
) {
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

struct DeserializerDelegate;
impl v8::ValueDeserializerImpl for DeserializerDelegate {
    fn read_host_object<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        deserializer: &dyn ValueDeserializerHelper,
    ) -> Option<v8::Local<'s, v8::Object>> {
        let (mut kind, mut length) = (0, 0);
        if !deserializer.read_uint32(&mut kind) || !deserializer.read_uint32(&mut length) {
            return None;
        }
        // Validate tags/alignment before allocating even a byte for malformed input.
        let width = match kind {
            0..=2 | 9 | 10 => 1,
            3 | 4 | 13 => 2,
            5..=7 => 4,
            8 | 11 | 12 => 8,
            _ => return None,
        };
        if !(length as usize).is_multiple_of(width) {
            return None;
        }
        let bytes = deserializer.read_raw_bytes(length as usize)?;
        let backing = copied_backing_store(scope, bytes).ok()?;
        let buffer = v8::ArrayBuffer::with_backing_store(scope, &backing);
        let length = length as usize;
        macro_rules! view {
            ($name:ident, $size:expr) => {{
                if !length.is_multiple_of($size) {
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
            13 => v8::Float16Array::new(scope, buffer, 0, length / 2)?.to_object(scope),
            _ => None,
        }
    }
}

// Bound serializer allocations before V8 requests host memory. The C++ shim
// owns only V8's standard serializer and delegates allocation admission here.
struct SerializationBudget {
    resources: Arc<ResourceLedger>,
    reservation: Option<Reservation>,
    allocated: usize,
    error: Option<String>,
}
unsafe extern "C" fn admit_serialization(data: *mut c_void, bytes: usize) -> bool {
    // SAFETY: the synchronous C++ call borrows this stack value; it never stores
    // the callback or calls it after returning, and never runs it concurrently.
    let budget = unsafe { &mut *data.cast::<SerializationBudget>() };
    if bytes <= budget.allocated {
        return true;
    }
    match budget
        .resources
        .reserve(ResourceClass::ExecutorBytes, bytes - budget.allocated)
    {
        Ok(extra) => {
            if let Some(reservation) = &mut budget.reservation {
                if reservation.merge(extra).is_err() {
                    budget.error = Some("Serialization allocation accounting failed".into());
                    return false;
                }
            } else {
                budget.reservation = Some(extra);
            }
            budget.allocated = bytes;
            true
        }
        Err(error) => {
            budget.error = Some(error.to_string());
            false
        }
    }
}
extern "C" {
    fn agentos_v8_serialize_bounded(
        context: *const v8::Context,
        value: *const v8::Value,
        limit: usize,
        admit: unsafe extern "C" fn(*mut c_void, usize) -> bool,
        budget: *mut c_void,
        bytes: *mut *mut u8,
        length: *mut usize,
        rejected: *mut usize,
        allocation_failed: *mut bool,
    ) -> bool;
}
unsafe extern "C" fn free_serialized_buffer(
    data: *mut c_void,
    _length: usize,
    _state: *mut c_void,
) {
    // SAFETY: the C++ serializer hands off a unique malloc allocation exactly
    // once. V8 owns it until this backing-store deleter runs.
    unsafe { libc::free(data) };
}

pub(crate) fn serialize<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    limit: usize,
    path: &'static str,
    resources: Arc<ResourceLedger>,
) -> Result<v8::Local<'s, v8::Value>, String> {
    v8::tc_scope!(let scope, scope);
    let mut budget = SerializationBudget {
        resources,
        reservation: None,
        allocated: 0,
        error: None,
    };
    let (mut data, mut length, mut rejected, mut allocation_failed) =
        (std::ptr::null_mut(), 0, 0, false);
    // SAFETY: these handles belong to this live isolate/context, every output
    // pointer is valid for the call, and the budget callback cannot outlive it.
    let ok = unsafe {
        agentos_v8_serialize_bounded(
            &*scope.get_current_context(),
            &*value,
            limit,
            admit_serialization,
            (&mut budget as *mut SerializationBudget).cast(),
            &mut data,
            &mut length,
            &mut rejected,
            &mut allocation_failed,
        )
    };
    if !ok {
        if rejected != 0 {
            scope.reset();
            if let Some(error) = budget.error {
                return Err(error);
            }
            throw_limit(scope, limit, rejected, path);
            scope.rethrow();
            return Err(format!("Serialized value exceeds {path}"));
        }
        if allocation_failed {
            return Err(
                "ERR_AGENTOS_V8_SERIALIZATION_ALLOCATION: unable to allocate serialized bytes"
                    .into(),
            );
        }
        scope.rethrow();
        return Err("Unable to serialize value".into());
    }
    if let Err(error) = crate::isolate::admit_external_allocation(scope, length) {
        // SAFETY: no backing store owns this successful C++ allocation yet.
        unsafe { libc::free(data.cast()) };
        return Err(error);
    }
    if length >= limit.saturating_mul(4) / 5 {
        tracing::warn!(
            bytes = length,
            limit,
            configuration_path = path,
            "V8 serialization approaches its configured byte limit"
        );
    }
    // SAFETY: the serializer shrank its unique malloc allocation to `length`;
    // the deleter matches that allocator and V8 takes over lifetime management.
    let backing = unsafe {
        v8::ArrayBuffer::new_backing_store_from_ptr(
            data.cast(),
            length,
            free_serialized_buffer,
            std::ptr::null_mut(),
        )
    }
    .make_shared();
    let buffer = v8::ArrayBuffer::with_backing_store(scope, &backing);
    v8::Uint8Array::new(scope, buffer, 0, length)
        .map(Into::into)
        .ok_or_else(|| "Unable to allocate serialized value".into())
}

pub(crate) fn deserialize<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    limit: usize,
    path: &'static str,
    resources: Arc<ResourceLedger>,
) -> Result<v8::Local<'s, v8::Value>, String> {
    let view = v8::Local::<v8::ArrayBufferView>::try_from(value)
        .map_err(|_| "buffer must be a TypedArray or DataView")?;
    if view.byte_length() > limit {
        throw_limit(scope, limit, view.byte_length(), path);
        return Err(format!("Serialized input exceeds {path}"));
    }
    let _bytes = resources
        .reserve(ResourceClass::ExecutorBytes, view.byte_length())
        .map_err(|error| error.to_string())?;
    let bytes = copy_view(scope, view)?;
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

pub(crate) fn read_default_host_object<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    reader: &dyn ValueDeserializerHelper,
) -> Option<v8::Local<'s, v8::Object>> {
    v8::ValueDeserializerImpl::read_host_object(&DeserializerDelegate, scope, reader)
}

/// Copy native input only after its owning caller has admitted the byte count.
/// Do not use infallible Vec allocation at the guest/native boundary.
pub(crate) fn copy_view(
    scope: &mut v8::PinScope<'_, '_>,
    view: v8::Local<v8::ArrayBufferView>,
) -> Result<Vec<u8>, String> {
    let length = view.byte_length();
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| "ERR_AGENTOS_V8_SERIALIZATION_ALLOCATION: unable to copy serialized bytes")?;
    bytes.resize(length, 0);
    let backing = view
        .buffer(scope)
        .ok_or("Serialized input has no buffer")?
        .get_backing_store();
    let offset = view.byte_offset();
    if offset > backing.byte_length() || length > backing.byte_length() - offset {
        return Err("Serialized input changed before copying".into());
    }
    if length != 0 {
        let data = backing
            .data()
            .ok_or("Serialized input has no backing data")?;
        // SAFETY: the held backing store covers this validated range. No guest
        // code runs between capture and copy, and workers cannot share it.
        // Avoid the binding's i32 CopyContents length conversion and stale stored
        // lengths for length-tracking resizable views.
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr().cast::<u8>().add(offset),
                bytes.as_mut_ptr(),
                length,
            );
        }
    }
    Ok(bytes)
}

pub(crate) fn copied_backing_store(
    scope: &mut v8::PinScope<'_, '_>,
    bytes: &[u8],
) -> Result<v8::SharedRef<v8::BackingStore>, String> {
    crate::isolate::admit_external_allocation(scope, bytes.len()).inspect_err(|error| {
        if let Some(message) = v8::String::new(scope, error) {
            let exception = v8::Exception::range_error(scope, message);
            scope.throw_exception(exception);
        }
    })?;
    // V8's C++ ArrayBuffer::New aborts the process on allocator exhaustion.
    // Use a fallible allocation here, after external-memory admission, and
    // transfer ownership without a second copy. V8 then accounts its lifetime.
    let mut owned = Vec::new();
    owned
        .try_reserve_exact(bytes.len())
        .map_err(|_| "ERR_AGENTOS_V8_SERIALIZATION_ALLOCATION: unable to copy host object")?;
    owned.extend_from_slice(bytes);
    Ok(v8::ArrayBuffer::new_backing_store_from_vec(owned).make_shared())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentos_driver_tokio::accounting::ResourceLimit;

    fn eval<'s>(scope: &mut v8::PinScope<'s, '_>, source: &str) -> v8::Local<'s, v8::Value> {
        let source = v8::String::new(scope, source).unwrap();
        v8::Script::compile(scope, source, None)
            .unwrap()
            .run(scope)
            .unwrap()
    }
    fn ledger(limit: usize) -> Arc<ResourceLedger> {
        Arc::new(ResourceLedger::root(
            "serializer-test",
            [(
                ResourceClass::ExecutorBytes,
                ResourceLimit::new(limit, "runtime.blocking.maxQueuedBytes"),
            )],
        ))
    }
    #[test]
    fn node_writer_emits_wire_15_and_reader_accepts_native_wire_16() {
        use v8::ValueSerializerHelper;

        struct NativeDelegate;
        impl v8::ValueSerializerImpl for NativeDelegate {
            fn throw_data_clone_error<'s>(
                &self,
                scope: &mut v8::PinScope<'s, '_>,
                message: v8::Local<'s, v8::String>,
            ) {
                let error = v8::Exception::error(scope, message);
                scope.throw_exception(error);
            }
        }

        let mut isolate = crate::isolate::create_isolate(Some(32));
        let context = crate::isolate::create_context(&mut isolate);
        v8::scope!(let scope, &mut *isolate);
        let context = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let value = eval(scope, "(() => { const b = new ArrayBuffer(2, {maxByteLength:9}); new Uint8Array(b).set([17,31]); return b; })()");
        let resources = ledger(1024 * 1024);
        let path = "limits.jsRuntime.v8SerializationLimitBytes";
        let wire = serialize(scope, value, 4096, path, resources.clone()).unwrap();
        let wire = v8::Local::<v8::ArrayBufferView>::try_from(wire).unwrap();
        // Produced by node:v8.serialize on Node 24.18.0 and 26.13.0.
        assert_eq!(
            copy_view(scope, wire).unwrap(),
            [255, 15, 126, 2, 9, 17, 31]
        );

        let native = v8::ValueSerializer::new(scope, Box::new(NativeDelegate));
        native.write_header();
        assert_eq!(native.write_value(context, value), Some(true));
        let bytes = native.release();
        assert_eq!(&bytes[..2], &[255, 16]);
        let backing = copied_backing_store(scope, &bytes).unwrap();
        let buffer = v8::ArrayBuffer::with_backing_store(scope, &backing);
        let input = v8::Uint8Array::new(scope, buffer, 0, bytes.len()).unwrap();
        let restored = deserialize(scope, input.into(), 4096, path, resources).unwrap();
        let restored = v8::Local::<v8::ArrayBuffer>::try_from(restored).unwrap();
        assert!(restored.is_resizable_by_user_javascript());
        assert_eq!(restored.byte_length(), 2);
        assert_eq!(restored.get_backing_store().max_byte_length(), 9);
        let view = v8::Uint8Array::new(scope, restored, 0, 2).unwrap();
        assert_eq!(copy_view(scope, view.into()).unwrap(), [17, 31]);
    }

    #[test]
    fn bounded_serializer_preserves_graphs_and_exact_wire_limit() {
        let mut isolate = crate::isolate::create_isolate(Some(32));
        let context = crate::isolate::create_context(&mut isolate);
        v8::scope!(let scope, &mut *isolate);
        let context = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let value = eval(scope, "(()=>{const v={n:9n,typed:new Uint16Array([500,600])};v.self=v;v.alias=v.typed;return v})()");
        let resources = ledger(1024 * 1024);
        let wire = serialize(
            scope,
            value,
            4096,
            "limits.jsRuntime.v8SerializationLimitBytes",
            resources.clone(),
        )
        .unwrap();
        let length = v8::Local::<v8::ArrayBufferView>::try_from(wire)
            .unwrap()
            .byte_length();
        assert!(
            length < 66,
            "exercise V8's minimum allocation above the exact wire limit"
        );
        assert!(serialize(
            scope,
            value,
            length,
            "limits.jsRuntime.v8SerializationLimitBytes",
            resources.clone()
        )
        .is_ok());
        let restored = deserialize(
            scope,
            wire,
            length,
            "limits.jsRuntime.v8SerializationLimitBytes",
            resources.clone(),
        )
        .unwrap();
        let key = v8::String::new(scope, "restored").unwrap();
        scope
            .get_current_context()
            .global(scope)
            .set(scope, key.into(), restored);
        assert!(eval(scope, "restored.self===restored&&restored.alias===restored.typed&&restored.n===9n&&restored.typed[1]===600").is_true());
        assert_eq!(resources.usage(ResourceClass::ExecutorBytes).used, 0);
    }
    #[test]
    fn bounded_serializer_rejects_growth_and_releases_reservations() {
        let mut isolate = crate::isolate::create_isolate(Some(32));
        let context = crate::isolate::create_context(&mut isolate);
        v8::scope!(let scope, &mut *isolate);
        let context = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let resources = ledger(256);
        let value = eval(scope, "'x'.repeat(1024)");
        let result = serialize(
            scope,
            value,
            4096,
            "limits.jsRuntime.v8SerializationLimitBytes",
            resources.clone(),
        );
        assert!(result
            .unwrap_err()
            .contains("runtime.blocking.maxQueuedBytes"));
        assert_eq!(resources.usage(ResourceClass::ExecutorBytes).used, 0);
        v8::tc_scope!(let scope, scope);
        assert!(serialize(
            scope,
            value,
            128,
            "limits.jsRuntime.v8SerializationLimitBytes",
            ledger(1024 * 1024)
        )
        .is_err());
        assert!(scope.has_caught());
    }
    #[test]
    fn bounded_serializer_copies_only_visible_and_length_tracking_bytes() {
        let mut isolate = crate::isolate::create_isolate(Some(32));
        let context = crate::isolate::create_context(&mut isolate);
        v8::scope!(let scope, &mut *isolate);
        let context = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let value = eval(scope, "(()=>{const b=new ArrayBuffer(4,{maxByteLength:16});const v=new Uint8Array(b);b.resize(8);v.set([1,2,3,4,5,6,7,8]);return v})()");
        let view = v8::Local::<v8::ArrayBufferView>::try_from(value).unwrap();
        assert_eq!(
            copy_view(scope, view).unwrap(),
            vec![1, 2, 3, 4, 5, 6, 7, 8]
        );
        let wire = serialize(
            scope,
            value,
            128,
            "limits.jsRuntime.v8SerializationLimitBytes",
            ledger(1024),
        )
        .unwrap();
        let restored = deserialize(
            scope,
            wire,
            128,
            "limits.jsRuntime.v8SerializationLimitBytes",
            ledger(1024),
        )
        .unwrap();
        assert_eq!(
            copy_view(
                scope,
                v8::Local::<v8::ArrayBufferView>::try_from(restored).unwrap()
            )
            .unwrap(),
            vec![1, 2, 3, 4, 5, 6, 7, 8]
        );
        let value = eval(
            scope,
            "(()=>{const all=new Uint8Array([99,98,1,2,97]);return all.subarray(2,4)})()",
        );
        let wire = serialize(
            scope,
            value,
            128,
            "limits.jsRuntime.v8SerializationLimitBytes",
            ledger(1024),
        )
        .unwrap();
        let raw = copy_view(
            scope,
            v8::Local::<v8::ArrayBufferView>::try_from(wire).unwrap(),
        )
        .unwrap();
        assert!(!raw.contains(&99) && !raw.contains(&98) && !raw.contains(&97));
    }
}
