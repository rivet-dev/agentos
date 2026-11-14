//! Cursor-owning Node V8 readers. Native state never leaves its isolate.
use agentos_driver_tokio::accounting::{Reservation, ResourceClass, ResourceLedger};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};
use v8::ValueDeserializerHelper;

struct Reader {
    // V8 borrows the stable allocation in `input`; destroy V8 before that input.
    decoder: v8::ValueDeserializer<'static>,
    receiver: v8::Weak<v8::Object>,
    _input: Vec<u8>,
    _handle: Reservation,
    _bytes: Reservation,
}
#[derive(Default)]
struct Readers {
    next_id: u32,
    entries: BTreeMap<u32, Rc<Reader>>,
}
#[derive(Clone, Default)]
struct Registry(Rc<RefCell<Readers>>);

fn registry(scope: &mut v8::Isolate) -> Registry {
    if let Some(registry) = scope.get_slot::<Registry>() {
        return registry.clone();
    }
    let registry = Registry::default();
    scope.set_slot(registry.clone());
    registry
}
fn key<'s>(scope: &mut v8::HandleScope<'s>) -> v8::Local<'s, v8::Private> {
    let name = v8::String::new(scope, "agentOS.node.v8.deserializer").unwrap();
    v8::Private::for_api(scope, Some(name))
}
pub(crate) fn reset(isolate: &mut v8::Isolate) {
    if let Some(registry) = isolate.get_slot::<Registry>().cloned() {
        // Drop weak finalizers outside the registry borrow.
        let entries = std::mem::take(&mut registry.0.borrow_mut().entries);
        drop(entries);
    }
}
pub(crate) fn release(isolate: &mut v8::Isolate) {
    if let Some(registry) = isolate.remove_slot::<Registry>() {
        let entries = std::mem::take(&mut registry.0.borrow_mut().entries);
        drop(entries);
    }
}
pub(crate) struct RegistryGuard(std::rc::Weak<RefCell<Readers>>);
impl RegistryGuard {
    pub(crate) fn new(isolate: &mut v8::Isolate) -> Self {
        Self(Rc::downgrade(&registry(isolate).0))
    }
}
impl Drop for RegistryGuard {
    fn drop(&mut self) {
        if let Some(registry) = self.0.upgrade() {
            let entries = std::mem::take(&mut registry.borrow_mut().entries);
            drop(entries);
        }
    }
}
struct Delegate(v8::Weak<v8::Object>);
impl v8::ValueDeserializerImpl for Delegate {
    fn read_host_object<'s>(
        &self,
        scope: &mut v8::HandleScope<'s>,
        _decoder: &dyn ValueDeserializerHelper,
    ) -> Option<v8::Local<'s, v8::Object>> {
        // V8 invokes ReadHostObject under a no-JavaScript scope. Node permits
        // its user hook here; keep that permission scoped to this callback.
        let scope = &mut v8::AllowJavascriptExecutionScope::new(scope);
        let receiver = self.0.to_local(scope)?;
        let name = v8::String::new(scope, "_readHostObject")?;
        let method = receiver.get(scope, name.into())?;
        let method = v8::Local::<v8::Function>::try_from(method).ok()?;
        // Reentrant read methods retrieve this same decoder without retaining a
        // registry borrow across the guest callback. V8 owns graph identities.
        let value = method.call(scope, receiver.into(), &[])?;
        v8::Local::<v8::Object>::try_from(value).ok()
    }
}

pub(crate) fn call<'s>(
    scope: &mut v8::HandleScope<'s>,
    args: &v8::FunctionCallbackArguments<'s>,
    method: &str,
    limit: usize,
    path: &'static str,
    resources: Arc<ResourceLedger>,
) -> Result<v8::Local<'s, v8::Value>, String> {
    let receiver = v8::Local::<v8::Object>::try_from(args.get(0))
        .map_err(|_| "Invalid Deserializer receiver")?;
    let private = key(scope);
    let registry = registry(scope);
    if method == "v8.deserializer.create" {
        let existing = receiver
            .get_private(scope, private)
            .ok_or("Invalid Deserializer receiver")?;
        if !existing.is_undefined() {
            return Err("Deserializer already initialized".into());
        }
        let input = v8::Local::<v8::ArrayBufferView>::try_from(args.get(1))
            .map_err(|_| "Deserializer input must be a TypedArray or DataView")?;
        if input.byte_length() > limit {
            crate::node_v8_serialization::throw_limit(scope, limit, input.byte_length(), path);
            return Err(format!("Deserializer input exceeds {path}"));
        }
        let handle = resources
            .reserve(ResourceClass::Capabilities, 1)
            .map_err(|error| error.to_string())?;
        let bytes = resources
            .reserve(ResourceClass::ExecutorBytes, input.byte_length())
            .map_err(|error| error.to_string())?;
        let mut input_bytes = vec![0; input.byte_length()];
        input.copy_contents(&mut input_bytes);
        let delegate_receiver = v8::Weak::new(scope, receiver);
        let decoder =
            v8::ValueDeserializer::new(scope, Box::new(Delegate(delegate_receiver)), &input_bytes);
        let id = {
            let mut state = registry.0.borrow_mut();
            state.next_id = state
                .next_id
                .checked_add(1)
                .ok_or("Deserializer identity space exhausted")?;
            state.next_id
        };
        let weak_registry = Rc::downgrade(&registry.0);
        let weak = v8::Weak::with_finalizer(
            scope,
            receiver,
            Box::new(move |_| {
                if let Some(registry) = weak_registry.upgrade() {
                    let removed = registry.borrow_mut().entries.remove(&id);
                    drop(removed);
                }
            }),
        );
        let owner = Rc::new(Reader {
            decoder,
            receiver: weak,
            _input: input_bytes,
            _handle: handle,
            _bytes: bytes,
        });
        let value = v8::Integer::new_from_unsigned(scope, id);
        receiver
            .set_private(scope, private, value.into())
            .ok_or("Unable to initialize Deserializer")?;
        registry.0.borrow_mut().entries.insert(id, owner);
        return Ok(v8::undefined(scope).into());
    }
    let id = receiver
        .get_private(scope, private)
        .and_then(|value| value.uint32_value(scope))
        .ok_or("Invalid Deserializer receiver")?;
    let owner = registry
        .0
        .borrow()
        .entries
        .get(&id)
        .cloned()
        .ok_or("Deserializer has been retired")?;
    if !owner
        .receiver
        .to_local(scope)
        .is_some_and(|original| original.strict_equals(receiver.into()))
    {
        return Err("Invalid Deserializer receiver".into());
    }
    let context = scope.get_current_context();
    match method {
        "v8.deserializer.readHeader" => owner
            .decoder
            .read_header(context)
            .filter(|ok| *ok)
            .map(|_| v8::Boolean::new(scope, true).into())
            .ok_or_else(|| {
                "Unable to deserialize cloned data due to invalid or unsupported version".into()
            }),
        "v8.deserializer.readValue" => owner
            .decoder
            .read_value(context)
            .ok_or_else(|| "Unable to deserialize cloned data".into()),
        "v8.deserializer.getWireFormatVersion" => Ok(v8::Integer::new_from_unsigned(
            scope,
            owner.decoder.get_wire_format_version(),
        )
        .into()),
        "v8.deserializer.readUint32" => {
            let mut value = 0;
            if !owner.decoder.read_uint32(&mut value) {
                return Err("ReadUint32 failed".into());
            }
            Ok(v8::Integer::new_from_unsigned(scope, value).into())
        }
        "v8.deserializer.readUint64" => {
            let mut value = 0;
            if !owner.decoder.read_uint64(&mut value) {
                return Err("ReadUint64 failed".into());
            }
            let high = v8::Integer::new_from_unsigned(scope, (value >> 32) as u32);
            let low = v8::Integer::new_from_unsigned(scope, value as u32);
            Ok(v8::Array::new_with_elements(scope, &[high.into(), low.into()]).into())
        }
        "v8.deserializer.readDouble" => {
            let mut value = 0.;
            if !owner.decoder.read_double(&mut value) {
                return Err("ReadDouble failed".into());
            }
            Ok(v8::Number::new(scope, value).into())
        }
        "v8.deserializer.readRawBytes" => {
            let count = args
                .get(1)
                .number_value(scope)
                .ok_or("Invalid raw byte count")?;
            if !count.is_finite() || count < 0. || count.fract() != 0. || count > limit as f64 {
                return Err("Invalid raw byte count".into());
            }
            let bytes = owner
                .decoder
                .read_raw_bytes(count as usize)
                .ok_or("ReadRawBytes failed")?;
            let length = bytes.len();
            let backing = v8::ArrayBuffer::new_backing_store_from_vec(bytes.to_vec()).make_shared();
            let buffer = v8::ArrayBuffer::with_backing_store(scope, &backing);
            Ok(v8::Uint8Array::new(scope, buffer, 0, length)
                .ok_or("Unable to allocate raw bytes")?
                .into())
        }
        "v8.deserializer.readHostObject" => {
            crate::node_v8_serialization::read_default_host_object(scope, &owner.decoder)
                .map(Into::into)
                .ok_or_else(|| "Unable to deserialize host object".into())
        }
        "v8.deserializer.transferArrayBuffer" => {
            let id = args
                .get(1)
                .uint32_value(scope)
                .ok_or("Invalid transfer identifier")?;
            let buffer = v8::Local::<v8::ArrayBuffer>::try_from(args.get(2))
                .map_err(|_| "Transfer requires an ArrayBuffer")?;
            owner.decoder.transfer_array_buffer(id, buffer);
            Ok(v8::undefined(scope).into())
        }
        _ => Err("Unsupported Deserializer operation".into()),
    }
}
