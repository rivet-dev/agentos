#![forbid(unsafe_code)]

//! Backend-agnostic sidecar logic shared by native and browser shells.

pub mod bridge_bytes;
pub mod captured_output;
pub mod command_line;
pub mod cron;
pub mod defaults;
pub mod diagnostics;
pub mod execution_defaults;
pub mod frames;
pub mod guest_fs;
pub mod guest_net;
pub mod guest_pty;
pub mod identity;
pub mod layers;
pub mod limits;
pub mod net;
pub mod permissions;
pub mod root_fs;
pub mod router;
pub mod signals;
pub mod tools;
pub mod vm_fetch;

pub use bridge_bytes::{
    bridge_buffer_value, decode_base64, decode_bridge_buffer_value, decode_encoded_bytes_value,
    encoded_bytes_value,
};
pub use captured_output::{
    validate_process_id, CaptureChunkOutcome, CapturedOutputBudget, CapturedOutputResult,
    CapturedOutputState, CAPTURED_OUTPUT_LIMIT_ERROR_CODE, CAPTURE_TERMINAL_FRAME_OVERHEAD_BYTES,
    MAX_PROCESS_ID_BYTES,
};
pub use command_line::{resolve_command_line, ResolvedCommandLine};
pub use cron::{
    decode_cron_action, CronAction, CronScheduler, CronSchedulerError, MAX_ACTIVE_CRON_RUNS,
    MAX_CRON_ACTION_BYTES, MAX_CRON_JOBS, MAX_CRON_STATE_BYTES,
};
pub use defaults::{
    default_guest_environment, guest_environment_with_overrides, DEFAULT_GUEST_PATH_ENV,
};
pub use diagnostics::{
    process_snapshot_entry_from_kernel, process_status_from_kernel,
    protocol_process_snapshot_entry, SharedProcessSnapshotEntry, SharedProcessSnapshotStatus,
};
pub use execution_defaults::apply_execute_defaults;
pub use frames::{
    authenticated_response, bound_udp_snapshot_response, event, layer_created_response,
    layer_sealed_response, listener_snapshot_response, overlay_created_response,
    package_linked_response, process_exited_event, process_exited_event_with_result,
    process_killed_response, process_output_event, process_snapshot_response,
    process_started_response, provided_commands_response, reject, respond, response_with_ownership,
    root_filesystem_bootstrapped_response, root_filesystem_snapshot_response,
    session_opened_response, signal_state_response, snapshot_exported_response,
    snapshot_imported_response, stdin_closed_response, stdin_written_response,
    unsupported_guest_kernel_call_detail, unsupported_guest_kernel_call_event,
    validate_authenticate_versions, vm_configured_response, vm_created_response,
    vm_disposed_response, vm_lifecycle_event, zombie_timer_count_response,
    AuthenticateVersionError, DispatchResult, UNSUPPORTED_GUEST_KERNEL_CALL_EVENT,
};
pub use guest_fs::{
    decode_guest_filesystem_content, empty_guest_filesystem_response,
    encode_guest_filesystem_content, guest_filesystem_stat, handle_guest_filesystem_call,
    resolve_guest_filesystem_request, targeted_guest_filesystem_response,
};
pub use guest_net::handle_guest_kernel_call;
pub use identity::{shared_guest_runtime_identity, SharedGuestRuntimeIdentity};
pub use layers::{VmLayerStore, MAX_VM_LAYERS};
pub use limits::{
    validate_vm_limits, virtual_os_cpu_count, virtual_os_freemem_bytes, virtual_os_totalmem_bytes,
    vm_limits_from_config, AcpLimits, HttpLimits, JsRuntimeLimits, PluginLimits, PythonLimits,
    ToolLimits, VmLimits, WasmLimits,
};
pub use net::{
    local_endpoint_value, remote_endpoint_value, socket_addr_family, socket_address_value,
    tcp_socket_info_value, unix_socket_info_value,
};
pub use permissions::{
    allow_all_policy, deny_all_policy, environment_permission_capability,
    evaluate_permissions_policy, filesystem_permission_capability, fs_permission_capability,
    network_permission_capability, permission_mode_to_kernel_decision, permissions_from_policy,
    permissions_with_allow_all_defaults, validate_permissions_policy,
};
pub use root_fs::{
    apply_root_filesystem_entry, build_root_filesystem, build_root_filesystem_with_loaded_snapshot,
    build_root_mount_table, build_root_mount_table_with_loaded_snapshot,
    convert_root_filesystem_entry, protocol_root_filesystem_mode,
    root_filesystem_descriptor_from_config, root_filesystem_mode_from_config,
    root_filesystem_protocol_descriptor_from_config, root_snapshot_entry,
    root_snapshot_from_entries, SidecarCoreError,
};
pub use router::{
    connection_id_of, generated_wire_blocking_extension_interrupt, request_dispatch_mode,
    request_is_unsupported_host_callback_direction, route_request_payload, session_scope_of,
    unsupported_host_callback_direction_dispatch, vm_id_of, BlockingExtensionInterrupt,
    RequestDispatchMode, RequestRoute, UNSUPPORTED_HOST_CALLBACK_DIRECTION_CODE,
    UNSUPPORTED_HOST_CALLBACK_DIRECTION_MESSAGE,
};
pub use signals::{
    apply_process_signal_state_update, canonical_signal_name, default_signal_exit_code,
    execution_signal_from_number, execution_signal_to_kernel, is_valid_posix_signal_number,
    parse_posix_signal, parse_process_signal_state_request, signal_number_from_name,
};
pub use tools::{
    ensure_toolkit_name_available, ensure_toolkit_registry_capacity, is_registry_command,
    registered_tool_command_names, toolkit_command_name, toolkit_name_for_command,
    validate_toolkit_registration, ToolRegistrationError, DEFAULT_TOOL_TIMEOUT_MS,
    MAX_REGISTERED_TOOLKITS, MAX_REGISTERED_TOOLS_PER_VM, MAX_TOOLKIT_NAME_LENGTH,
    MAX_TOOLS_PER_TOOLKIT, MAX_TOOL_DESCRIPTION_LENGTH, MAX_TOOL_EXAMPLES_PER_TOOL,
    MAX_TOOL_EXAMPLE_INPUT_BYTES, MAX_TOOL_NAME_LENGTH, MAX_TOOL_SCHEMA_BYTES,
    MAX_TOOL_SCHEMA_DEPTH, MAX_TOOL_TIMEOUT_MS,
};
pub use vm_fetch::{
    ensure_vm_fetch_raw_response_buffer_within_limit, ensure_vm_fetch_response_within_limit,
    parse_kernel_http_fetch_response, serialize_kernel_http_fetch_request,
    VM_FETCH_BUFFER_LIMIT_BYTES,
};
