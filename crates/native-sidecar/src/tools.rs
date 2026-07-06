use crate::protocol::{
    HostCallbackRequest, HostCallbacksRegisteredResponse, RegisterHostCallbacksRequest,
    RequestFrame, ResponsePayload,
};
use crate::service::{kernel_error, normalize_path, DispatchResult};
use crate::state::{BridgeError, VmState, TOOL_DRIVER_NAME};
use crate::{NativeSidecar, NativeSidecarBridge, SidecarError};
use agentos_kernel::command_registry::CommandDriver;
use agentos_native_sidecar_core::permissions::{
    allow_all_policy, deny_all_policy, evaluate_permissions_policy,
};
use agentos_native_sidecar_core::tools::{
    ensure_command_aliases_available as core_ensure_command_aliases_available,
    ensure_toolkit_name_available as core_ensure_toolkit_name_available,
    ensure_toolkit_registry_capacity as core_ensure_toolkit_registry_capacity,
    registered_tool_command_names,
    validate_toolkit_registration as core_validate_toolkit_registration, ToolRegistrationError,
    DEFAULT_TOOL_TIMEOUT_MS,
};
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use agentos_native_sidecar_core::tools::{
    MAX_REGISTERED_TOOLKITS, MAX_REGISTERED_TOOLS_PER_VM, MAX_TOOLS_PER_TOOLKIT,
    MAX_TOOL_DESCRIPTION_LENGTH, MAX_TOOL_EXAMPLES_PER_TOOL, MAX_TOOL_EXAMPLE_INPUT_BYTES,
    MAX_TOOL_SCHEMA_BYTES, MAX_TOOL_SCHEMA_DEPTH, MAX_TOOL_TIMEOUT_MS,
};
use agentos_vm_config::PermissionMode;
use serde_json::{json, Map, Number, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub(crate) enum ToolCommandResolution {
    Invoke {
        request: HostCallbackRequest,
        timeout: Duration,
    },
    Failure(String),
}

pub(crate) fn format_tool_failure_output(message: &str) -> Vec<u8> {
    let mut output = message.as_bytes().to_vec();
    if !output.ends_with(b"\n") {
        output.push(b'\n');
    }
    output
}

pub(crate) fn register_host_callbacks<B>(
    sidecar: &mut NativeSidecar<B>,
    request: &RequestFrame,
    payload: RegisterHostCallbacksRequest,
) -> Result<DispatchResult, SidecarError>
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    let (connection_id, session_id, vm_id) = sidecar.vm_scope_for(&request.ownership)?;
    sidecar.require_owned_vm(&connection_id, &session_id, &vm_id)?;

    validate_toolkit_registration(&payload)?;

    let registered_name = payload.name.clone();
    let (original_permissions, original_toolkits, original_command_guest_paths) = {
        let vm = sidecar.vms.get(&vm_id).expect("owned VM should exist");
        (
            vm.configuration.permissions.clone(),
            vm.toolkits.clone(),
            vm.command_guest_paths.clone(),
        )
    };
    sidecar
        .bridge
        .set_vm_permissions(&vm_id, &allow_all_policy())?;
    let registration_result = (|| -> Result<_, SidecarError> {
        let vm = sidecar.vms.get_mut(&vm_id).expect("owned VM should exist");
        ensure_toolkit_name_available(&vm.toolkits, &registered_name)?;
        ensure_command_aliases_available(&vm.toolkits, &payload)?;
        ensure_toolkit_registry_capacity(&vm.toolkits, &payload)?;
        vm.toolkits.insert(registered_name.clone(), payload);
        refresh_tool_registry(vm)?;
        Ok::<_, SidecarError>(tool_command_names(vm).len() as u32)
    })();
    let command_count = match registration_result {
        Ok(result) => {
            sidecar
                .bridge
                .set_vm_permissions(&vm_id, &original_permissions)?;
            result
        }
        Err(error) => {
            let vm = sidecar.vms.get_mut(&vm_id).expect("owned VM should exist");
            vm.toolkits = original_toolkits;
            vm.command_guest_paths = original_command_guest_paths;
            match sidecar.bridge.restore_vm_permissions_fail_closed(
                &vm_id,
                &original_permissions,
                "toolkit registration rollback",
                &error,
            ) {
                Ok(()) => return Err(error),
                Err(rollback_error) => {
                    vm.configuration.permissions = deny_all_policy();
                    return Err(rollback_error);
                }
            }
        }
    };

    Ok(DispatchResult {
        response: sidecar.respond(
            request,
            ResponsePayload::HostCallbacksRegistered(HostCallbacksRegisteredResponse {
                registration: registered_name,
                command_count,
            }),
        ),
        events: Vec::new(),
    })
}

fn refresh_tool_registry(vm: &mut VmState) -> Result<(), SidecarError> {
    let commands = tool_command_names(vm);
    vm.kernel
        .register_driver(CommandDriver::new(
            TOOL_DRIVER_NAME,
            commands.iter().cloned(),
        ))
        .map_err(kernel_error)?;

    for command in commands {
        vm.command_guest_paths
            .insert(command.clone(), format!("/bin/{command}"));
    }
    Ok(())
}

pub(crate) fn resolve_tool_command(
    vm: &mut VmState,
    command: &str,
    args: &[String],
    cwd: Option<&str>,
) -> Result<Option<ToolCommandResolution>, SidecarError> {
    let Some(kind) = identify_tool_command(vm, command) else {
        return Ok(None);
    };
    let guest_cwd = cwd
        .map(normalize_path)
        .unwrap_or_else(|| vm.guest_cwd.clone());
    let resolution = match kind {
        ToolCommand::Registry(command_name) => {
            resolve_registry_command(vm, &command_name, args, &guest_cwd)?
        }
        ToolCommand::Toolkit { toolkit_name } => {
            resolve_toolkit_command(vm, &toolkit_name, args, &guest_cwd)?
        }
    };
    Ok(Some(resolution))
}

pub(crate) fn is_tool_command(vm: &VmState, command: &str) -> bool {
    identify_tool_command(vm, command).is_some()
}

pub(crate) fn normalized_tool_command_name(command: &str) -> Option<String> {
    tool_command_name_from_specifier(command).map(ToOwned::to_owned)
}

fn identify_tool_command(vm: &VmState, command: &str) -> Option<ToolCommand> {
    let command_name = tool_command_name_from_specifier(command).unwrap_or(command);

    if vm.toolkits.values().any(|toolkit| {
        toolkit
            .registry_command_aliases
            .iter()
            .any(|alias| alias == command_name)
    }) {
        return Some(ToolCommand::Registry(command_name.to_owned()));
    }

    vm.toolkits
        .iter()
        .find(|(_toolkit_name, toolkit)| {
            toolkit
                .command_aliases
                .iter()
                .any(|alias| alias == command_name)
        })
        .map(|(toolkit_name, _toolkit)| ToolCommand::Toolkit {
            toolkit_name: toolkit_name.to_owned(),
        })
}

fn tool_command_name_from_specifier(command: &str) -> Option<&str> {
    let file_name = Path::new(command).file_name()?.to_str()?;
    let normalized = normalize_path(command);
    let registered_internal_path = normalized
        .strip_prefix("/__secure_exec/commands/")
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
) -> Result<ToolCommandResolution, SidecarError> {
    let timeout_ms =
        command_callback_timeout_ms(vm, &ToolCommand::Registry(command_name.to_owned()));
    Ok(build_command_callback_resolution(
        command_name,
        build_registry_command_input(command_name, args, guest_cwd),
        timeout_ms,
    ))
}

fn resolve_toolkit_command(
    vm: &mut VmState,
    toolkit_name: &str,
    args: &[String],
    _guest_cwd: &str,
) -> Result<ToolCommandResolution, SidecarError> {
    let Some((tool_name, tool_args)) = args.split_first() else {
        return Ok(ToolCommandResolution::Failure(format!(
            "toolkit command {toolkit_name} requires a tool name"
        )));
    };
    let callback_key = format!("{toolkit_name}:{tool_name}");
    let Some(tool) = vm
        .toolkits
        .get(toolkit_name)
        .and_then(|toolkit| toolkit.callbacks.get(tool_name))
        .cloned()
    else {
        return Ok(ToolCommandResolution::Failure(format!(
            "unknown tool callback {callback_key}"
        )));
    };
    if !matches!(
        evaluate_permissions_policy(
            &vm.configuration.permissions,
            "binding",
            "binding.invoke",
            Some(&callback_key),
        ),
        PermissionMode::Allow
    ) {
        return Ok(ToolCommandResolution::Failure(format!(
            "blocked by binding.invoke policy for {callback_key}"
        )));
    }

    let input_schema: Value = serde_json::from_str(&tool.input_schema).map_err(|error| {
        SidecarError::InvalidState(format!(
            "tool {callback_key} input schema is not valid JSON: {error}"
        ))
    })?;
    let input = match parse_toolkit_command_input(vm, &input_schema, tool_args) {
        Ok(input) => input,
        Err(message) => return Ok(ToolCommandResolution::Failure(message)),
    };
    if let Err(message) = validate_tool_input_schema(&input_schema, &input) {
        return Ok(ToolCommandResolution::Failure(message));
    }
    let timeout_ms = tool.timeout_ms.unwrap_or(DEFAULT_TOOL_TIMEOUT_MS);

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
) -> ToolCommandResolution {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    ToolCommandResolution::Invoke {
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

fn parse_toolkit_command_input(
    vm: &mut VmState,
    schema: &Value,
    args: &[String],
) -> Result<Value, String> {
    match args {
        [] => Ok(Value::Object(Map::new())),
        [flag, raw] if flag == "--json" => {
            serde_json::from_str(raw).map_err(|error| format!("invalid --json tool input: {error}"))
        }
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
        _ => parse_toolkit_command_flags(schema, args),
    }
}

fn parse_toolkit_command_flags(schema: &Value, args: &[String]) -> Result<Value, String> {
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
        let parsed_value = parse_tool_flag_value(raw_flag, field_schema, value)?;
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

fn parse_tool_flag_value(
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

fn validate_tool_input_schema(schema: &Value, input: &Value) -> Result<(), String> {
    let Some(schema_object) = schema.as_object() else {
        return Ok(());
    };
    if schema_object.get("type").and_then(Value::as_str) != Some("object") {
        return Ok(());
    }
    let Some(input_object) = input.as_object() else {
        return Err(String::from(
            "ToolInputSchemaViolation at $: expected object",
        ));
    };

    if let Some(required) = schema_object.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            if !input_object.contains_key(name) {
                return Err(format!(
                    "ToolInputSchemaViolation at $.{name}: missing required property"
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
            validate_tool_input_value_type(value, property_schema, &format!("$.{name}"))?;
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
                    "ToolInputSchemaViolation at $.{name}: unexpected property"
                ));
            }
        }
    }

    Ok(())
}

fn validate_tool_input_value_type(value: &Value, schema: &Value, path: &str) -> Result<(), String> {
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
            "ToolInputSchemaViolation at {path}: expected {expected}"
        ))
    }
}

fn command_callback_timeout_ms(vm: &VmState, kind: &ToolCommand) -> u64 {
    let callbacks = match kind {
        ToolCommand::Registry(command_name) => vm
            .toolkits
            .values()
            .filter(|toolkit| {
                toolkit
                    .registry_command_aliases
                    .iter()
                    .any(|alias| alias == command_name)
            })
            .flat_map(|toolkit| toolkit.callbacks.values())
            .collect::<Vec<_>>(),
        ToolCommand::Toolkit { toolkit_name, .. } => vm
            .toolkits
            .get(toolkit_name)
            .map(|toolkit| toolkit.callbacks.values().collect::<Vec<_>>())
            .unwrap_or_default(),
    };

    callbacks
        .into_iter()
        .filter_map(|callback| callback.timeout_ms)
        .max()
        .unwrap_or(DEFAULT_TOOL_TIMEOUT_MS)
}

fn ensure_toolkit_name_available(
    toolkits: &BTreeMap<String, RegisterHostCallbacksRequest>,
    toolkit_name: &str,
) -> Result<(), SidecarError> {
    core_ensure_toolkit_name_available(toolkits, toolkit_name).map_err(tool_registration_error)
}

fn ensure_command_aliases_available(
    toolkits: &BTreeMap<String, RegisterHostCallbacksRequest>,
    payload: &RegisterHostCallbacksRequest,
) -> Result<(), SidecarError> {
    core_ensure_command_aliases_available(toolkits, payload).map_err(tool_registration_error)
}

fn ensure_toolkit_registry_capacity(
    toolkits: &BTreeMap<String, RegisterHostCallbacksRequest>,
    payload: &RegisterHostCallbacksRequest,
) -> Result<(), SidecarError> {
    core_ensure_toolkit_registry_capacity(toolkits, payload).map_err(tool_registration_error)
}

fn tool_command_names(vm: &VmState) -> Vec<String> {
    registered_tool_command_names(&vm.toolkits)
}

fn validate_toolkit_registration(
    payload: &RegisterHostCallbacksRequest,
) -> Result<(), SidecarError> {
    core_validate_toolkit_registration(payload).map_err(tool_registration_error)
}

fn tool_registration_error(error: ToolRegistrationError) -> SidecarError {
    match error {
        ToolRegistrationError::InvalidState(message) => SidecarError::InvalidState(message),
        ToolRegistrationError::Conflict(message) => SidecarError::Conflict(message),
    }
}

enum ToolCommand {
    Registry(String),
    Toolkit { toolkit_name: String },
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

    fn registered_tool(description: String) -> RegisteredHostCallbackDefinition {
        RegisteredHostCallbackDefinition {
            description,
            input_schema: screenshot_schema().to_string(),
            timeout_ms: None,
            examples: Vec::new(),
        }
    }

    fn toolkit_with_descriptions(
        toolkit_description: String,
        tool_description: String,
    ) -> RegisterHostCallbacksRequest {
        toolkit_with_schema(
            String::from("browser"),
            toolkit_description,
            String::from("screenshot"),
            tool_description,
            screenshot_schema(),
        )
    }

    fn toolkit_with_schema(
        toolkit_name: String,
        toolkit_description: String,
        tool_name: String,
        tool_description: String,
        input_schema: Value,
    ) -> RegisterHostCallbacksRequest {
        RegisterHostCallbacksRequest {
            name: toolkit_name.clone(),
            description: toolkit_description,
            command_aliases: vec![format!("agentos-{toolkit_name}")],
            registry_command_aliases: vec![String::from("agentos")],
            callbacks: std::collections::HashMap::from([(
                tool_name,
                RegisteredHostCallbackDefinition {
                    description: tool_description,
                    input_schema: input_schema.to_string(),
                    timeout_ms: None,
                    examples: Vec::new(),
                },
            )]),
        }
    }

    #[test]
    fn accepts_toolkit_and_tool_descriptions_at_length_limit() {
        let description = "a".repeat(MAX_TOOL_DESCRIPTION_LENGTH);
        let payload = toolkit_with_descriptions(description.clone(), description);

        validate_toolkit_registration(&payload).expect("description at limit should pass");
    }

    #[test]
    fn rejects_toolkit_registration_over_shape_limits() {
        let too_many_tools = RegisterHostCallbacksRequest {
            name: String::from("browser"),
            description: String::from("Browser automation"),
            command_aliases: vec![String::from("agentos-browser")],
            registry_command_aliases: vec![String::from("agentos")],
            callbacks: (0..=MAX_TOOLS_PER_TOOLKIT)
                .map(|index| {
                    (
                        format!("tool-{index}"),
                        registered_tool(String::from("Run a bounded test tool")),
                    )
                })
                .collect(),
        };
        assert!(validate_toolkit_registration(&too_many_tools)
            .expect_err("toolkit should reject too many tools")
            .to_string()
            .contains("max is 64"));

        let mut long_timeout = toolkit_with_descriptions(
            String::from("Browser automation"),
            String::from("Take a screenshot"),
        );
        long_timeout
            .callbacks
            .get_mut("screenshot")
            .expect("test tool")
            .timeout_ms = Some(MAX_TOOL_TIMEOUT_MS + 1);
        assert!(validate_toolkit_registration(&long_timeout)
            .expect_err("toolkit should reject long timeouts")
            .to_string()
            .contains("timeout is"));

        let mut too_many_examples = toolkit_with_descriptions(
            String::from("Browser automation"),
            String::from("Take a screenshot"),
        );
        too_many_examples
            .callbacks
            .get_mut("screenshot")
            .expect("test tool")
            .examples = (0..=MAX_TOOL_EXAMPLES_PER_TOOL)
            .map(|index| crate::protocol::RegisteredHostCallbackExample {
                description: format!("example {index}"),
                input: json!({ "url": "https://example.com" }).to_string(),
            })
            .collect();
        assert!(validate_toolkit_registration(&too_many_examples)
            .expect_err("toolkit should reject too many examples")
            .to_string()
            .contains("examples"));
    }

    #[test]
    fn validates_host_callback_command_aliases() {
        let mut payload = toolkit_with_descriptions(
            String::from("Browser automation"),
            String::from("Take a screenshot"),
        );
        payload.command_aliases = vec![String::from("agentos-browser"), String::from("bad/path")];
        assert!(validate_toolkit_registration(&payload)
            .expect_err("slashes should be rejected")
            .to_string()
            .contains("invalid host callback command alias"));

        payload.command_aliases = vec![String::from("agentos-browser")];
        payload.registry_command_aliases = vec![String::from("agentos-browser")];
        assert!(validate_toolkit_registration(&payload)
            .expect_err("ambiguous aliases should be rejected")
            .to_string()
            .contains("must not also be a registry command alias"));

        payload.registry_command_aliases = vec![String::from("agentos")];
        validate_toolkit_registration(&payload).expect("distinct aliases should pass");

        let existing = BTreeMap::from([(String::from("browser"), payload.clone())]);
        let mut next = toolkit_with_schema(
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
    fn parses_toolkit_command_flags_from_schema() {
        let input = parse_toolkit_command_flags(
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
    fn parse_toolkit_command_flags_reports_missing_required_flags() {
        let error = parse_toolkit_command_flags(&screenshot_schema(), &[])
            .expect_err("missing required flag");

        assert_eq!(error, "Missing required flag: --url");
    }

    #[test]
    fn rejects_toolkit_registration_with_oversized_schema_or_example_input() {
        let mut deep_schema = Value::Null;
        for _ in 0..=MAX_TOOL_SCHEMA_DEPTH {
            deep_schema = json!({ "items": deep_schema });
        }
        let deep_schema_payload = toolkit_with_schema(
            String::from("browser"),
            String::from("Browser automation"),
            String::from("screenshot"),
            String::from("Take a screenshot"),
            deep_schema,
        );
        assert!(validate_toolkit_registration(&deep_schema_payload)
            .expect_err("toolkit should reject deep schemas")
            .to_string()
            .contains("max JSON depth"));

        let mut oversized_schema_payload = toolkit_with_schema(
            String::from("browser"),
            String::from("Browser automation"),
            String::from("screenshot"),
            String::from("Take a screenshot"),
            json!({ "description": "a".repeat(MAX_TOOL_SCHEMA_BYTES) }),
        );
        assert!(validate_toolkit_registration(&oversized_schema_payload)
            .expect_err("toolkit should reject oversized schemas")
            .to_string()
            .contains("input schema is"));

        oversized_schema_payload
            .callbacks
            .get_mut("screenshot")
            .expect("test tool")
            .input_schema = screenshot_schema().to_string();
        let oversized_example_input = crate::protocol::RegisteredHostCallbackExample {
            description: String::from("large example"),
            input: json!({ "payload": "a".repeat(MAX_TOOL_EXAMPLE_INPUT_BYTES) }).to_string(),
        };
        oversized_schema_payload
            .callbacks
            .get_mut("screenshot")
            .expect("test tool")
            .examples = vec![oversized_example_input];
        assert!(validate_toolkit_registration(&oversized_schema_payload)
            .expect_err("toolkit should reject oversized example inputs")
            .to_string()
            .contains("example 0 input is"));
    }

    #[test]
    fn rejects_toolkit_description_longer_than_limit() {
        let payload = toolkit_with_descriptions(
            "a".repeat(MAX_TOOL_DESCRIPTION_LENGTH + 1),
            String::from("Take a screenshot"),
        );

        let error = validate_toolkit_registration(&payload).expect_err("long toolkit rejected");
        assert_eq!(
            error.to_string(),
            format!(
                "Toolkit \"browser\" description is {} characters, max is {}",
                MAX_TOOL_DESCRIPTION_LENGTH + 1,
                MAX_TOOL_DESCRIPTION_LENGTH
            )
        );
    }

    #[test]
    fn rejects_tool_description_longer_than_limit() {
        let payload = toolkit_with_descriptions(
            String::from("Browser automation"),
            "a".repeat(MAX_TOOL_DESCRIPTION_LENGTH + 1),
        );

        let error = validate_toolkit_registration(&payload).expect_err("long tool rejected");
        assert_eq!(
            error.to_string(),
            format!(
                "Tool \"browser/screenshot\" description is {} characters, max is {}",
                MAX_TOOL_DESCRIPTION_LENGTH + 1,
                MAX_TOOL_DESCRIPTION_LENGTH
            )
        );
    }

    #[test]
    fn tools_reject_duplicate_toolkit_registration() {
        let toolkits = BTreeMap::from([(
            String::from("browser"),
            toolkit_with_descriptions(
                String::from("Browser automation"),
                String::from("Take a screenshot"),
            ),
        )]);

        let error =
            ensure_toolkit_name_available(&toolkits, "browser").expect_err("duplicate rejected");
        assert_eq!(
            error,
            SidecarError::Conflict(String::from("toolkit already registered: browser"))
        );
    }
}
