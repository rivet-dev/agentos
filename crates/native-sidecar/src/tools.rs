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
    ensure_toolkit_name_available as core_ensure_toolkit_name_available,
    ensure_toolkit_registry_capacity as core_ensure_toolkit_registry_capacity, is_registry_command,
    registered_tool_command_names, toolkit_name_for_command,
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
    Output(Value),
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

    if !vm.toolkits.is_empty() && is_registry_command(command_name) {
        return Some(ToolCommand::Registry(command_name.to_owned()));
    }

    toolkit_name_for_command(&vm.toolkits, command_name).map(|toolkit_name| ToolCommand::Toolkit {
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
    _command_name: &str,
    args: &[String],
    guest_cwd: &str,
) -> Result<ToolCommandResolution, SidecarError> {
    let Some(subcommand) = args.first() else {
        return Ok(ToolCommandResolution::Output(registry_usage_payload()));
    };
    if is_help_flag(subcommand) {
        return Ok(ToolCommandResolution::Output(registry_usage_payload()));
    }
    if subcommand == "list-tools" {
        return Ok(match args.get(1) {
            Some(toolkit_name) => match describe_toolkit_payload(&vm.toolkits, toolkit_name) {
                Ok(payload) => ToolCommandResolution::Output(payload),
                Err(message) => ToolCommandResolution::Failure(message),
            },
            None => ToolCommandResolution::Output(list_toolkits_payload(&vm.toolkits)),
        });
    }

    let Some(toolkit) = vm.toolkits.get(subcommand) else {
        return Ok(ToolCommandResolution::Failure(format!(
            "No toolkit \"{subcommand}\". Available: {}",
            toolkit_names(&vm.toolkits)
        )));
    };
    let Some(tool_name) = args.get(1) else {
        return Ok(ToolCommandResolution::Output(
            describe_toolkit_payload(&vm.toolkits, subcommand)
                .expect("known toolkit should be describable"),
        ));
    };
    if is_help_flag(tool_name) {
        return Ok(ToolCommandResolution::Output(
            describe_toolkit_payload(&vm.toolkits, subcommand)
                .expect("known toolkit should be describable"),
        ));
    }
    if args.get(2).is_some_and(|value| is_help_flag(value)) {
        return Ok(match describe_tool_payload(toolkit, tool_name) {
            Ok(payload) => ToolCommandResolution::Output(payload),
            Err(message) => ToolCommandResolution::Failure(message),
        });
    }

    resolve_toolkit_command(vm, subcommand, &args[1..], guest_cwd)
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

fn is_help_flag(value: &str) -> bool {
    value == "--help" || value == "-h"
}

fn registry_usage_payload() -> Value {
    json!({
        "usage": "agentos <command>: list-tools [toolkit], <toolkit> --help, or <toolkit> <tool> ..."
    })
}

fn toolkit_names(toolkits: &BTreeMap<String, RegisterHostCallbacksRequest>) -> String {
    toolkits.keys().cloned().collect::<Vec<_>>().join(", ")
}

fn tool_names(toolkit: &RegisterHostCallbacksRequest) -> String {
    let mut names = toolkit.callbacks.keys().cloned().collect::<Vec<_>>();
    names.sort();
    names.join(", ")
}

fn list_toolkits_payload(toolkits: &BTreeMap<String, RegisterHostCallbacksRequest>) -> Value {
    json!({
        "toolkits": toolkits
            .iter()
            .map(|(name, toolkit)| {
                let mut tools = toolkit.callbacks.keys().cloned().collect::<Vec<_>>();
                tools.sort();
                json!({
                    "name": name,
                    "description": toolkit.description,
                    "tools": tools,
                })
            })
            .collect::<Vec<_>>()
    })
}

fn describe_toolkit_payload(
    toolkits: &BTreeMap<String, RegisterHostCallbacksRequest>,
    toolkit_name: &str,
) -> Result<Value, String> {
    let Some(toolkit) = toolkits.get(toolkit_name) else {
        return Err(format!(
            "No toolkit \"{toolkit_name}\". Available: {}",
            toolkit_names(toolkits)
        ));
    };
    let tools = toolkit
        .callbacks
        .iter()
        .map(|(name, tool)| {
            let schema = serde_json::from_str::<Value>(&tool.input_schema).map_err(|error| {
                format!(
                    "registered tool {toolkit_name}:{name} has an invalid input schema: {error}"
                )
            })?;
            Ok((
                name.clone(),
                json!({
                    "description": tool.description,
                    "flags": describe_tool_flags_payload(&schema),
                }),
            ))
        })
        .collect::<Result<Map<_, _>, String>>()?;
    Ok(json!({
        "name": toolkit_name,
        "description": toolkit.description,
        "tools": tools,
    }))
}

fn describe_tool_payload(
    toolkit: &RegisterHostCallbacksRequest,
    tool_name: &str,
) -> Result<Value, String> {
    let Some(tool) = toolkit.callbacks.get(tool_name) else {
        return Err(format!(
            "No tool \"{tool_name}\" in toolkit \"{}\". Available: {}",
            toolkit.name,
            tool_names(toolkit)
        ));
    };
    let schema = serde_json::from_str::<Value>(&tool.input_schema).map_err(|error| {
        format!(
            "registered tool {}:{tool_name} has an invalid input schema: {error}",
            toolkit.name
        )
    })?;
    let examples = tool
        .examples
        .iter()
        .map(|example| {
            let input = serde_json::from_str::<Value>(&example.input).map_err(|error| {
                format!(
                    "registered tool {}:{tool_name} has an invalid example input: {error}",
                    toolkit.name
                )
            })?;
            Ok(json!({
                "description": example.description,
                "input": input,
            }))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(json!({
        "toolkit": toolkit.name,
        "tool": tool_name,
        "description": tool.description,
        "flags": describe_tool_flags_payload(&schema),
        "examples": examples,
    }))
}

fn describe_tool_flags_payload(schema: &Value) -> Vec<Value> {
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    properties
        .iter()
        .map(|(field_name, field_schema)| {
            json!({
                "name": format!("--{}", camel_to_kebab(field_name)),
                "type": describe_tool_flag_type(field_schema),
                "required": required.contains(field_name.as_str()),
            })
        })
        .collect()
}

fn describe_tool_flag_type(schema: &Value) -> String {
    match json_schema_type(schema) {
        Some("array") => format!(
            "{}[]",
            schema
                .get("items")
                .and_then(json_schema_type)
                .unwrap_or("string")
        ),
        Some("string") => schema
            .get("enum")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .filter(|values| !values.is_empty())
            .map(|values| values.join("|"))
            .unwrap_or_else(|| String::from("string")),
        Some(other) => other.to_owned(),
        None => String::from("string"),
    }
}

pub(crate) fn build_host_tool_reference(
    toolkits: &BTreeMap<String, RegisterHostCallbacksRequest>,
) -> Result<String, SidecarError> {
    if toolkits.is_empty() {
        return Ok(String::new());
    }

    let mut lines = vec![
        String::from("## Available Host Tools"),
        String::new(),
        String::from("Run `agentos list-tools` to see all available tools."),
        String::new(),
    ];

    for (toolkit_name, toolkit) in toolkits {
        lines.push(format!("### {toolkit_name}"));
        lines.push(String::new());
        lines.push(toolkit.description.clone());
        lines.push(String::new());

        for (tool_name, tool) in &toolkit.callbacks {
            let schema = serde_json::from_str::<Value>(&tool.input_schema).map_err(|error| {
                SidecarError::InvalidState(format!(
                    "registered tool {toolkit_name}:{tool_name} has an invalid input schema: {error}"
                ))
            })?;
            let signature = describe_tool_flags_payload(&schema)
                .iter()
                .filter_map(|flag| {
                    let name = flag.get("name")?.as_str()?;
                    let value_type = flag.get("type")?.as_str()?;
                    let required = flag.get("required")?.as_bool()?;
                    Some(if required {
                        format!("{name} <{value_type}>")
                    } else {
                        format!("[{name} <{value_type}>]")
                    })
                })
                .collect::<Vec<_>>()
                .join(" ");
            let suffix = (!signature.is_empty())
                .then(|| format!(" {signature}"))
                .unwrap_or_default();
            lines.push(format!(
                "- `agentos-{toolkit_name} {tool_name}{suffix}` — {}",
                tool.description
            ));
        }
        lines.push(String::new());

        let tools_with_examples = toolkit
            .callbacks
            .iter()
            .filter(|(_, tool)| !tool.examples.is_empty())
            .collect::<Vec<_>>();
        if !tools_with_examples.is_empty() {
            lines.push(String::from("**Examples:**"));
            lines.push(String::new());
            for (tool_name, tool) in tools_with_examples {
                for example in &tool.examples {
                    let input = serde_json::from_str::<Value>(&example.input).map_err(|error| {
                        SidecarError::InvalidState(format!(
                            "registered tool {toolkit_name}:{tool_name} has an invalid example input: {error}"
                        ))
                    })?;
                    let arguments = tool_input_to_flags(&input);
                    let suffix = (!arguments.is_empty())
                        .then(|| format!(" {arguments}"))
                        .unwrap_or_default();
                    lines.push(format!(
                        "- {}: `agentos-{toolkit_name} {tool_name}{suffix}`",
                        example.description
                    ));
                }
            }
            lines.push(String::new());
        }

        lines.push(format!(
            "Run `agentos-{toolkit_name} <tool> --help` for details."
        ));
        lines.push(String::new());
    }

    Ok(lines.join("\n"))
}

fn tool_input_to_flags(input: &Value) -> String {
    let Some(input) = input.as_object() else {
        return String::new();
    };
    input
        .iter()
        .flat_map(|(key, value)| {
            let flag = format!("--{}", camel_to_kebab(key));
            match value {
                Value::Bool(true) => vec![flag],
                Value::Bool(false) => vec![format!("--no-{}", camel_to_kebab(key))],
                Value::Array(items) => items
                    .iter()
                    .map(|item| format!("{flag} {}", tool_cli_string(item)))
                    .collect(),
                _ => vec![format!("{flag} {}", tool_cli_string(value))],
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn tool_cli_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn ensure_toolkit_name_available(
    toolkits: &BTreeMap<String, RegisterHostCallbacksRequest>,
    toolkit_name: &str,
) -> Result<(), SidecarError> {
    core_ensure_toolkit_name_available(toolkits, toolkit_name).map_err(tool_registration_error)
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
            name: toolkit_name,
            description: toolkit_description,
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
    fn derives_host_callback_command_names() {
        let payload = toolkit_with_descriptions(
            String::from("Browser automation"),
            String::from("Take a screenshot"),
        );
        let next = toolkit_with_schema(
            String::from("files"),
            String::from("File utilities"),
            String::from("read"),
            String::from("Read a file"),
            screenshot_schema(),
        );
        let registered = BTreeMap::from([
            (String::from("browser"), payload),
            (String::from("files"), next),
        ]);
        assert_eq!(
            registered_tool_command_names(&registered),
            vec!["agentos", "agentos-browser", "agentos-files"]
        );
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
    fn registry_metadata_payloads_are_sidecar_owned() {
        let mut toolkit = toolkit_with_descriptions(
            String::from("Browser automation"),
            String::from("Take a screenshot"),
        );
        toolkit
            .callbacks
            .get_mut("screenshot")
            .expect("screenshot tool")
            .examples = vec![crate::protocol::RegisteredHostCallbackExample {
            description: String::from("Capture the home page"),
            input: json!({ "url": "https://example.com" }).to_string(),
        }];
        let toolkits = BTreeMap::from([(String::from("browser"), toolkit.clone())]);

        assert_eq!(
            list_toolkits_payload(&toolkits),
            json!({
                "toolkits": [{
                    "name": "browser",
                    "description": "Browser automation",
                    "tools": ["screenshot"],
                }]
            })
        );

        let described =
            describe_toolkit_payload(&toolkits, "browser").expect("describe registered toolkit");
        assert!(described["tools"]["screenshot"]["flags"]
            .as_array()
            .expect("tool flags")
            .iter()
            .any(|flag| flag["name"] == json!("--url") && flag["required"] == json!(true)));
        let tool = describe_tool_payload(&toolkit, "screenshot").expect("describe registered tool");
        assert_eq!(
            tool["examples"][0]["input"]["url"],
            json!("https://example.com")
        );
        let reference = build_host_tool_reference(&toolkits).expect("build tool reference");
        assert!(reference.contains("## Available Host Tools"));
        assert!(reference.contains("`agentos-browser screenshot"));
        assert!(reference.contains("--url <string>"));
        assert!(reference.contains("[--full-page <boolean>]"));
        assert!(reference.contains(
            "Capture the home page: `agentos-browser screenshot --url https://example.com`"
        ));
        assert_eq!(
            registry_usage_payload()["usage"],
            json!(
            "agentos <command>: list-tools [toolkit], <toolkit> --help, or <toolkit> <tool> ..."
        )
        );
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
