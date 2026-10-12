//! Native-only handoff of closure-owned bridge budgets. A snapshot serializes
//! the initializer as context data; no Rust Global crosses an isolate boundary.
use crate::runtime_protocol::GuestBridgePolicy;
use agentos_executor_contract::GuestBridgeLimits;

pub(crate) const SNAPSHOT_INSTALLER_DATA_INDEX: usize = 0;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct InstalledPolicy {
    owner: Option<String>,
    limits: GuestBridgeLimits,
}

/// One bounded scalar owner pin per session survives context/isolate recreation.
pub(crate) fn pin_session(
    pinned: &mut Option<InstalledPolicy>,
    supplied: Option<&GuestBridgePolicy>,
) -> Result<(), String> {
    let expected = InstalledPolicy {
        owner: supplied.map(|policy| policy.vm_id.clone()),
        limits: supplied
            .map(|policy| policy.limits.clone())
            .unwrap_or_default(),
    };
    validate(&expected.limits)?;
    if let Some(existing) = pinned {
        if existing != &expected {
            return Err("bridge policy owner or limits changed; create a new session".into());
        }
    } else {
        *pinned = Some(expected);
    }
    Ok(())
}

/// Default bridge source stays byte-identical, preserving its existing hash.
/// Nondefaults become part of the existing bounded bridge/helper/cache input.
pub(crate) fn bridge_code_with_limits(
    code: &str,
    limits: &GuestBridgeLimits,
) -> Result<String, String> {
    validate(limits)?;
    if limits == &GuestBridgeLimits::default() {
        return Ok(code.to_owned());
    }
    let json = serde_json::to_string(limits).map_err(|error| error.to_string())?;
    let source = format!("Object.defineProperty(globalThis,\"__agentOSBridgeLimits\",{{configurable:true,enumerable:false,writable:false,value:Object.freeze({json})}});\n{code}");
    crate::snapshot::validate_bridge_code_size(&source)?;
    Ok(source)
}

fn validate(limits: &GuestBridgeLimits) -> Result<(), String> {
    for (name, value) in [
        (
            "limits.http.maxChunkSizeLineBytes",
            limits.max_chunk_size_line_bytes,
        ),
        (
            "limits.http.maxPendingResponseBytes",
            limits.max_pending_response_bytes,
        ),
        (
            "limits.jsRuntime.maxReadlineHistoryEntries",
            limits.max_readline_history_entries,
        ),
    ] {
        if value == 0 || value as u128 > ((1_u128 << 53) - 1) {
            return Err(format!("{name} must be a positive JavaScript-safe integer"));
        }
    }
    Ok(())
}

fn limits_from_object(
    scope: &mut v8::HandleScope,
    object: v8::Local<v8::Object>,
) -> Result<GuestBridgeLimits, String> {
    let mut values = [0; 3];
    for (index, name) in [
        "maxChunkSizeLineBytes",
        "maxPendingResponseBytes",
        "maxReadlineHistoryEntries",
    ]
    .into_iter()
    .enumerate()
    {
        let key = v8::String::new(scope, name).ok_or("bridge policy key allocation failed")?;
        let value = object
            .get(scope, key.into())
            .ok_or("bridge policy value unavailable")?;
        if !value.is_number() {
            return Err("bridge policy value is not numeric".into());
        }
        let number = value
            .number_value(scope)
            .ok_or("bridge policy numeric conversion failed")?;
        if !number.is_finite()
            || number.fract() != 0.0
            || number <= 0.0
            || number > ((1_u64 << 53) - 1) as f64
            || number > usize::MAX as f64
        {
            return Err("bridge policy value must be a positive representable safe integer".into());
        }
        values[index] = number as usize;
    }
    Ok(GuestBridgeLimits {
        max_chunk_size_line_bytes: values[0],
        max_pending_response_bytes: values[1],
        max_readline_history_entries: values[2],
    })
}

fn invoke(
    scope: &mut v8::HandleScope,
    installer: v8::Local<v8::Function>,
    limits: &GuestBridgeLimits,
) -> Result<(), String> {
    let object = v8::Object::new(scope);
    for (name, value) in [
        ("maxChunkSizeLineBytes", limits.max_chunk_size_line_bytes),
        ("maxPendingResponseBytes", limits.max_pending_response_bytes),
        (
            "maxReadlineHistoryEntries",
            limits.max_readline_history_entries,
        ),
    ] {
        let key = v8::String::new(scope, name).ok_or("bridge policy key allocation failed")?;
        let value = v8::Number::new(scope, value as f64);
        if object.create_data_property(scope, key.into(), value.into()) != Some(true) {
            return Err("bridge policy object installation failed".into());
        }
    }
    let tc = &mut v8::TryCatch::new(scope);
    let receiver = v8::undefined(tc);
    if installer
        .call(tc, receiver.into(), &[object.into()])
        .is_none()
    {
        let message = tc
            .exception()
            .map(|value| value.to_rust_string_lossy(tc))
            .unwrap_or_else(|| "no exception value".into());
        return Err(format!("bridge policy initializer failed: {message}"));
    }
    Ok(())
}

/// Capture/hide and install BEFORE any snapshot userland. The serialized tuple
/// contains a V8 function plus exactly three primitives, never a VM id or Global.
pub(crate) fn capture_installer<'s>(
    scope: &mut v8::HandleScope<'s>,
) -> Result<Option<v8::Local<'s, v8::Array>>, String> {
    let context = scope.get_current_context();
    let global = context.global(scope);
    let key = v8::String::new(scope, "__agentOSBridgeLimitInstaller")
        .ok_or("bridge installer key allocation failed")?;
    let value = global
        .get(scope, key.into())
        .ok_or("bridge installer lookup failed")?;
    let installer = if value.is_undefined() {
        None
    } else {
        Some(
            v8::Local::<v8::Function>::try_from(value)
                .map_err(|_| "bridge installer is not a function")?,
        )
    };
    if global.delete(scope, key.into()) != Some(true) {
        return Err("bridge installer could not be hidden".into());
    }
    let defaults_key = v8::String::new(scope, "__agentOSBridgeLimits")
        .ok_or("bridge defaults key allocation failed")?;
    let value = global
        .get(scope, defaults_key.into())
        .ok_or("bridge defaults lookup failed")?;
    let baked = if installer.is_some() {
        let object = v8::Local::<v8::Object>::try_from(value)
            .map_err(|_| "bridge defaults are not an object")?;
        Some(limits_from_object(scope, object)?)
    } else {
        None
    };
    if global.delete(scope, defaults_key.into()) != Some(true) {
        return Err("bridge defaults could not be hidden".into());
    }
    if let (Some(installer), Some(baked)) = (installer, baked) {
        invoke(scope, installer, &baked)?;
        let data = v8::Array::new(scope, 4);
        if data.set_index(scope, 0, installer.into()) != Some(true) {
            return Err("bridge installer tuple failed".into());
        }
        for (index, value) in [
            baked.max_chunk_size_line_bytes,
            baked.max_pending_response_bytes,
            baked.max_readline_history_entries,
        ]
        .into_iter()
        .enumerate()
        {
            let value = v8::Number::new(scope, value as f64);
            if data.set_index(scope, (index + 1) as u32, value.into()) != Some(true) {
                return Err("bridge installer tuple failed".into());
            }
        }
        Ok(Some(data))
    } else {
        Ok(None)
    }
}

/// Preserve the existing cold bridge compiler/cache and its original errors.
/// The session calls this inside the original watchdog lifecycle.
pub(crate) fn prepare(
    scope: &mut v8::HandleScope,
    supplied: Option<&GuestBridgePolicy>,
    from_snapshot: bool,
    bridge_code: &str,
    cache: &mut Option<crate::execution::BridgeCodeCache>,
) -> Result<(), crate::ipc::ExecutionError> {
    if !from_snapshot
        && scope
            .get_current_context()
            .get_slot::<InstalledPolicy>()
            .is_none()
    {
        crate::snapshot::inject_snapshot_defaults(scope).map_err(|message| {
            crate::ipc::ExecutionError {
                error_type: "Error".into(),
                message,
                stack: String::new(),
                code: Some("ERR_AGENTOS_BRIDGE_INIT".into()),
            }
        })?;
        let (code, error) = crate::execution::run_bridge_cached(scope, bridge_code, cache);
        if code != 0 {
            return Err(error.unwrap_or_else(|| crate::ipc::ExecutionError {
                error_type: "Error".into(),
                message: "cold bridge initialization failed".into(),
                stack: String::new(),
                code: Some("ERR_AGENTOS_BRIDGE_INIT".into()),
            }));
        }
    }
    install(scope, supplied, from_snapshot, bridge_code).map_err(|message| {
        crate::ipc::ExecutionError {
            error_type: "Error".into(),
            message,
            stack: String::new(),
            code: Some("ERR_AGENTOS_INVALID_BRIDGE_POLICY".into()),
        }
    })
}

/// Called after bridge bindings are ready, before trusted init/prelude or guest.
/// The context slot contains plain Rust values only, with no rooting-handle cycle.
pub(crate) fn install(
    scope: &mut v8::HandleScope,
    supplied: Option<&GuestBridgePolicy>,
    from_snapshot: bool,
    bridge_code: &str,
) -> Result<(), String> {
    let expected = InstalledPolicy {
        owner: supplied.map(|policy| policy.vm_id.clone()),
        limits: supplied
            .map(|policy| policy.limits.clone())
            .unwrap_or_default(),
    };
    validate(&expected.limits)?;
    let context = scope.get_current_context();
    if let Some(installed) = context.get_slot::<InstalledPolicy>() {
        return if installed == &expected {
            Ok(())
        } else {
            Err("bridge policy owner or limits changed; create a new execution context".into())
        };
    }
    let installer = if from_snapshot {
        match scope.get_context_data_from_snapshot_once::<v8::Array>(SNAPSHOT_INSTALLER_DATA_INDEX)
        {
            Ok(installer) => Some(installer),
            Err(v8::DataError::NoData { .. }) => None,
            Err(error) => return Err(format!("bridge installer snapshot data invalid: {error:?}")),
        }
    } else {
        capture_installer(scope)?
    };
    if let Some(data) = installer {
        if data.length() != 4 {
            return Err("bridge installer tuple length invalid".into());
        }
        let function = data.get_index(scope, 0).ok_or("bridge installer missing")?;
        let function = v8::Local::<v8::Function>::try_from(function)
            .map_err(|_| "bridge installer tuple function invalid")?;
        let object = v8::Object::new(scope);
        for (index, name) in [
            "maxChunkSizeLineBytes",
            "maxPendingResponseBytes",
            "maxReadlineHistoryEntries",
        ]
        .into_iter()
        .enumerate()
        {
            let value = data
                .get_index(scope, (index + 1) as u32)
                .ok_or("baked bridge policy missing")?;
            let key =
                v8::String::new(scope, name).ok_or("baked bridge policy key allocation failed")?;
            if object.create_data_property(scope, key.into(), value) != Some(true) {
                return Err("baked bridge policy object failed".into());
            }
        }
        let baked = limits_from_object(scope, object)?;
        if baked != expected.limits {
            return Err("snapshot bridge policy differs from requested VM limits; create a matching context".into());
        }
        invoke(scope, function, &expected.limits)?;
    } else if supplied.is_some() || bridge_code.contains("__agentOSBridgeLimitInstaller") {
        // Arbitrary legacy test bridges may have no HTTP/readline implementation.
        // An actual policy-aware bridge must never silently skip installation.
        return Err("required native bridge policy initializer unavailable".into());
    }
    if !context.set_slot(expected) {
        return Err("bridge policy context state unexpectedly replaced".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/support/guest_bridge_policy.rs"]
pub(crate) mod tests;
