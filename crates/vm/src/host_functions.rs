use crate::core::host_functions::{
    ensure_collection_name_available as core_ensure_collection_name_available,
    ensure_command_aliases_available as core_ensure_command_aliases_available,
    ensure_host_function_registry_capacity_with_limits, registered_host_function_command_names,
    validate_host_functions_registration as core_validate_host_functions_registration,
    validate_host_functions_registration_with_limits, HostFunctionRegistrationError,
};
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use crate::core::host_functions::{
    MAX_EXAMPLES_PER_HOST_FUNCTION, MAX_HOST_FUNCTIONS_PER_COLLECTION,
    MAX_HOST_FUNCTION_DESCRIPTION_LENGTH, MAX_HOST_FUNCTION_EXAMPLE_INPUT_BYTES,
    MAX_HOST_FUNCTION_SCHEMA_BYTES, MAX_HOST_FUNCTION_SCHEMA_DEPTH, MAX_HOST_FUNCTION_TIMEOUT_MS,
    MAX_REGISTERED_HOST_FUNCTIONS_PER_VM, MAX_REGISTERED_HOST_FUNCTION_COLLECTIONS,
};
use crate::core::permissions::{deny_all_policy, evaluate_permissions_policy};
use crate::core::respond as shared_respond;
use crate::protocol::{
    HostCallbackRequest, HostCallbacksRegisteredResponse, RegisterHostCallbacksRequest,
    RequestFrame, ResponsePayload,
};
use crate::service::{kernel_error, normalize_path, DispatchResult};
use crate::state::{BridgeError, SharedBridge, VmHandle, VmState, HOST_FUNCTION_DRIVER_NAME};
use crate::{VmError, VmManager, VmManagerHost};
use agentos_vm_config::PermissionMode;
use agentos_vm_kernel::command_registry::CommandDriver;
use serde_json::{json, Map, Number, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub(crate) enum HostFunctionCommandResolution {
    Invoke {
        request: HostCallbackRequest,
        timeout: Duration,
    },
    Failure(String),
}

pub(crate) fn format_host_function_failure_output(message: &str) -> Vec<u8> {
    let mut output = message.as_bytes().to_vec();
    if !output.ends_with(b"\n") {
        output.push(b'\n');
    }
    output
}

pub(crate) fn register_host_callbacks<B>(
    sidecar: &mut VmManager<B>,
    request: &RequestFrame,
    payload: RegisterHostCallbacksRequest,
) -> impl std::future::Future<Output = Result<DispatchResult, VmError>> + 'static
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let input = sidecar.prepare_owned_vm_route(request);
    let bridge = sidecar.bridge.clone();
    async move {
        let input = input?;
        let registered_name = payload.name.clone();
        let (original_permissions, original_host_functions, original_command_guest_paths) = input
            .vm
            .try_read("snapshot host callback registration", |vm| {
                (
                    vm.configuration.permissions.clone(),
                    vm.host_functions.clone(),
                    vm.command_guest_paths.clone(),
                )
            })?;
        // Reject preflight failures before mutation so rollback cannot mask a
        // typed capacity error or unnecessarily re-register an unchanged driver.
        input
            .vm
            .try_read("validate host callback registration", |vm| {
                validate_host_functions_registration_with_limits(
                    &payload,
                    &vm.limits.host_functions,
                )
                .map_err(host_function_registration_error)?;
                ensure_collection_name_available(&vm.host_functions, &registered_name)?;
                ensure_command_aliases_available(&vm.host_functions, &payload)?;
                ensure_host_function_registry_capacity_with_limits(
                    &vm.host_functions,
                    &payload,
                    &vm.limits.host_functions,
                )
                .map_err(host_function_registration_error)
            })??;
        // Operator-only command registration must not relax live guest permissions.
        let registration_result = input.vm.try_command("register host callbacks", |vm| {
            vm.host_functions.insert(registered_name.clone(), payload);
            refresh_host_function_registry(vm)?;
            Ok(host_function_command_names(vm).len() as u32)
        });
        let command_count = match registration_result {
            Ok(result) => {
                if let Err(restore_error) =
                    bridge.set_vm_permissions(&input.vm_id, &original_permissions)
                {
                    rollback_host_callback_registration(
                        &input.vm,
                        &bridge,
                        &input.vm_id,
                        &original_permissions,
                        original_host_functions,
                        original_command_guest_paths,
                        &restore_error,
                    )?;
                    return Err(restore_error);
                }
                result
            }
            Err(error) => {
                match rollback_host_callback_registration(
                    &input.vm,
                    &bridge,
                    &input.vm_id,
                    &original_permissions,
                    original_host_functions,
                    original_command_guest_paths,
                    &error,
                ) {
                    Ok(()) => return Err(error),
                    Err(rollback_error) => return Err(rollback_error),
                }
            }
        };

        Ok(DispatchResult {
            response: shared_respond(
                &input.request,
                ResponsePayload::HostCallbacksRegistered(HostCallbacksRegisteredResponse {
                    registration: registered_name,
                    command_count,
                }),
            ),
            events: Vec::new(),
        })
    }
}

fn rollback_host_callback_registration<B>(
    vm: &VmHandle,
    bridge: &SharedBridge<B>,
    vm_id: &str,
    original_permissions: &agentos_vm_config::PermissionsPolicy,
    original_host_functions: BTreeMap<String, RegisterHostCallbacksRequest>,
    original_command_guest_paths: BTreeMap<String, String>,
    operation_error: &VmError,
) -> Result<(), VmError>
where
    B: VmManagerHost + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    // These two rollback legs are independent. In particular, a failed VM
    // registry refresh must never skip restoring (or fail-closing) the trusted
    // bridge permission policy.
    let state_rollback = vm.try_command("rollback host callback registration", |vm| {
        vm.host_functions = original_host_functions;
        vm.command_guest_paths = original_command_guest_paths;
        refresh_host_function_registry(vm)
    });
    let permission_rollback = bridge.restore_vm_permissions_fail_closed(
        vm_id,
        original_permissions,
        "host function collection registration rollback",
        operation_error,
    );

    match (state_rollback, permission_rollback) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(state_error), Ok(())) => Err(VmError::InvalidState(format!(
            "host function collection registration state rollback failed after {operation_error}: {state_error}; original permissions were restored"
        ))),
        (Ok(()), Err(permission_error)) => {
            vm.try_command("record fail-closed host function permissions", |vm| {
                vm.configuration.permissions = deny_all_policy();
                Ok(())
            })?;
            Err(permission_error)
        }
        (Err(state_error), Err(permission_error)) => {
            let fail_closed_state =
                vm.try_command("record fail-closed host function permissions", |vm| {
                    vm.configuration.permissions = deny_all_policy();
                    Ok(())
                });
            Err(VmError::InvalidState(format!(
                "host function collection registration rollback failed after {operation_error}: state rollback: {state_error}; permission rollback: {permission_error}; fail-closed VM state: {fail_closed_state:?}"
            )))
        }
    }
}

fn refresh_host_function_registry(vm: &mut VmState) -> Result<(), VmError> {
    let commands = host_function_command_names(vm);
    vm.kernel
        .register_driver_for_operator(CommandDriver::new(
            HOST_FUNCTION_DRIVER_NAME,
            commands.iter().cloned(),
        ))
        .map_err(kernel_error)?;

    for command in commands {
        vm.command_guest_paths
            .insert(command.clone(), format!("/bin/{command}"));
    }
    Ok(())
}

pub(crate) fn resolve_host_function_command(
    vm: &mut VmState,
    command: &str,
    args: &[String],
    cwd: Option<&str>,
) -> Result<Option<HostFunctionCommandResolution>, VmError> {
    let Some(kind) = identify_host_function_command(vm, command) else {
        return Ok(None);
    };
    let guest_cwd = cwd
        .map(normalize_path)
        .unwrap_or_else(|| vm.guest_cwd.clone());
    let resolution = match kind {
        HostFunctionCommand::Registry(command_name) => {
            resolve_registry_command(vm, &command_name, args, &guest_cwd)?
        }
        HostFunctionCommand::Collection { collection_name } => {
            resolve_host_function_collection_command(vm, &collection_name, args, &guest_cwd)?
        }
    };
    Ok(Some(resolution))
}

pub(crate) fn is_host_function_command(vm: &VmState, command: &str) -> bool {
    identify_host_function_command(vm, command).is_some()
}

pub(crate) fn normalized_host_function_command_name(command: &str) -> Option<String> {
    host_function_command_name_from_specifier(command).map(ToOwned::to_owned)
}

fn identify_host_function_command(vm: &VmState, command: &str) -> Option<HostFunctionCommand> {
    let command_name = host_function_command_name_from_specifier(command).unwrap_or(command);

    if vm.host_functions.values().any(|collection| {
        collection
            .registry_command_aliases
            .iter()
            .any(|alias| alias == command_name)
    }) {
        return Some(HostFunctionCommand::Registry(command_name.to_owned()));
    }

    vm.host_functions
        .iter()
        .find(|(_collection_name, collection)| {
            collection
                .command_aliases
                .iter()
                .any(|alias| alias == command_name)
        })
        .map(
            |(collection_name, _collection)| HostFunctionCommand::Collection {
                collection_name: collection_name.to_owned(),
            },
        )
}

fn host_function_command_name_from_specifier(command: &str) -> Option<&str> {
    let file_name = Path::new(command).file_name()?.to_str()?;
    let normalized = normalize_path(command);
    let registered_internal_path = normalized
        .strip_prefix("/__agentos/commands/")
        .and_then(|suffix| suffix.rsplit('/').next())
        .is_some_and(|name| name == file_name);
    if !matches!(
        normalized.as_str(),
        path if path == format!("/bin/{file_name}")
            || path == format!("/usr/bin/{file_name}")
            || path == format!("/usr/local/bin/{file_name}")
    ) && !registered_internal_path
    {
        return None;
    }
    Some(file_name)
}

fn resolve_registry_command(
    vm: &mut VmState,
    command_name: &str,
    args: &[String],
    guest_cwd: &str,
) -> Result<HostFunctionCommandResolution, VmError> {
    // `agentos <collection> <function> ...` invokes a host function through the
    // host's registry command, so it needs the same `hostFunction.invoke` check
    // as the `agentos-<collection> <function>` command. Listing and help are not
    // gated.
    if let [collection_name, host_function_name, ..] = args {
        let registered = vm
            .host_functions
            .get(collection_name)
            .is_some_and(|collection| collection.callbacks.contains_key(host_function_name));
        if registered {
            let callback_key = format!("{collection_name}:{host_function_name}");
            if !matches!(
                evaluate_permissions_policy(
                    &vm.configuration.permissions,
                    "hostFunction",
                    "hostFunction.invoke",
                    Some(&callback_key),
                ),
                PermissionMode::Allow
            ) {
                return Ok(HostFunctionCommandResolution::Failure(format!(
                    "blocked by hostFunction.invoke policy for {callback_key}"
                )));
            }
        }
    }
    let timeout_ms =
        command_callback_timeout_ms(vm, &HostFunctionCommand::Registry(command_name.to_owned()));
    Ok(build_command_callback_resolution(
        command_name,
        build_registry_command_input(command_name, args, guest_cwd),
        timeout_ms,
    ))
}

fn resolve_host_function_collection_command(
    vm: &mut VmState,
    collection_name: &str,
    args: &[String],
    _guest_cwd: &str,
) -> Result<HostFunctionCommandResolution, VmError> {
    let Some((host_function_name, host_function_args)) = args.split_first() else {
        return Ok(HostFunctionCommandResolution::Failure(format!(
            "collection command {collection_name} requires a host function name"
        )));
    };
    let callback_key = format!("{collection_name}:{host_function_name}");
    let Some(host_function) = vm
        .host_functions
        .get(collection_name)
        .and_then(|collection| collection.callbacks.get(host_function_name))
        .cloned()
    else {
        return Ok(HostFunctionCommandResolution::Failure(format!(
            "unknown host function callback {callback_key}"
        )));
    };
    if !matches!(
        evaluate_permissions_policy(
            &vm.configuration.permissions,
            "hostFunction",
            "hostFunction.invoke",
            Some(&callback_key),
        ),
        PermissionMode::Allow
    ) {
        return Ok(HostFunctionCommandResolution::Failure(format!(
            "blocked by hostFunction.invoke policy for {callback_key}"
        )));
    }

    let input_schema: Value =
        serde_json::from_str(&host_function.input_schema).map_err(|error| {
            VmError::InvalidState(format!(
                "host function {callback_key} input schema is not valid JSON: {error}"
            ))
        })?;
    let input = match parse_host_function_command_input(vm, &input_schema, host_function_args) {
        Ok(input) => input,
        Err(message) => return Ok(HostFunctionCommandResolution::Failure(message)),
    };
    if let Err(message) = validate_host_function_input_schema(&input_schema, &input) {
        return Ok(HostFunctionCommandResolution::Failure(message));
    }
    let timeout_ms = host_function
        .timeout_ms
        .unwrap_or(vm.limits.host_functions.default_timeout_ms);

    Ok(build_command_callback_resolution(
        &callback_key,
        input,
        timeout_ms,
    ))
}

fn build_command_callback_resolution(
    command_name: &str,
    input: Value,
    timeout_ms: u64,
) -> HostFunctionCommandResolution {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    HostFunctionCommandResolution::Invoke {
        request: HostCallbackRequest {
            invocation_id: format!("{command_name}:{nonce}"),
            callback_key: command_name.to_owned(),
            input: input.to_string(),
            timeout_ms,
        },
        timeout: Duration::from_millis(timeout_ms),
    }
}

fn build_registry_command_input(command_name: &str, args: &[String], guest_cwd: &str) -> Value {
    json!({
        "type": "command",
        "command": command_name,
        "args": args,
        "cwd": guest_cwd,
    })
}

fn parse_host_function_command_input(
    vm: &mut VmState,
    schema: &Value,
    args: &[String],
) -> Result<Value, String> {
    match args {
        [] => Ok(Value::Object(Map::new())),
        [flag, raw] if flag == "--json" => serde_json::from_str(raw)
            .map_err(|error| format!("invalid --json host function input: {error}")),
        [flag, path] if flag == "--json-file" => {
            let bytes = vm
                .kernel
                .read_file(path)
                .map_err(|error| format!("failed to read --json-file {path}: {error}"))?;
            let raw = String::from_utf8(bytes)
                .map_err(|error| format!("invalid UTF-8 in --json-file {path}: {error}"))?;
            serde_json::from_str(&raw)
                .map_err(|error| format!("invalid JSON in --json-file {path}: {error}"))
        }
        _ => parse_host_function_command_flags(schema, args),
    }
}

fn parse_host_function_command_flags(schema: &Value, args: &[String]) -> Result<Value, String> {
    let Some(schema_object) = schema.as_object() else {
        return Ok(json!({ "args": args }));
    };
    if schema_object.get("type").and_then(Value::as_str) != Some("object") {
        return Ok(json!({ "args": args }));
    }
    let Some(properties) = schema_object.get("properties").and_then(Value::as_object) else {
        return Ok(json!({ "args": args }));
    };

    let required = schema_object
        .get("required")
        .and_then(Value::as_array)
        .map(|required| {
            required
                .iter()
                .filter_map(Value::as_str)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let flag_to_field = properties
        .iter()
        .map(|(field_name, field_schema)| (camel_to_kebab(field_name), (field_name, field_schema)))
        .collect::<BTreeMap<_, _>>();

    let mut input = Map::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        let Some(raw_flag) = arg.strip_prefix("--") else {
            return Err(format!("Unexpected positional argument: \"{arg}\""));
        };
        let (negated, flag_name) = raw_flag
            .strip_prefix("no-")
            .map_or((false, raw_flag), |name| (true, name));
        let Some((field_name, field_schema)) = flag_to_field.get(flag_name) else {
            return Err(format!("Unknown flag: --{raw_flag}"));
        };
        let field_type = json_schema_type(field_schema);

        if negated {
            if field_type != Some("boolean") {
                return Err(format!("Unknown flag: --{raw_flag}"));
            }
            input.insert((*field_name).clone(), Value::Bool(false));
            index += 1;
            continue;
        }

        if field_type == Some("boolean") {
            input.insert((*field_name).clone(), Value::Bool(true));
            index += 1;
            continue;
        }

        let Some(value) = args.get(index + 1) else {
            return Err(format!("Flag --{raw_flag} requires a value"));
        };
        let parsed_value = parse_host_function_flag_value(raw_flag, field_schema, value)?;
        if field_type == Some("array") {
            let entry = input
                .entry((*field_name).clone())
                .or_insert_with(|| Value::Array(Vec::new()));
            let Some(values) = entry.as_array_mut() else {
                return Err(format!("Flag --{raw_flag} cannot be repeated"));
            };
            values.push(parsed_value);
        } else {
            input.insert((*field_name).clone(), parsed_value);
        }
        index += 2;
    }

    for field_name in required {
        if !input.contains_key(field_name) {
            return Err(format!(
                "Missing required flag: --{}",
                camel_to_kebab(field_name)
            ));
        }
    }

    Ok(Value::Object(input))
}

fn parse_host_function_flag_value(
    raw_flag: &str,
    field_schema: &Value,
    value: &str,
) -> Result<Value, String> {
    let item_schema = field_schema
        .get("items")
        .filter(|_| json_schema_type(field_schema) == Some("array"))
        .unwrap_or(field_schema);
    match json_schema_type(item_schema) {
        Some("integer") => {
            let number = value
                .parse::<i64>()
                .map_err(|_| format!("Flag --{raw_flag} expects an integer, got \"{value}\""))?;
            Ok(Value::Number(Number::from(number)))
        }
        Some("number") => {
            let number = value
                .parse::<f64>()
                .map_err(|_| format!("Flag --{raw_flag} expects a number, got \"{value}\""))?;
            Number::from_f64(number).map(Value::Number).ok_or_else(|| {
                format!("Flag --{raw_flag} expects a finite number, got \"{value}\"")
            })
        }
        Some("boolean") => match value {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err(format!(
                "Flag --{raw_flag} expects a boolean, got \"{value}\""
            )),
        },
        _ => Ok(Value::String(value.to_owned())),
    }
}

fn json_schema_type(schema: &Value) -> Option<&str> {
    schema.get("type").and_then(Value::as_str)
}

fn camel_to_kebab(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for (index, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                output.push('-');
            }
            output.push(ch.to_ascii_lowercase());
        } else {
            output.push(ch);
        }
    }
    output
}

fn validate_host_function_input_schema(schema: &Value, input: &Value) -> Result<(), String> {
    let Some(schema_object) = schema.as_object() else {
        return Ok(());
    };
    if schema_object.get("type").and_then(Value::as_str) != Some("object") {
        return Ok(());
    }
    let Some(input_object) = input.as_object() else {
        return Err(String::from(
            "HostFunctionInputSchemaViolation at $: expected object",
        ));
    };

    if let Some(required) = schema_object.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            if !input_object.contains_key(name) {
                return Err(format!(
                    "HostFunctionInputSchemaViolation at $.{name}: missing required property"
                ));
            }
        }
    }

    let properties = schema_object
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for (name, property_schema) in &properties {
        if let Some(value) = input_object.get(name) {
            validate_host_function_input_value_type(value, property_schema, &format!("$.{name}"))?;
        }
    }
    if schema_object
        .get("additionalProperties")
        .and_then(Value::as_bool)
        == Some(false)
    {
        for name in input_object.keys() {
            if !properties.contains_key(name) {
                return Err(format!(
                    "HostFunctionInputSchemaViolation at $.{name}: unexpected property"
                ));
            }
        }
    }

    Ok(())
}

fn validate_host_function_input_value_type(
    value: &Value,
    schema: &Value,
    path: &str,
) -> Result<(), String> {
    let Some(expected) = schema.get("type").and_then(Value::as_str) else {
        return Ok(());
    };
    let matches = match expected {
        "array" => value.is_array(),
        "boolean" => value.is_boolean(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        "object" => value.is_object(),
        "string" => value.is_string(),
        _ => true,
    };
    if matches {
        Ok(())
    } else {
        Err(format!(
            "HostFunctionInputSchemaViolation at {path}: expected {expected}"
        ))
    }
}

fn command_callback_timeout_ms(vm: &VmState, kind: &HostFunctionCommand) -> u64 {
    let callbacks = match kind {
        HostFunctionCommand::Registry(command_name) => vm
            .host_functions
            .values()
            .filter(|collection| {
                collection
                    .registry_command_aliases
                    .iter()
                    .any(|alias| alias == command_name)
            })
            .flat_map(|collection| collection.callbacks.values())
            .collect::<Vec<_>>(),
        HostFunctionCommand::Collection {
            collection_name, ..
        } => vm
            .host_functions
            .get(collection_name)
            .map(|collection| collection.callbacks.values().collect::<Vec<_>>())
            .unwrap_or_default(),
    };

    callbacks
        .into_iter()
        .map(|callback| {
            callback
                .timeout_ms
                .unwrap_or(vm.limits.host_functions.default_timeout_ms)
        })
        .max()
        .unwrap_or(vm.limits.host_functions.default_timeout_ms)
}

fn ensure_collection_name_available(
    host_functions: &BTreeMap<String, RegisterHostCallbacksRequest>,
    collection_name: &str,
) -> Result<(), VmError> {
    core_ensure_collection_name_available(host_functions, collection_name)
        .map_err(host_function_registration_error)
}

fn ensure_command_aliases_available(
    host_functions: &BTreeMap<String, RegisterHostCallbacksRequest>,
    payload: &RegisterHostCallbacksRequest,
) -> Result<(), VmError> {
    core_ensure_command_aliases_available(host_functions, payload)
        .map_err(host_function_registration_error)
}

fn host_function_command_names(vm: &VmState) -> Vec<String> {
    registered_host_function_command_names(&vm.host_functions)
}

fn validate_host_functions_registration(
    payload: &RegisterHostCallbacksRequest,
) -> Result<(), VmError> {
    core_validate_host_functions_registration(payload).map_err(host_function_registration_error)
}

fn host_function_registration_error(error: HostFunctionRegistrationError) -> VmError {
    match error {
        HostFunctionRegistrationError::InvalidState(message) => VmError::InvalidState(message),
        HostFunctionRegistrationError::Conflict(message) => VmError::Conflict(message),
        HostFunctionRegistrationError::ResourceLimit {
            used,
            requested,
            limit,
            config_path,
        } => VmError::ResourceLimit(agentos_driver_tokio::accounting::LimitError {
            scope: String::from("vm"),
            resource: agentos_driver_tokio::accounting::ResourceClass::Capabilities,
            used,
            requested,
            limit,
            config_path: config_path.to_owned(),
        }),
    }
}

enum HostFunctionCommand {
    Registry(String),
    Collection { collection_name: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::RegisteredHostCallbackDefinition;
    use std::collections::BTreeMap;

    fn screenshot_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "fullPage": { "type": "boolean" },
                "width": { "type": "number" },
                "format": { "type": "string", "enum": ["png", "jpg"] },
                "tags": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["url"]
        })
    }

    fn registered_host_function(description: String) -> RegisteredHostCallbackDefinition {
        RegisteredHostCallbackDefinition {
            description,
            input_schema: screenshot_schema().to_string(),
            timeout_ms: None,
            examples: Vec::new(),
        }
    }

    fn host_functions_with_descriptions(
        collection_description: String,
        host_function_description: String,
    ) -> RegisterHostCallbacksRequest {
        host_functions_with_schema(
            String::from("browser"),
            collection_description,
            String::from("screenshot"),
            host_function_description,
            screenshot_schema(),
        )
    }

    fn host_functions_with_schema(
        collection_name: String,
        collection_description: String,
        host_function_name: String,
        host_function_description: String,
        input_schema: Value,
    ) -> RegisterHostCallbacksRequest {
        RegisterHostCallbacksRequest {
            name: collection_name.clone(),
            description: collection_description,
            command_aliases: vec![format!("agentos-{collection_name}")],
            registry_command_aliases: vec![String::from("agentos")],
            callbacks: std::collections::HashMap::from([(
                host_function_name,
                RegisteredHostCallbackDefinition {
                    description: host_function_description,
                    input_schema: input_schema.to_string(),
                    timeout_ms: None,
                    examples: Vec::new(),
                },
            )]),
        }
    }

    #[test]
    fn accepts_collection_and_host_function_descriptions_at_length_limit() {
        let description = "a".repeat(MAX_HOST_FUNCTION_DESCRIPTION_LENGTH);
        let payload = host_functions_with_descriptions(description.clone(), description);

        validate_host_functions_registration(&payload).expect("description at limit should pass");
    }

    #[test]
    fn rejects_host_function_collection_registration_over_shape_limits() {
        let too_many_host_functions = RegisterHostCallbacksRequest {
            name: String::from("browser"),
            description: String::from("Browser automation"),
            command_aliases: vec![String::from("agentos-browser")],
            registry_command_aliases: vec![String::from("agentos")],
            callbacks: (0..=MAX_HOST_FUNCTIONS_PER_COLLECTION)
                .map(|index| {
                    (
                        format!("host_function-{index}"),
                        registered_host_function(String::from("Run a bounded test host_function")),
                    )
                })
                .collect(),
        };
        assert!(
            validate_host_functions_registration(&too_many_host_functions)
                .expect_err("collection should reject too many host_functions")
                .to_string()
                .contains("limits.hostFunctions.maxFunctionsPerCollection")
        );

        let mut long_timeout = host_functions_with_descriptions(
            String::from("Browser automation"),
            String::from("Take a screenshot"),
        );
        long_timeout
            .callbacks
            .get_mut("screenshot")
            .expect("test host_function")
            .timeout_ms = Some(MAX_HOST_FUNCTION_TIMEOUT_MS + 1);
        assert!(validate_host_functions_registration(&long_timeout)
            .expect_err("collection should reject long timeouts")
            .to_string()
            .contains("limits.hostFunctions.maxTimeoutMs"));

        let mut too_many_examples = host_functions_with_descriptions(
            String::from("Browser automation"),
            String::from("Take a screenshot"),
        );
        too_many_examples
            .callbacks
            .get_mut("screenshot")
            .expect("test host_function")
            .examples = (0..=MAX_EXAMPLES_PER_HOST_FUNCTION)
            .map(|index| crate::protocol::RegisteredHostCallbackExample {
                description: format!("example {index}"),
                input: json!({ "url": "https://example.com" }).to_string(),
            })
            .collect();
        assert!(validate_host_functions_registration(&too_many_examples)
            .expect_err("collection should reject too many examples")
            .to_string()
            .contains("limits.hostFunctions.maxExamplesPerFunction"));
    }

    #[test]
    fn validates_host_callback_command_aliases() {
        let mut payload = host_functions_with_descriptions(
            String::from("Browser automation"),
            String::from("Take a screenshot"),
        );
        payload.command_aliases = vec![String::from("agentos-browser"), String::from("bad/path")];
        assert!(validate_host_functions_registration(&payload)
            .expect_err("slashes should be rejected")
            .to_string()
            .contains("invalid host callback command alias"));

        payload.command_aliases = vec![String::from("agentos-browser")];
        payload.registry_command_aliases = vec![String::from("agentos-browser")];
        assert!(validate_host_functions_registration(&payload)
            .expect_err("ambiguous aliases should be rejected")
            .to_string()
            .contains("must not also be a registry command alias"));

        payload.registry_command_aliases = vec![String::from("agentos")];
        validate_host_functions_registration(&payload).expect("distinct aliases should pass");

        let existing = BTreeMap::from([(String::from("browser"), payload.clone())]);
        let mut next = host_functions_with_schema(
            String::from("files"),
            String::from("File utilities"),
            String::from("read"),
            String::from("Read a file"),
            screenshot_schema(),
        );
        next.command_aliases = vec![String::from("agentos-browser")];
        assert!(ensure_command_aliases_available(&existing, &next)
            .expect_err("direct command aliases should be unique")
            .to_string()
            .contains("already registered"));

        next.command_aliases = vec![String::from("agentos-files")];
        next.registry_command_aliases = vec![String::from("agentos")];
        ensure_command_aliases_available(&existing, &next).expect("registry aliases can be shared");
    }

    #[test]
    fn parses_host_function_collection_command_flags_from_schema() {
        let input = parse_host_function_command_flags(
            &screenshot_schema(),
            &[
                String::from("--url"),
                String::from("https://example.com"),
                String::from("--full-page"),
                String::from("--width"),
                String::from("320"),
                String::from("--tags"),
                String::from("smoke"),
                String::from("--tags"),
                String::from("full"),
            ],
        )
        .expect("parse flags");

        assert_eq!(
            input,
            json!({
                "url": "https://example.com",
                "fullPage": true,
                "width": 320.0,
                "tags": ["smoke", "full"],
            })
        );
    }

    #[test]
    fn parse_host_function_command_flags_reports_missing_required_flags() {
        let error = parse_host_function_command_flags(&screenshot_schema(), &[])
            .expect_err("missing required flag");

        assert_eq!(error, "Missing required flag: --url");
    }

    #[test]
    fn rejects_host_function_collection_registration_with_oversized_schema_or_example_input() {
        let mut deep_schema = Value::Null;
        for _ in 0..=MAX_HOST_FUNCTION_SCHEMA_DEPTH {
            deep_schema = json!({ "items": deep_schema });
        }
        let deep_schema_payload = host_functions_with_schema(
            String::from("browser"),
            String::from("Browser automation"),
            String::from("screenshot"),
            String::from("Take a screenshot"),
            deep_schema,
        );
        assert!(validate_host_functions_registration(&deep_schema_payload)
            .expect_err("collection should reject deep schemas")
            .to_string()
            .contains("max JSON depth"));

        let mut oversized_schema_payload = host_functions_with_schema(
            String::from("browser"),
            String::from("Browser automation"),
            String::from("screenshot"),
            String::from("Take a screenshot"),
            json!({ "description": "a".repeat(MAX_HOST_FUNCTION_SCHEMA_BYTES) }),
        );
        assert!(
            validate_host_functions_registration(&oversized_schema_payload)
                .expect_err("collection should reject oversized schemas")
                .to_string()
                .contains("limits.hostFunctions.maxSchemaBytes")
        );

        oversized_schema_payload
            .callbacks
            .get_mut("screenshot")
            .expect("test host_function")
            .input_schema = screenshot_schema().to_string();
        let oversized_example_input = crate::protocol::RegisteredHostCallbackExample {
            description: String::from("large example"),
            input: json!({ "payload": "a".repeat(MAX_HOST_FUNCTION_EXAMPLE_INPUT_BYTES) })
                .to_string(),
        };
        oversized_schema_payload
            .callbacks
            .get_mut("screenshot")
            .expect("test host_function")
            .examples = vec![oversized_example_input];
        assert!(
            validate_host_functions_registration(&oversized_schema_payload)
                .expect_err("collection should reject oversized example inputs")
                .to_string()
                .contains("limits.hostFunctions.maxExampleInputBytes")
        );
    }

    #[test]
    fn rejects_collection_description_longer_than_limit() {
        let payload = host_functions_with_descriptions(
            "a".repeat(MAX_HOST_FUNCTION_DESCRIPTION_LENGTH + 1),
            String::from("Take a screenshot"),
        );

        let error =
            validate_host_functions_registration(&payload).expect_err("long collection rejected");
        assert_eq!(
            error.to_string(),
            format!(
                "Host function collection \"browser\" description is {} characters, max is {}",
                MAX_HOST_FUNCTION_DESCRIPTION_LENGTH + 1,
                MAX_HOST_FUNCTION_DESCRIPTION_LENGTH
            )
        );
    }

    #[test]
    fn rejects_host_function_description_longer_than_limit() {
        let payload = host_functions_with_descriptions(
            String::from("Browser automation"),
            "a".repeat(MAX_HOST_FUNCTION_DESCRIPTION_LENGTH + 1),
        );

        let error = validate_host_functions_registration(&payload)
            .expect_err("long host_function rejected");
        assert_eq!(
            error.to_string(),
            format!(
                "Host function \"browser/screenshot\" description is {} characters, max is {}",
                MAX_HOST_FUNCTION_DESCRIPTION_LENGTH + 1,
                MAX_HOST_FUNCTION_DESCRIPTION_LENGTH
            )
        );
    }

    #[test]
    fn host_functions_reject_duplicate_collection_registration() {
        let host_functions = BTreeMap::from([(
            String::from("browser"),
            host_functions_with_descriptions(
                String::from("Browser automation"),
                String::from("Take a screenshot"),
            ),
        )]);

        let error = ensure_collection_name_available(&host_functions, "browser")
            .expect_err("duplicate rejected");
        assert_eq!(
            error,
            VmError::Conflict(String::from(
                "host function collection already registered: browser",
            ))
        );
    }
}
