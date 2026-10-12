// Keep error-bearing core helpers in the same crate as the included service state.
pub use agentos_vm::core::*;

#[path = "../../src/core/ca.rs"]
pub mod ca;
#[path = "../../src/core/diagnostics.rs"]
pub mod diagnostics;
// These helpers return the local root_fs error to included native converters.
#[allow(dead_code)]
#[path = "../../src/core/guest_fs.rs"]
pub mod guest_fs;
#[allow(dead_code)]
#[path = "../../src/core/guest_net.rs"]
pub mod guest_net;
#[allow(dead_code)]
#[path = "../../src/core/guest_pty.rs"]
pub mod guest_pty;
#[allow(dead_code)]
#[path = "../../src/core/layers.rs"]
pub mod layers;
#[path = "../../src/core/process_exit.rs"]
pub(crate) mod process_exit;
#[allow(dead_code)]
#[path = "../../src/core/root_fs.rs"]
pub mod root_fs;
#[allow(dead_code)]
#[path = "../../src/core/signals.rs"]
pub mod signals;
#[allow(dead_code)]
#[path = "../../src/core/vm_fetch.rs"]
pub mod vm_fetch;

#[allow(unused_imports)]
pub use diagnostics::{
    process_snapshot_entry_from_kernel, process_status_from_kernel,
    protocol_process_snapshot_entry, SharedProcessSnapshotEntry, SharedProcessSnapshotStatus,
};

// Explicit local exports shadow the external glob only for this error closure.
#[allow(unused_imports)]
pub use guest_fs::{
    decode_guest_filesystem_content, empty_guest_filesystem_response,
    encode_guest_filesystem_content, guest_filesystem_stat, handle_guest_filesystem_call,
    targeted_guest_filesystem_response,
};
pub use guest_net::handle_guest_kernel_call;
#[allow(unused_imports)]
pub use layers::{VmLayerStore, MAX_VM_LAYERS};
#[allow(unused_imports)]
pub use root_fs::{
    apply_root_filesystem_entry, build_root_filesystem, build_root_filesystem_with_loaded_snapshot,
    build_root_mount_table, build_root_mount_table_with_loaded_snapshot,
    convert_root_filesystem_entry, protocol_root_filesystem_mode,
    root_filesystem_descriptor_from_config, root_filesystem_mode_from_config,
    root_filesystem_protocol_descriptor_from_config, root_snapshot_entry,
    root_snapshot_from_entries, SidecarCoreError,
};
#[allow(unused_imports)]
pub use signals::{
    apply_process_signal_state_update, canonical_signal_name, default_signal_exit_code,
    execution_signal_from_number, execution_signal_to_kernel, is_valid_posix_signal_number,
    parse_posix_signal, parse_process_signal_state_request, signal_number_from_name,
};
#[allow(unused_imports)]
pub use vm_fetch::{
    ensure_vm_fetch_raw_response_buffer_within_limit, ensure_vm_fetch_response_within_limit,
    parse_kernel_http_fetch_response, serialize_kernel_http_fetch_request,
    VM_FETCH_BUFFER_LIMIT_BYTES,
};
