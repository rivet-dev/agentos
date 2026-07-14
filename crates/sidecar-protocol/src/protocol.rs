use crate::generated_protocol::v1 as generated_protocol;
use serde::de::{self, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::error::Error;
use std::fmt;

pub use crate::wire::{OwnershipRequirement, ProtocolCodecError, RequestDirection};

pub const PROTOCOL_NAME: &str = crate::wire::PROTOCOL_NAME;
pub const PROTOCOL_VERSION: u16 = crate::wire::PROTOCOL_VERSION;
pub const DEFAULT_MAX_FRAME_BYTES: usize = crate::wire::DEFAULT_MAX_FRAME_BYTES;
pub const DEFAULT_COMPLETED_RESPONSE_CAP: usize = 10_000;
pub type RequestId = crate::wire::RequestId;
pub type ExtEnvelope = crate::wire::ExtEnvelope;

fn serialize_bare_newtype_tag<S, T>(serializer: S, tag: u64, payload: &T) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize,
{
    let mut tuple = serializer.serialize_tuple(2)?;
    tuple.serialize_element(&serde_bare::Uint(tag))?;
    tuple.serialize_element(payload)?;
    tuple.end()
}

/// Convert the compatibility protocol frame into the generated wire frame.
pub fn to_generated_protocol_frame(
    frame: &ProtocolFrame,
) -> Result<generated_protocol::ProtocolFrame, ProtocolCodecError> {
    Ok(match frame {
        ProtocolFrame::Request(frame) => {
            generated_protocol::ProtocolFrame::RequestFrame(generated_protocol::RequestFrame {
                schema: to_generated_protocol_schema(&frame.schema),
                request_id: frame.request_id,
                ownership: to_generated_ownership_scope(&frame.ownership),
                payload: to_generated_request_payload(&frame.payload)?,
            })
        }
        ProtocolFrame::Response(frame) => {
            generated_protocol::ProtocolFrame::ResponseFrame(generated_protocol::ResponseFrame {
                schema: to_generated_protocol_schema(&frame.schema),
                request_id: frame.request_id,
                ownership: to_generated_ownership_scope(&frame.ownership),
                payload: to_generated_response_payload(&frame.payload)?,
            })
        }
        ProtocolFrame::Event(frame) => {
            generated_protocol::ProtocolFrame::EventFrame(generated_protocol::EventFrame {
                schema: to_generated_protocol_schema(&frame.schema),
                ownership: to_generated_ownership_scope(&frame.ownership),
                payload: to_generated_event_payload(&frame.payload),
            })
        }
        ProtocolFrame::SidecarRequest(frame) => {
            generated_protocol::ProtocolFrame::SidecarRequestFrame(
                generated_protocol::SidecarRequestFrame {
                    schema: to_generated_protocol_schema(&frame.schema),
                    request_id: frame.request_id,
                    ownership: to_generated_ownership_scope(&frame.ownership),
                    payload: to_generated_sidecar_request_payload(&frame.payload)?,
                },
            )
        }
        ProtocolFrame::SidecarResponse(frame) => {
            generated_protocol::ProtocolFrame::SidecarResponseFrame(
                generated_protocol::SidecarResponseFrame {
                    schema: to_generated_protocol_schema(&frame.schema),
                    request_id: frame.request_id,
                    ownership: to_generated_ownership_scope(&frame.ownership),
                    payload: to_generated_sidecar_response_payload(&frame.payload)?,
                },
            )
        }
    })
}

/// Convert the generated wire frame into the compatibility protocol frame.
pub fn from_generated_protocol_frame(
    frame: generated_protocol::ProtocolFrame,
) -> Result<ProtocolFrame, ProtocolCodecError> {
    Ok(match frame {
        generated_protocol::ProtocolFrame::RequestFrame(frame) => {
            ProtocolFrame::Request(RequestFrame {
                schema: from_generated_protocol_schema(frame.schema),
                request_id: frame.request_id,
                ownership: from_generated_ownership_scope(frame.ownership),
                payload: from_generated_request_payload(frame.payload)?,
            })
        }
        generated_protocol::ProtocolFrame::ResponseFrame(frame) => {
            ProtocolFrame::Response(ResponseFrame {
                schema: from_generated_protocol_schema(frame.schema),
                request_id: frame.request_id,
                ownership: from_generated_ownership_scope(frame.ownership),
                payload: from_generated_response_payload(frame.payload)?,
            })
        }
        generated_protocol::ProtocolFrame::EventFrame(frame) => ProtocolFrame::Event(EventFrame {
            schema: from_generated_protocol_schema(frame.schema),
            ownership: from_generated_ownership_scope(frame.ownership),
            payload: from_generated_event_payload(frame.payload),
        }),
        generated_protocol::ProtocolFrame::SidecarRequestFrame(frame) => {
            ProtocolFrame::SidecarRequest(SidecarRequestFrame {
                schema: from_generated_protocol_schema(frame.schema),
                request_id: frame.request_id,
                ownership: from_generated_ownership_scope(frame.ownership),
                payload: from_generated_sidecar_request_payload(frame.payload)?,
            })
        }
        generated_protocol::ProtocolFrame::SidecarResponseFrame(frame) => {
            ProtocolFrame::SidecarResponse(SidecarResponseFrame {
                schema: from_generated_protocol_schema(frame.schema),
                request_id: frame.request_id,
                ownership: from_generated_ownership_scope(frame.ownership),
                payload: from_generated_sidecar_response_payload(frame.payload)?,
            })
        }
    })
}

fn to_generated_protocol_schema(schema: &ProtocolSchema) -> generated_protocol::ProtocolSchema {
    schema.clone()
}

fn from_generated_protocol_schema(schema: generated_protocol::ProtocolSchema) -> ProtocolSchema {
    schema
}

// `OwnershipScope` is now an alias for the generated `crate::wire::OwnershipScope`, so the
// compat<->generated converters are identity functions. They are retained so the JSON-carrying
// frame-conversion chain (still hand-written, out of scope for this pass) keeps compiling
// without site-by-site rewiring.
pub(crate) fn to_generated_ownership_scope(
    ownership: &OwnershipScope,
) -> generated_protocol::OwnershipScope {
    ownership.clone()
}

pub(crate) fn from_generated_ownership_scope(
    ownership: generated_protocol::OwnershipScope,
) -> OwnershipScope {
    ownership
}

fn to_generated_dispose_reason(reason: &DisposeReason) -> generated_protocol::DisposeReason {
    match reason {
        DisposeReason::Requested => generated_protocol::DisposeReason::Requested,
        DisposeReason::ConnectionClosed => generated_protocol::DisposeReason::ConnectionClosed,
        DisposeReason::HostShutdown => generated_protocol::DisposeReason::HostShutdown,
    }
}

fn from_generated_dispose_reason(reason: generated_protocol::DisposeReason) -> DisposeReason {
    match reason {
        generated_protocol::DisposeReason::Requested => DisposeReason::Requested,
        generated_protocol::DisposeReason::ConnectionClosed => DisposeReason::ConnectionClosed,
        generated_protocol::DisposeReason::HostShutdown => DisposeReason::HostShutdown,
    }
}

fn to_generated_filesystem_operation(
    operation: &FilesystemOperation,
) -> generated_protocol::FilesystemOperation {
    match operation {
        FilesystemOperation::Read => generated_protocol::FilesystemOperation::Read,
        FilesystemOperation::Write => generated_protocol::FilesystemOperation::Write,
        FilesystemOperation::Stat => generated_protocol::FilesystemOperation::Stat,
        FilesystemOperation::ReadDir => generated_protocol::FilesystemOperation::ReadDir,
        FilesystemOperation::Mkdir => generated_protocol::FilesystemOperation::Mkdir,
        FilesystemOperation::Remove => generated_protocol::FilesystemOperation::Remove,
        FilesystemOperation::Rename => generated_protocol::FilesystemOperation::Rename,
    }
}

fn from_generated_filesystem_operation(
    operation: generated_protocol::FilesystemOperation,
) -> FilesystemOperation {
    match operation {
        generated_protocol::FilesystemOperation::Read => FilesystemOperation::Read,
        generated_protocol::FilesystemOperation::Write => FilesystemOperation::Write,
        generated_protocol::FilesystemOperation::Stat => FilesystemOperation::Stat,
        generated_protocol::FilesystemOperation::ReadDir => FilesystemOperation::ReadDir,
        generated_protocol::FilesystemOperation::Mkdir => FilesystemOperation::Mkdir,
        generated_protocol::FilesystemOperation::Remove => FilesystemOperation::Remove,
        generated_protocol::FilesystemOperation::Rename => FilesystemOperation::Rename,
    }
}

fn to_generated_guest_filesystem_operation(
    operation: &GuestFilesystemOperation,
) -> generated_protocol::GuestFilesystemOperation {
    match operation {
        GuestFilesystemOperation::ReadFile => {
            generated_protocol::GuestFilesystemOperation::ReadFile
        }
        GuestFilesystemOperation::WriteFile => {
            generated_protocol::GuestFilesystemOperation::WriteFile
        }
        GuestFilesystemOperation::CreateDir => {
            generated_protocol::GuestFilesystemOperation::CreateDir
        }
        GuestFilesystemOperation::Mkdir => generated_protocol::GuestFilesystemOperation::Mkdir,
        GuestFilesystemOperation::Exists => generated_protocol::GuestFilesystemOperation::Exists,
        GuestFilesystemOperation::Stat => generated_protocol::GuestFilesystemOperation::Stat,
        GuestFilesystemOperation::Lstat => generated_protocol::GuestFilesystemOperation::Lstat,
        GuestFilesystemOperation::ReadDir => generated_protocol::GuestFilesystemOperation::ReadDir,
        GuestFilesystemOperation::ReadDirRecursive => {
            generated_protocol::GuestFilesystemOperation::ReadDirRecursive
        }
        GuestFilesystemOperation::RemoveFile => {
            generated_protocol::GuestFilesystemOperation::RemoveFile
        }
        GuestFilesystemOperation::RemoveDir => {
            generated_protocol::GuestFilesystemOperation::RemoveDir
        }
        GuestFilesystemOperation::Remove => generated_protocol::GuestFilesystemOperation::Remove,
        GuestFilesystemOperation::Copy => generated_protocol::GuestFilesystemOperation::Copy,
        GuestFilesystemOperation::Move => generated_protocol::GuestFilesystemOperation::Move,
        GuestFilesystemOperation::Rename => generated_protocol::GuestFilesystemOperation::Rename,
        GuestFilesystemOperation::Realpath => {
            generated_protocol::GuestFilesystemOperation::Realpath
        }
        GuestFilesystemOperation::Symlink => generated_protocol::GuestFilesystemOperation::Symlink,
        GuestFilesystemOperation::ReadLink => {
            generated_protocol::GuestFilesystemOperation::ReadLink
        }
        GuestFilesystemOperation::Link => generated_protocol::GuestFilesystemOperation::Link,
        GuestFilesystemOperation::Chmod => generated_protocol::GuestFilesystemOperation::Chmod,
        GuestFilesystemOperation::Chown => generated_protocol::GuestFilesystemOperation::Chown,
        GuestFilesystemOperation::Utimes => generated_protocol::GuestFilesystemOperation::Utimes,
        GuestFilesystemOperation::Truncate => {
            generated_protocol::GuestFilesystemOperation::Truncate
        }
        GuestFilesystemOperation::Pread => generated_protocol::GuestFilesystemOperation::Pread,
        GuestFilesystemOperation::Pwrite => generated_protocol::GuestFilesystemOperation::Pwrite,
    }
}

fn from_generated_guest_filesystem_operation(
    operation: generated_protocol::GuestFilesystemOperation,
) -> GuestFilesystemOperation {
    match operation {
        generated_protocol::GuestFilesystemOperation::ReadFile => {
            GuestFilesystemOperation::ReadFile
        }
        generated_protocol::GuestFilesystemOperation::WriteFile => {
            GuestFilesystemOperation::WriteFile
        }
        generated_protocol::GuestFilesystemOperation::CreateDir => {
            GuestFilesystemOperation::CreateDir
        }
        generated_protocol::GuestFilesystemOperation::Mkdir => GuestFilesystemOperation::Mkdir,
        generated_protocol::GuestFilesystemOperation::Exists => GuestFilesystemOperation::Exists,
        generated_protocol::GuestFilesystemOperation::Stat => GuestFilesystemOperation::Stat,
        generated_protocol::GuestFilesystemOperation::Lstat => GuestFilesystemOperation::Lstat,
        generated_protocol::GuestFilesystemOperation::ReadDir => GuestFilesystemOperation::ReadDir,
        generated_protocol::GuestFilesystemOperation::ReadDirRecursive => {
            GuestFilesystemOperation::ReadDirRecursive
        }
        generated_protocol::GuestFilesystemOperation::RemoveFile => {
            GuestFilesystemOperation::RemoveFile
        }
        generated_protocol::GuestFilesystemOperation::RemoveDir => {
            GuestFilesystemOperation::RemoveDir
        }
        generated_protocol::GuestFilesystemOperation::Remove => GuestFilesystemOperation::Remove,
        generated_protocol::GuestFilesystemOperation::Copy => GuestFilesystemOperation::Copy,
        generated_protocol::GuestFilesystemOperation::Move => GuestFilesystemOperation::Move,
        generated_protocol::GuestFilesystemOperation::Rename => GuestFilesystemOperation::Rename,
        generated_protocol::GuestFilesystemOperation::Realpath => {
            GuestFilesystemOperation::Realpath
        }
        generated_protocol::GuestFilesystemOperation::Symlink => GuestFilesystemOperation::Symlink,
        generated_protocol::GuestFilesystemOperation::ReadLink => {
            GuestFilesystemOperation::ReadLink
        }
        generated_protocol::GuestFilesystemOperation::Link => GuestFilesystemOperation::Link,
        generated_protocol::GuestFilesystemOperation::Chmod => GuestFilesystemOperation::Chmod,
        generated_protocol::GuestFilesystemOperation::Chown => GuestFilesystemOperation::Chown,
        generated_protocol::GuestFilesystemOperation::Utimes => GuestFilesystemOperation::Utimes,
        generated_protocol::GuestFilesystemOperation::Truncate => {
            GuestFilesystemOperation::Truncate
        }
        generated_protocol::GuestFilesystemOperation::Pread => GuestFilesystemOperation::Pread,
        generated_protocol::GuestFilesystemOperation::Pwrite => GuestFilesystemOperation::Pwrite,
    }
}

fn to_generated_permission_mode(mode: &PermissionMode) -> generated_protocol::PermissionMode {
    mode.clone()
}

fn from_generated_permission_mode(mode: generated_protocol::PermissionMode) -> PermissionMode {
    mode
}

fn to_generated_root_filesystem_entry_encoding(
    encoding: &RootFilesystemEntryEncoding,
) -> generated_protocol::RootFilesystemEntryEncoding {
    match encoding {
        RootFilesystemEntryEncoding::Utf8 => generated_protocol::RootFilesystemEntryEncoding::Utf8,
        RootFilesystemEntryEncoding::Base64 => {
            generated_protocol::RootFilesystemEntryEncoding::Base64
        }
    }
}

fn from_generated_root_filesystem_entry_encoding(
    encoding: generated_protocol::RootFilesystemEntryEncoding,
) -> RootFilesystemEntryEncoding {
    match encoding {
        generated_protocol::RootFilesystemEntryEncoding::Utf8 => RootFilesystemEntryEncoding::Utf8,
        generated_protocol::RootFilesystemEntryEncoding::Base64 => {
            RootFilesystemEntryEncoding::Base64
        }
    }
}

fn to_generated_stream_channel(channel: &StreamChannel) -> generated_protocol::StreamChannel {
    match channel {
        StreamChannel::Stdout => generated_protocol::StreamChannel::Stdout,
        StreamChannel::Stderr => generated_protocol::StreamChannel::Stderr,
    }
}

fn from_generated_stream_channel(channel: generated_protocol::StreamChannel) -> StreamChannel {
    match channel {
        generated_protocol::StreamChannel::Stdout => StreamChannel::Stdout,
        generated_protocol::StreamChannel::Stderr => StreamChannel::Stderr,
    }
}

fn to_generated_vm_lifecycle_state(
    state: &VmLifecycleState,
) -> generated_protocol::VmLifecycleState {
    match state {
        VmLifecycleState::Creating => generated_protocol::VmLifecycleState::Creating,
        VmLifecycleState::Ready => generated_protocol::VmLifecycleState::Ready,
        VmLifecycleState::Disposing => generated_protocol::VmLifecycleState::Disposing,
        VmLifecycleState::Disposed => generated_protocol::VmLifecycleState::Disposed,
        VmLifecycleState::Failed => generated_protocol::VmLifecycleState::Failed,
    }
}

fn from_generated_vm_lifecycle_state(
    state: generated_protocol::VmLifecycleState,
) -> VmLifecycleState {
    match state {
        generated_protocol::VmLifecycleState::Creating => VmLifecycleState::Creating,
        generated_protocol::VmLifecycleState::Ready => VmLifecycleState::Ready,
        generated_protocol::VmLifecycleState::Disposing => VmLifecycleState::Disposing,
        generated_protocol::VmLifecycleState::Disposed => VmLifecycleState::Disposed,
        generated_protocol::VmLifecycleState::Failed => VmLifecycleState::Failed,
    }
}

fn to_generated_guest_filesystem_stat(
    stat: &GuestFilesystemStat,
) -> generated_protocol::GuestFilesystemStat {
    stat.clone()
}

fn from_generated_guest_filesystem_stat(
    stat: generated_protocol::GuestFilesystemStat,
) -> GuestFilesystemStat {
    stat
}

fn to_generated_process_snapshot_entry(
    entry: &ProcessSnapshotEntry,
) -> generated_protocol::ProcessSnapshotEntry {
    entry.clone()
}

fn from_generated_process_snapshot_entry(
    entry: generated_protocol::ProcessSnapshotEntry,
) -> ProcessSnapshotEntry {
    entry
}

fn to_generated_ext_envelope(envelope: &ExtEnvelope) -> generated_protocol::ExtEnvelope {
    envelope.clone()
}

fn from_generated_ext_envelope(envelope: generated_protocol::ExtEnvelope) -> ExtEnvelope {
    envelope
}

fn to_generated_request_payload(
    payload: &RequestPayload,
) -> Result<generated_protocol::RequestPayload, ProtocolCodecError> {
    Ok(match payload {
        RequestPayload::Authenticate(inner) => {
            generated_protocol::RequestPayload::AuthenticateRequest(inner.clone())
        }
        RequestPayload::OpenSession(inner) => {
            generated_protocol::RequestPayload::OpenSessionRequest(inner.clone())
        }
        RequestPayload::CloseSession(inner) => {
            generated_protocol::RequestPayload::CloseSessionRequest(inner.clone())
        }
        RequestPayload::CreateVm(inner) => {
            generated_protocol::RequestPayload::CreateVmRequest(inner.clone())
        }
        RequestPayload::DisposeVm(inner) => generated_protocol::RequestPayload::DisposeVmRequest(
            generated_protocol::DisposeVmRequest {
                reason: to_generated_dispose_reason(&inner.reason),
            },
        ),
        RequestPayload::BootstrapRootFilesystem(inner) => {
            generated_protocol::RequestPayload::BootstrapRootFilesystemRequest(inner.clone())
        }
        RequestPayload::ConfigureVm(inner) => {
            generated_protocol::RequestPayload::ConfigureVmRequest(inner.clone())
        }
        RequestPayload::RegisterHostCallbacks(inner) => {
            generated_protocol::RequestPayload::RegisterHostCallbacksRequest(inner.clone())
        }
        RequestPayload::CreateLayer(_) => generated_protocol::RequestPayload::CreateLayerRequest,
        RequestPayload::SealLayer(inner) => {
            generated_protocol::RequestPayload::SealLayerRequest(inner.clone())
        }
        RequestPayload::ImportSnapshot(inner) => {
            generated_protocol::RequestPayload::ImportSnapshotRequest(inner.clone())
        }
        RequestPayload::ExportSnapshot(inner) => {
            generated_protocol::RequestPayload::ExportSnapshotRequest(inner.clone())
        }
        RequestPayload::CreateOverlay(inner) => {
            generated_protocol::RequestPayload::CreateOverlayRequest(inner.clone())
        }
        RequestPayload::GuestFilesystemCall(inner) => {
            generated_protocol::RequestPayload::GuestFilesystemCallRequest(inner.clone())
        }
        RequestPayload::SnapshotRootFilesystem(_) => {
            generated_protocol::RequestPayload::SnapshotRootFilesystemRequest
        }
        RequestPayload::Execute(inner) => {
            generated_protocol::RequestPayload::ExecuteRequest(inner.clone())
        }
        RequestPayload::WriteStdin(inner) => {
            generated_protocol::RequestPayload::WriteStdinRequest(inner.clone())
        }
        RequestPayload::CloseStdin(inner) => {
            generated_protocol::RequestPayload::CloseStdinRequest(inner.clone())
        }
        RequestPayload::KillProcess(inner) => {
            generated_protocol::RequestPayload::KillProcessRequest(inner.clone())
        }
        RequestPayload::GetProcessSnapshot(_) => {
            generated_protocol::RequestPayload::GetProcessSnapshotRequest
        }
        RequestPayload::GetResourceSnapshot(_) => {
            generated_protocol::RequestPayload::GetResourceSnapshotRequest
        }
        RequestPayload::FindListener(inner) => {
            generated_protocol::RequestPayload::FindListenerRequest(inner.clone())
        }
        RequestPayload::FindBoundUdp(inner) => {
            generated_protocol::RequestPayload::FindBoundUdpRequest(inner.clone())
        }
        RequestPayload::VmFetch(inner) => {
            generated_protocol::RequestPayload::VmFetchRequest(inner.clone())
        }
        RequestPayload::GetSignalState(inner) => {
            generated_protocol::RequestPayload::GetSignalStateRequest(inner.clone())
        }
        RequestPayload::GetZombieTimerCount(_) => {
            generated_protocol::RequestPayload::GetZombieTimerCountRequest
        }
        RequestPayload::HostFilesystemCall(inner) => {
            generated_protocol::RequestPayload::HostFilesystemCallRequest(
                generated_protocol::HostFilesystemCallRequest {
                    operation: to_generated_filesystem_operation(&inner.operation),
                    path: inner.path.clone(),
                    payload_size_bytes: inner.payload_size_bytes,
                },
            )
        }
        RequestPayload::PersistenceLoad(inner) => {
            generated_protocol::RequestPayload::PersistenceLoadRequest(inner.clone())
        }
        RequestPayload::PersistenceFlush(inner) => {
            generated_protocol::RequestPayload::PersistenceFlushRequest(inner.clone())
        }
        RequestPayload::Ext(inner) => {
            generated_protocol::RequestPayload::ExtEnvelope(to_generated_ext_envelope(inner))
        }
        RequestPayload::GuestKernelCall(inner) => {
            generated_protocol::RequestPayload::GuestKernelCallRequest(inner.clone())
        }
        RequestPayload::ResizePty(inner) => {
            generated_protocol::RequestPayload::ResizePtyRequest(inner.clone())
        }
        RequestPayload::LinkPackage(inner) => {
            generated_protocol::RequestPayload::LinkPackageRequest(inner.clone())
        }
        RequestPayload::ProvidedCommands(_) => {
            generated_protocol::RequestPayload::ProvidedCommandsRequest
        }
        RequestPayload::ScheduleCron(inner) => {
            generated_protocol::RequestPayload::ScheduleCronRequest(inner.clone())
        }
        RequestPayload::ListCronJobs(_) => generated_protocol::RequestPayload::ListCronJobsRequest,
        RequestPayload::CancelCronJob(inner) => {
            generated_protocol::RequestPayload::CancelCronJobRequest(inner.clone())
        }
        RequestPayload::WakeCron(inner) => {
            generated_protocol::RequestPayload::WakeCronRequest(inner.clone())
        }
        RequestPayload::CompleteCronRun(inner) => {
            generated_protocol::RequestPayload::CompleteCronRunRequest(inner.clone())
        }
        RequestPayload::ExportCronState(_) => {
            generated_protocol::RequestPayload::ExportCronStateRequest
        }
        RequestPayload::ImportCronState(inner) => {
            generated_protocol::RequestPayload::ImportCronStateRequest(inner.clone())
        }
        RequestPayload::InitializeVm(inner) => {
            generated_protocol::RequestPayload::InitializeVmRequest(inner.clone())
        }
    })
}

fn from_generated_request_payload(
    payload: generated_protocol::RequestPayload,
) -> Result<RequestPayload, ProtocolCodecError> {
    Ok(match payload {
        generated_protocol::RequestPayload::AuthenticateRequest(inner) => {
            RequestPayload::Authenticate(inner)
        }
        generated_protocol::RequestPayload::OpenSessionRequest(inner) => {
            RequestPayload::OpenSession(inner)
        }
        generated_protocol::RequestPayload::CloseSessionRequest(inner) => {
            RequestPayload::CloseSession(inner)
        }
        generated_protocol::RequestPayload::CreateVmRequest(inner) => {
            RequestPayload::CreateVm(inner)
        }
        generated_protocol::RequestPayload::DisposeVmRequest(inner) => {
            RequestPayload::DisposeVm(DisposeVmRequest {
                reason: from_generated_dispose_reason(inner.reason),
            })
        }
        generated_protocol::RequestPayload::BootstrapRootFilesystemRequest(inner) => {
            RequestPayload::BootstrapRootFilesystem(inner)
        }
        generated_protocol::RequestPayload::ConfigureVmRequest(inner) => {
            RequestPayload::ConfigureVm(inner)
        }
        generated_protocol::RequestPayload::RegisterHostCallbacksRequest(inner) => {
            RequestPayload::RegisterHostCallbacks(inner)
        }
        generated_protocol::RequestPayload::CreateLayerRequest => {
            RequestPayload::CreateLayer(CreateLayerRequest {})
        }
        generated_protocol::RequestPayload::SealLayerRequest(inner) => {
            RequestPayload::SealLayer(inner)
        }
        generated_protocol::RequestPayload::ImportSnapshotRequest(inner) => {
            RequestPayload::ImportSnapshot(inner)
        }
        generated_protocol::RequestPayload::ExportSnapshotRequest(inner) => {
            RequestPayload::ExportSnapshot(inner)
        }
        generated_protocol::RequestPayload::CreateOverlayRequest(inner) => {
            RequestPayload::CreateOverlay(inner)
        }
        generated_protocol::RequestPayload::GuestFilesystemCallRequest(inner) => {
            RequestPayload::GuestFilesystemCall(inner)
        }
        generated_protocol::RequestPayload::SnapshotRootFilesystemRequest => {
            RequestPayload::SnapshotRootFilesystem(SnapshotRootFilesystemRequest {})
        }
        generated_protocol::RequestPayload::ExecuteRequest(inner) => RequestPayload::Execute(inner),
        generated_protocol::RequestPayload::WriteStdinRequest(inner) => {
            RequestPayload::WriteStdin(inner)
        }
        generated_protocol::RequestPayload::CloseStdinRequest(inner) => {
            RequestPayload::CloseStdin(inner)
        }
        generated_protocol::RequestPayload::KillProcessRequest(inner) => {
            RequestPayload::KillProcess(inner)
        }
        generated_protocol::RequestPayload::GetProcessSnapshotRequest => {
            RequestPayload::GetProcessSnapshot(GetProcessSnapshotRequest {})
        }
        generated_protocol::RequestPayload::GetResourceSnapshotRequest => {
            RequestPayload::GetResourceSnapshot(GetResourceSnapshotRequest {})
        }
        generated_protocol::RequestPayload::FindListenerRequest(inner) => {
            RequestPayload::FindListener(inner)
        }
        generated_protocol::RequestPayload::FindBoundUdpRequest(inner) => {
            RequestPayload::FindBoundUdp(inner)
        }
        generated_protocol::RequestPayload::VmFetchRequest(inner) => RequestPayload::VmFetch(inner),
        generated_protocol::RequestPayload::GetSignalStateRequest(inner) => {
            RequestPayload::GetSignalState(inner)
        }
        generated_protocol::RequestPayload::GetZombieTimerCountRequest => {
            RequestPayload::GetZombieTimerCount(GetZombieTimerCountRequest {})
        }
        generated_protocol::RequestPayload::HostFilesystemCallRequest(inner) => {
            RequestPayload::HostFilesystemCall(HostFilesystemCallRequest {
                operation: from_generated_filesystem_operation(inner.operation),
                path: inner.path,
                payload_size_bytes: inner.payload_size_bytes,
            })
        }
        generated_protocol::RequestPayload::PersistenceLoadRequest(inner) => {
            RequestPayload::PersistenceLoad(inner)
        }
        generated_protocol::RequestPayload::PersistenceFlushRequest(inner) => {
            RequestPayload::PersistenceFlush(inner)
        }
        generated_protocol::RequestPayload::ExtEnvelope(inner) => {
            RequestPayload::Ext(from_generated_ext_envelope(inner))
        }
        generated_protocol::RequestPayload::GuestKernelCallRequest(inner) => {
            RequestPayload::GuestKernelCall(inner)
        }
        generated_protocol::RequestPayload::ResizePtyRequest(inner) => {
            RequestPayload::ResizePty(inner)
        }
        generated_protocol::RequestPayload::LinkPackageRequest(inner) => {
            RequestPayload::LinkPackage(inner)
        }
        generated_protocol::RequestPayload::ProvidedCommandsRequest => {
            RequestPayload::ProvidedCommands(ProvidedCommandsRequest {})
        }
        generated_protocol::RequestPayload::ScheduleCronRequest(inner) => {
            RequestPayload::ScheduleCron(inner)
        }
        generated_protocol::RequestPayload::ListCronJobsRequest => {
            RequestPayload::ListCronJobs(ListCronJobsRequest {})
        }
        generated_protocol::RequestPayload::CancelCronJobRequest(inner) => {
            RequestPayload::CancelCronJob(inner)
        }
        generated_protocol::RequestPayload::WakeCronRequest(inner) => {
            RequestPayload::WakeCron(inner)
        }
        generated_protocol::RequestPayload::CompleteCronRunRequest(inner) => {
            RequestPayload::CompleteCronRun(inner)
        }
        generated_protocol::RequestPayload::ExportCronStateRequest => {
            RequestPayload::ExportCronState(ExportCronStateRequest {})
        }
        generated_protocol::RequestPayload::ImportCronStateRequest(inner) => {
            RequestPayload::ImportCronState(inner)
        }
        generated_protocol::RequestPayload::InitializeVmRequest(inner) => {
            RequestPayload::InitializeVm(inner)
        }
    })
}

fn to_generated_response_payload(
    payload: &ResponsePayload,
) -> Result<generated_protocol::ResponsePayload, ProtocolCodecError> {
    Ok(match payload {
        ResponsePayload::Authenticated(inner) => {
            generated_protocol::ResponsePayload::AuthenticatedResponse(inner.clone())
        }
        ResponsePayload::SessionOpened(inner) => {
            generated_protocol::ResponsePayload::SessionOpenedResponse(inner.clone())
        }
        ResponsePayload::SessionClosed(inner) => {
            generated_protocol::ResponsePayload::SessionClosedResponse(inner.clone())
        }
        ResponsePayload::VmCreated(inner) => {
            generated_protocol::ResponsePayload::VmCreatedResponse(inner.clone())
        }
        ResponsePayload::VmDisposed(inner) => {
            generated_protocol::ResponsePayload::VmDisposedResponse(inner.clone())
        }
        ResponsePayload::RootFilesystemBootstrapped(inner) => {
            generated_protocol::ResponsePayload::RootFilesystemBootstrappedResponse(inner.clone())
        }
        ResponsePayload::VmConfigured(inner) => {
            generated_protocol::ResponsePayload::VmConfiguredResponse(inner.clone())
        }
        ResponsePayload::HostCallbacksRegistered(inner) => {
            generated_protocol::ResponsePayload::HostCallbacksRegisteredResponse(inner.clone())
        }
        ResponsePayload::LayerCreated(inner) => {
            generated_protocol::ResponsePayload::LayerCreatedResponse(inner.clone())
        }
        ResponsePayload::LayerSealed(inner) => {
            generated_protocol::ResponsePayload::LayerSealedResponse(inner.clone())
        }
        ResponsePayload::SnapshotImported(inner) => {
            generated_protocol::ResponsePayload::SnapshotImportedResponse(inner.clone())
        }
        ResponsePayload::SnapshotExported(inner) => {
            generated_protocol::ResponsePayload::SnapshotExportedResponse(
                generated_protocol::SnapshotExportedResponse {
                    layer_id: inner.layer_id.clone(),
                    entries: inner.entries.clone(),
                },
            )
        }
        ResponsePayload::OverlayCreated(inner) => {
            generated_protocol::ResponsePayload::OverlayCreatedResponse(inner.clone())
        }
        ResponsePayload::GuestFilesystemResult(inner) => {
            generated_protocol::ResponsePayload::GuestFilesystemResultResponse(
                generated_protocol::GuestFilesystemResultResponse {
                    operation: to_generated_guest_filesystem_operation(&inner.operation),
                    path: inner.path.clone(),
                    content: inner.content.clone(),
                    encoding: inner
                        .encoding
                        .as_ref()
                        .map(to_generated_root_filesystem_entry_encoding),
                    entries: inner.entries.clone(),
                    stat: inner.stat.as_ref().map(to_generated_guest_filesystem_stat),
                    exists: inner.exists,
                    target: inner.target.clone(),
                },
            )
        }
        ResponsePayload::RootFilesystemSnapshot(inner) => {
            generated_protocol::ResponsePayload::RootFilesystemSnapshotResponse(
                generated_protocol::RootFilesystemSnapshotResponse {
                    entries: inner.entries.clone(),
                },
            )
        }
        ResponsePayload::ProcessStarted(inner) => {
            generated_protocol::ResponsePayload::ProcessStartedResponse(inner.clone())
        }
        ResponsePayload::StdinWritten(inner) => {
            generated_protocol::ResponsePayload::StdinWrittenResponse(inner.clone())
        }
        ResponsePayload::StdinClosed(inner) => {
            generated_protocol::ResponsePayload::StdinClosedResponse(inner.clone())
        }
        ResponsePayload::ProcessKilled(inner) => {
            generated_protocol::ResponsePayload::ProcessKilledResponse(inner.clone())
        }
        ResponsePayload::ProcessSnapshot(inner) => {
            generated_protocol::ResponsePayload::ProcessSnapshotResponse(
                generated_protocol::ProcessSnapshotResponse {
                    processes: inner
                        .processes
                        .iter()
                        .map(to_generated_process_snapshot_entry)
                        .collect(),
                },
            )
        }
        ResponsePayload::ResourceSnapshot(inner) => {
            generated_protocol::ResponsePayload::ResourceSnapshotResponse(inner.clone())
        }
        ResponsePayload::ListenerSnapshot(inner) => {
            generated_protocol::ResponsePayload::ListenerSnapshotResponse(inner.clone())
        }
        ResponsePayload::BoundUdpSnapshot(inner) => {
            generated_protocol::ResponsePayload::BoundUdpSnapshotResponse(inner.clone())
        }
        ResponsePayload::VmFetchResult(inner) => {
            generated_protocol::ResponsePayload::VmFetchResponse(inner.clone())
        }
        ResponsePayload::SignalState(inner) => {
            generated_protocol::ResponsePayload::SignalStateResponse(inner.clone())
        }
        ResponsePayload::ZombieTimerCount(inner) => {
            generated_protocol::ResponsePayload::ZombieTimerCountResponse(inner.clone())
        }
        ResponsePayload::FilesystemResult(inner) => {
            generated_protocol::ResponsePayload::FilesystemResultResponse(
                generated_protocol::FilesystemResultResponse {
                    operation: to_generated_filesystem_operation(&inner.operation),
                    status: inner.status.clone(),
                    payload_size_bytes: inner.payload_size_bytes,
                },
            )
        }
        ResponsePayload::PermissionDecision(inner) => {
            generated_protocol::ResponsePayload::PermissionDecisionResponse(
                generated_protocol::PermissionDecisionResponse {
                    capability: inner.capability.clone(),
                    decision: to_generated_permission_mode(&inner.decision),
                },
            )
        }
        ResponsePayload::PersistenceState(inner) => {
            generated_protocol::ResponsePayload::PersistenceStateResponse(inner.clone())
        }
        ResponsePayload::PersistenceFlushed(inner) => {
            generated_protocol::ResponsePayload::PersistenceFlushedResponse(inner.clone())
        }
        ResponsePayload::Rejected(inner) => {
            generated_protocol::ResponsePayload::RejectedResponse(inner.clone())
        }
        ResponsePayload::ExtResult(inner) => {
            generated_protocol::ResponsePayload::ExtEnvelope(to_generated_ext_envelope(inner))
        }
        ResponsePayload::GuestKernelResult(inner) => {
            generated_protocol::ResponsePayload::GuestKernelResultResponse(inner.clone())
        }
        ResponsePayload::PtyResized(inner) => {
            generated_protocol::ResponsePayload::PtyResizedResponse(inner.clone())
        }
        ResponsePayload::PackageLinked(inner) => {
            generated_protocol::ResponsePayload::PackageLinkedResponse(inner.clone())
        }
        ResponsePayload::ProvidedCommands(inner) => {
            generated_protocol::ResponsePayload::ProvidedCommandsResponse(inner.clone())
        }
        ResponsePayload::CronScheduled(inner) => {
            generated_protocol::ResponsePayload::CronScheduledResponse(inner.clone())
        }
        ResponsePayload::CronJobs(inner) => {
            generated_protocol::ResponsePayload::CronJobsResponse(inner.clone())
        }
        ResponsePayload::CronCancelled(inner) => {
            generated_protocol::ResponsePayload::CronCancelledResponse(inner.clone())
        }
        ResponsePayload::CronWake(inner) => {
            generated_protocol::ResponsePayload::CronWakeResponse(inner.clone())
        }
        ResponsePayload::CronRunCompleted(inner) => {
            generated_protocol::ResponsePayload::CronRunCompletedResponse(inner.clone())
        }
        ResponsePayload::CronStateExported(inner) => {
            generated_protocol::ResponsePayload::CronStateExportedResponse(inner.clone())
        }
        ResponsePayload::CronStateImported(inner) => {
            generated_protocol::ResponsePayload::CronStateImportedResponse(inner.clone())
        }
        ResponsePayload::VmInitialized(inner) => {
            generated_protocol::ResponsePayload::VmInitializedResponse(inner.clone())
        }
    })
}

fn from_generated_response_payload(
    payload: generated_protocol::ResponsePayload,
) -> Result<ResponsePayload, ProtocolCodecError> {
    Ok(match payload {
        generated_protocol::ResponsePayload::AuthenticatedResponse(inner) => {
            ResponsePayload::Authenticated(inner)
        }
        generated_protocol::ResponsePayload::SessionOpenedResponse(inner) => {
            ResponsePayload::SessionOpened(inner)
        }
        generated_protocol::ResponsePayload::SessionClosedResponse(inner) => {
            ResponsePayload::SessionClosed(inner)
        }
        generated_protocol::ResponsePayload::VmCreatedResponse(inner) => {
            ResponsePayload::VmCreated(inner)
        }
        generated_protocol::ResponsePayload::VmDisposedResponse(inner) => {
            ResponsePayload::VmDisposed(inner)
        }
        generated_protocol::ResponsePayload::RootFilesystemBootstrappedResponse(inner) => {
            ResponsePayload::RootFilesystemBootstrapped(inner)
        }
        generated_protocol::ResponsePayload::VmConfiguredResponse(inner) => {
            ResponsePayload::VmConfigured(inner)
        }
        generated_protocol::ResponsePayload::HostCallbacksRegisteredResponse(inner) => {
            ResponsePayload::HostCallbacksRegistered(inner)
        }
        generated_protocol::ResponsePayload::LayerCreatedResponse(inner) => {
            ResponsePayload::LayerCreated(inner)
        }
        generated_protocol::ResponsePayload::LayerSealedResponse(inner) => {
            ResponsePayload::LayerSealed(inner)
        }
        generated_protocol::ResponsePayload::SnapshotImportedResponse(inner) => {
            ResponsePayload::SnapshotImported(inner)
        }
        generated_protocol::ResponsePayload::SnapshotExportedResponse(inner) => {
            ResponsePayload::SnapshotExported(SnapshotExportedResponse {
                layer_id: inner.layer_id,
                entries: inner.entries,
            })
        }
        generated_protocol::ResponsePayload::OverlayCreatedResponse(inner) => {
            ResponsePayload::OverlayCreated(inner)
        }
        generated_protocol::ResponsePayload::GuestFilesystemResultResponse(inner) => {
            ResponsePayload::GuestFilesystemResult(GuestFilesystemResultResponse {
                operation: from_generated_guest_filesystem_operation(inner.operation),
                path: inner.path,
                content: inner.content,
                encoding: inner
                    .encoding
                    .map(from_generated_root_filesystem_entry_encoding),
                entries: inner.entries,
                stat: inner.stat.map(from_generated_guest_filesystem_stat),
                exists: inner.exists,
                target: inner.target,
            })
        }
        generated_protocol::ResponsePayload::RootFilesystemSnapshotResponse(inner) => {
            ResponsePayload::RootFilesystemSnapshot(RootFilesystemSnapshotResponse {
                entries: inner.entries,
            })
        }
        generated_protocol::ResponsePayload::ProcessStartedResponse(inner) => {
            ResponsePayload::ProcessStarted(inner)
        }
        generated_protocol::ResponsePayload::StdinWrittenResponse(inner) => {
            ResponsePayload::StdinWritten(inner)
        }
        generated_protocol::ResponsePayload::StdinClosedResponse(inner) => {
            ResponsePayload::StdinClosed(inner)
        }
        generated_protocol::ResponsePayload::ProcessKilledResponse(inner) => {
            ResponsePayload::ProcessKilled(inner)
        }
        generated_protocol::ResponsePayload::ProcessSnapshotResponse(inner) => {
            ResponsePayload::ProcessSnapshot(ProcessSnapshotResponse {
                processes: inner
                    .processes
                    .into_iter()
                    .map(from_generated_process_snapshot_entry)
                    .collect(),
            })
        }
        generated_protocol::ResponsePayload::ResourceSnapshotResponse(inner) => {
            ResponsePayload::ResourceSnapshot(inner)
        }
        generated_protocol::ResponsePayload::ListenerSnapshotResponse(inner) => {
            ResponsePayload::ListenerSnapshot(inner)
        }
        generated_protocol::ResponsePayload::BoundUdpSnapshotResponse(inner) => {
            ResponsePayload::BoundUdpSnapshot(inner)
        }
        generated_protocol::ResponsePayload::VmFetchResponse(inner) => {
            ResponsePayload::VmFetchResult(inner)
        }
        generated_protocol::ResponsePayload::SignalStateResponse(inner) => {
            ResponsePayload::SignalState(inner)
        }
        generated_protocol::ResponsePayload::ZombieTimerCountResponse(inner) => {
            ResponsePayload::ZombieTimerCount(inner)
        }
        generated_protocol::ResponsePayload::FilesystemResultResponse(inner) => {
            ResponsePayload::FilesystemResult(FilesystemResultResponse {
                operation: from_generated_filesystem_operation(inner.operation),
                status: inner.status,
                payload_size_bytes: inner.payload_size_bytes,
            })
        }
        generated_protocol::ResponsePayload::PermissionDecisionResponse(inner) => {
            ResponsePayload::PermissionDecision(PermissionDecisionResponse {
                capability: inner.capability,
                decision: from_generated_permission_mode(inner.decision),
            })
        }
        generated_protocol::ResponsePayload::PersistenceStateResponse(inner) => {
            ResponsePayload::PersistenceState(inner)
        }
        generated_protocol::ResponsePayload::PersistenceFlushedResponse(inner) => {
            ResponsePayload::PersistenceFlushed(inner)
        }
        generated_protocol::ResponsePayload::RejectedResponse(inner) => {
            ResponsePayload::Rejected(inner)
        }
        generated_protocol::ResponsePayload::ExtEnvelope(inner) => {
            ResponsePayload::ExtResult(from_generated_ext_envelope(inner))
        }
        generated_protocol::ResponsePayload::GuestKernelResultResponse(inner) => {
            ResponsePayload::GuestKernelResult(inner)
        }
        generated_protocol::ResponsePayload::PtyResizedResponse(inner) => {
            ResponsePayload::PtyResized(inner)
        }
        generated_protocol::ResponsePayload::PackageLinkedResponse(inner) => {
            ResponsePayload::PackageLinked(inner)
        }
        generated_protocol::ResponsePayload::ProvidedCommandsResponse(inner) => {
            ResponsePayload::ProvidedCommands(inner)
        }
        generated_protocol::ResponsePayload::CronScheduledResponse(inner) => {
            ResponsePayload::CronScheduled(inner)
        }
        generated_protocol::ResponsePayload::CronJobsResponse(inner) => {
            ResponsePayload::CronJobs(inner)
        }
        generated_protocol::ResponsePayload::CronCancelledResponse(inner) => {
            ResponsePayload::CronCancelled(inner)
        }
        generated_protocol::ResponsePayload::CronWakeResponse(inner) => {
            ResponsePayload::CronWake(inner)
        }
        generated_protocol::ResponsePayload::CronRunCompletedResponse(inner) => {
            ResponsePayload::CronRunCompleted(inner)
        }
        generated_protocol::ResponsePayload::CronStateExportedResponse(inner) => {
            ResponsePayload::CronStateExported(inner)
        }
        generated_protocol::ResponsePayload::CronStateImportedResponse(inner) => {
            ResponsePayload::CronStateImported(inner)
        }
        generated_protocol::ResponsePayload::VmInitializedResponse(inner) => {
            ResponsePayload::VmInitialized(inner)
        }
    })
}

fn to_generated_event_payload(payload: &EventPayload) -> generated_protocol::EventPayload {
    match payload {
        EventPayload::VmLifecycle(inner) => generated_protocol::EventPayload::VmLifecycleEvent(
            generated_protocol::VmLifecycleEvent {
                state: to_generated_vm_lifecycle_state(&inner.state),
            },
        ),
        EventPayload::ProcessOutput(inner) => generated_protocol::EventPayload::ProcessOutputEvent(
            generated_protocol::ProcessOutputEvent {
                process_id: inner.process_id.clone(),
                channel: to_generated_stream_channel(&inner.channel),
                chunk: inner.chunk.clone(),
            },
        ),
        EventPayload::ProcessExited(inner) => {
            generated_protocol::EventPayload::ProcessExitedEvent(inner.clone())
        }
        EventPayload::CronDispatch(inner) => {
            generated_protocol::EventPayload::CronDispatchEvent(inner.clone())
        }
        EventPayload::Structured(inner) => {
            generated_protocol::EventPayload::StructuredEvent(inner.clone())
        }
        EventPayload::Ext(inner) => {
            generated_protocol::EventPayload::ExtEnvelope(to_generated_ext_envelope(inner))
        }
    }
}

fn from_generated_event_payload(payload: generated_protocol::EventPayload) -> EventPayload {
    match payload {
        generated_protocol::EventPayload::VmLifecycleEvent(inner) => {
            EventPayload::VmLifecycle(VmLifecycleEvent {
                state: from_generated_vm_lifecycle_state(inner.state),
            })
        }
        generated_protocol::EventPayload::ProcessOutputEvent(inner) => {
            EventPayload::ProcessOutput(ProcessOutputEvent {
                process_id: inner.process_id,
                channel: from_generated_stream_channel(inner.channel),
                chunk: inner.chunk,
            })
        }
        generated_protocol::EventPayload::ProcessExitedEvent(inner) => {
            EventPayload::ProcessExited(inner)
        }
        generated_protocol::EventPayload::CronDispatchEvent(inner) => {
            EventPayload::CronDispatch(inner)
        }
        generated_protocol::EventPayload::StructuredEvent(inner) => EventPayload::Structured(inner),
        generated_protocol::EventPayload::ExtEnvelope(inner) => {
            EventPayload::Ext(from_generated_ext_envelope(inner))
        }
    }
}

fn to_generated_sidecar_request_payload(
    payload: &SidecarRequestPayload,
) -> Result<generated_protocol::SidecarRequestPayload, ProtocolCodecError> {
    Ok(match payload {
        SidecarRequestPayload::HostCallback(inner) => {
            generated_protocol::SidecarRequestPayload::HostCallbackRequest(inner.clone())
        }
        SidecarRequestPayload::JsBridgeCall(inner) => {
            generated_protocol::SidecarRequestPayload::JsBridgeCallRequest(inner.clone())
        }
        SidecarRequestPayload::Ext(inner) => {
            generated_protocol::SidecarRequestPayload::ExtEnvelope(to_generated_ext_envelope(inner))
        }
    })
}

fn from_generated_sidecar_request_payload(
    payload: generated_protocol::SidecarRequestPayload,
) -> Result<SidecarRequestPayload, ProtocolCodecError> {
    Ok(match payload {
        generated_protocol::SidecarRequestPayload::HostCallbackRequest(inner) => {
            SidecarRequestPayload::HostCallback(inner)
        }
        generated_protocol::SidecarRequestPayload::JsBridgeCallRequest(inner) => {
            SidecarRequestPayload::JsBridgeCall(inner)
        }
        generated_protocol::SidecarRequestPayload::ExtEnvelope(inner) => {
            SidecarRequestPayload::Ext(from_generated_ext_envelope(inner))
        }
    })
}

fn to_generated_sidecar_response_payload(
    payload: &SidecarResponsePayload,
) -> Result<generated_protocol::SidecarResponsePayload, ProtocolCodecError> {
    Ok(match payload {
        SidecarResponsePayload::HostCallbackResult(inner) => {
            generated_protocol::SidecarResponsePayload::HostCallbackResultResponse(inner.clone())
        }
        SidecarResponsePayload::JsBridgeResult(inner) => {
            generated_protocol::SidecarResponsePayload::JsBridgeResultResponse(inner.clone())
        }
        SidecarResponsePayload::ExtResult(inner) => {
            generated_protocol::SidecarResponsePayload::ExtEnvelope(to_generated_ext_envelope(
                inner,
            ))
        }
    })
}

fn from_generated_sidecar_response_payload(
    payload: generated_protocol::SidecarResponsePayload,
) -> Result<SidecarResponsePayload, ProtocolCodecError> {
    Ok(match payload {
        generated_protocol::SidecarResponsePayload::HostCallbackResultResponse(inner) => {
            SidecarResponsePayload::HostCallbackResult(inner)
        }
        generated_protocol::SidecarResponsePayload::JsBridgeResultResponse(inner) => {
            SidecarResponsePayload::JsBridgeResult(inner)
        }
        generated_protocol::SidecarResponsePayload::ExtEnvelope(inner) => {
            SidecarResponsePayload::ExtResult(from_generated_ext_envelope(inner))
        }
    })
}

macro_rules! impl_bare_newtype_union_enum {
    (
        $name:ident,
        $json_name:ident,
        $(#[$json_attr:meta])*
        {
            $($variant:ident($ty:ty) = $tag:literal),+ $(,)?
        }
    ) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        $(#[$json_attr])*
        enum $json_name {
            $($variant($ty)),+
        }

        impl From<&$name> for $json_name {
            fn from(value: &$name) -> Self {
                match value {
                    $($name::$variant(inner) => Self::$variant(inner.clone()),)+
                }
            }
        }

        impl From<$json_name> for $name {
            fn from(value: $json_name) -> Self {
                match value {
                    $($json_name::$variant(inner) => Self::$variant(inner),)+
                }
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                if serializer.is_human_readable() {
                    $json_name::from(self).serialize(serializer)
                } else {
                    match self {
                        $(Self::$variant(inner) => serialize_bare_newtype_tag(serializer, $tag, inner),)+
                    }
                }
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                if deserializer.is_human_readable() {
                    Ok($json_name::deserialize(deserializer)?.into())
                } else {
                    struct UnionVisitor;

                    impl<'de> Visitor<'de> for UnionVisitor {
                        type Value = $name;

                        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                            write!(formatter, "a {} BARE union", stringify!($name))
                        }

                        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
                        where
                            A: SeqAccess<'de>,
                        {
                            let serde_bare::Uint(tag) = seq
                                .next_element()?
                                .ok_or_else(|| de::Error::custom(concat!("missing ", stringify!($name), " tag")))?;
                            match tag {
                                $(
                                    $tag => {
                                        let payload = seq.next_element::<$ty>()?.ok_or_else(|| {
                                            de::Error::custom(format!(
                                                "missing {} payload for tag {}",
                                                stringify!($variant),
                                                $tag
                                            ))
                                        })?;
                                        Ok($name::$variant(payload))
                                    }
                                )+
                                _ => Err(de::Error::custom(format!(
                                    "unknown {} tag: {}",
                                    stringify!($name),
                                    tag
                                ))),
                            }
                        }
                    }

                    deserializer.deserialize_tuple(2, UnionVisitor)
                }
            }
        }
    };
}

pub type ProtocolSchema = crate::wire::ProtocolSchema;

pub type OwnershipScope = crate::wire::OwnershipScope;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolFrame {
    Request(RequestFrame),
    Response(ResponseFrame),
    Event(EventFrame),
    SidecarRequest(SidecarRequestFrame),
    SidecarResponse(SidecarResponseFrame),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestFrame {
    pub schema: ProtocolSchema,
    pub request_id: RequestId,
    pub ownership: OwnershipScope,
    pub payload: RequestPayload,
}

impl RequestFrame {
    pub fn new(request_id: RequestId, ownership: OwnershipScope, payload: RequestPayload) -> Self {
        Self {
            schema: ProtocolSchema::current(),
            request_id,
            ownership,
            payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseFrame {
    pub schema: ProtocolSchema,
    pub request_id: RequestId,
    pub ownership: OwnershipScope,
    pub payload: ResponsePayload,
}

impl ResponseFrame {
    pub fn new(request_id: RequestId, ownership: OwnershipScope, payload: ResponsePayload) -> Self {
        Self {
            schema: ProtocolSchema::current(),
            request_id,
            ownership,
            payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidecarRequestFrame {
    pub schema: ProtocolSchema,
    pub request_id: RequestId,
    pub ownership: OwnershipScope,
    pub payload: SidecarRequestPayload,
}

impl SidecarRequestFrame {
    pub fn new(
        request_id: RequestId,
        ownership: OwnershipScope,
        payload: SidecarRequestPayload,
    ) -> Self {
        Self {
            schema: ProtocolSchema::current(),
            request_id,
            ownership,
            payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidecarResponseFrame {
    pub schema: ProtocolSchema,
    pub request_id: RequestId,
    pub ownership: OwnershipScope,
    pub payload: SidecarResponsePayload,
}

impl SidecarResponseFrame {
    pub fn new(
        request_id: RequestId,
        ownership: OwnershipScope,
        payload: SidecarResponsePayload,
    ) -> Self {
        Self {
            schema: ProtocolSchema::current(),
            request_id,
            ownership,
            payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventFrame {
    pub schema: ProtocolSchema,
    pub ownership: OwnershipScope,
    pub payload: EventPayload,
}

impl EventFrame {
    pub fn new(ownership: OwnershipScope, payload: EventPayload) -> Self {
        Self {
            schema: ProtocolSchema::current(),
            ownership,
            payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestPayload {
    Authenticate(AuthenticateRequest),
    OpenSession(OpenSessionRequest),
    CreateVm(CreateVmRequest),
    DisposeVm(DisposeVmRequest),
    BootstrapRootFilesystem(BootstrapRootFilesystemRequest),
    ConfigureVm(ConfigureVmRequest),
    RegisterHostCallbacks(RegisterHostCallbacksRequest),
    CreateLayer(CreateLayerRequest),
    SealLayer(SealLayerRequest),
    ImportSnapshot(ImportSnapshotRequest),
    ExportSnapshot(ExportSnapshotRequest),
    CreateOverlay(CreateOverlayRequest),
    GuestFilesystemCall(GuestFilesystemCallRequest),
    SnapshotRootFilesystem(SnapshotRootFilesystemRequest),
    Execute(ExecuteRequest),
    WriteStdin(WriteStdinRequest),
    CloseStdin(CloseStdinRequest),
    KillProcess(KillProcessRequest),
    GetProcessSnapshot(GetProcessSnapshotRequest),
    FindListener(FindListenerRequest),
    FindBoundUdp(FindBoundUdpRequest),
    VmFetch(VmFetchRequest),
    GetSignalState(GetSignalStateRequest),
    GetZombieTimerCount(GetZombieTimerCountRequest),
    HostFilesystemCall(HostFilesystemCallRequest),
    PersistenceLoad(PersistenceLoadRequest),
    PersistenceFlush(PersistenceFlushRequest),
    Ext(ExtEnvelope),
    GuestKernelCall(GuestKernelCallRequest),
    ResizePty(ResizePtyRequest),
    GetResourceSnapshot(GetResourceSnapshotRequest),
    LinkPackage(LinkPackageRequest),
    ProvidedCommands(ProvidedCommandsRequest),
    ScheduleCron(ScheduleCronRequest),
    ListCronJobs(ListCronJobsRequest),
    CancelCronJob(CancelCronJobRequest),
    WakeCron(WakeCronRequest),
    CompleteCronRun(CompleteCronRunRequest),
    ExportCronState(ExportCronStateRequest),
    ImportCronState(ImportCronStateRequest),
    InitializeVm(InitializeVmRequest),
    CloseSession(CloseSessionRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponsePayload {
    Authenticated(AuthenticatedResponse),
    SessionOpened(SessionOpenedResponse),
    VmCreated(VmCreatedResponse),
    VmDisposed(VmDisposedResponse),
    RootFilesystemBootstrapped(RootFilesystemBootstrappedResponse),
    VmConfigured(VmConfiguredResponse),
    HostCallbacksRegistered(HostCallbacksRegisteredResponse),
    LayerCreated(LayerCreatedResponse),
    LayerSealed(LayerSealedResponse),
    SnapshotImported(SnapshotImportedResponse),
    SnapshotExported(SnapshotExportedResponse),
    OverlayCreated(OverlayCreatedResponse),
    GuestFilesystemResult(GuestFilesystemResultResponse),
    RootFilesystemSnapshot(RootFilesystemSnapshotResponse),
    ProcessStarted(ProcessStartedResponse),
    StdinWritten(StdinWrittenResponse),
    StdinClosed(StdinClosedResponse),
    ProcessKilled(ProcessKilledResponse),
    ProcessSnapshot(ProcessSnapshotResponse),
    ListenerSnapshot(ListenerSnapshotResponse),
    BoundUdpSnapshot(BoundUdpSnapshotResponse),
    VmFetchResult(VmFetchResponse),
    SignalState(SignalStateResponse),
    ZombieTimerCount(ZombieTimerCountResponse),
    FilesystemResult(FilesystemResultResponse),
    PermissionDecision(PermissionDecisionResponse),
    PersistenceState(PersistenceStateResponse),
    PersistenceFlushed(PersistenceFlushedResponse),
    Rejected(RejectedResponse),
    ExtResult(ExtEnvelope),
    GuestKernelResult(GuestKernelResultResponse),
    PtyResized(PtyResizedResponse),
    ResourceSnapshot(ResourceSnapshotResponse),
    PackageLinked(PackageLinkedResponse),
    ProvidedCommands(ProvidedCommandsResponse),
    CronScheduled(CronScheduledResponse),
    CronJobs(CronJobsResponse),
    CronCancelled(CronCancelledResponse),
    CronWake(CronWakeResponse),
    CronRunCompleted(CronRunCompletedResponse),
    CronStateExported(CronStateExportedResponse),
    CronStateImported(CronStateImportedResponse),
    VmInitialized(VmInitializedResponse),
    SessionClosed(SessionClosedResponse),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarRequestPayload {
    HostCallback(HostCallbackRequest),
    JsBridgeCall(JsBridgeCallRequest),
    Ext(ExtEnvelope),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarResponsePayload {
    HostCallbackResult(HostCallbackResultResponse),
    JsBridgeResult(JsBridgeResultResponse),
    ExtResult(ExtEnvelope),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventPayload {
    VmLifecycle(VmLifecycleEvent),
    ProcessOutput(ProcessOutputEvent),
    ProcessExited(ProcessExitedEvent),
    CronDispatch(CronDispatchEvent),
    Structured(StructuredEvent),
    Ext(ExtEnvelope),
}

pub type SidecarPlacement = crate::wire::SidecarPlacement;

pub type SidecarPlacementShared = crate::wire::SidecarPlacementShared;

pub type SidecarPlacementExplicit = crate::wire::SidecarPlacementExplicit;

pub type GuestRuntimeKind = crate::wire::GuestRuntimeKind;

pub type DisposeReason = crate::wire::DisposeReason;

pub type FilesystemOperation = crate::wire::FilesystemOperation;

pub type GuestFilesystemOperation = crate::wire::GuestFilesystemOperation;

pub type PermissionMode = crate::wire::PermissionMode;

pub type FsPermissionRule = crate::wire::FsPermissionRule;

pub type PatternPermissionRule = crate::wire::PatternPermissionRule;

pub type FsPermissionRuleSet = crate::wire::FsPermissionRuleSet;

pub type PatternPermissionRuleSet = crate::wire::PatternPermissionRuleSet;

pub type FsPermissionScope = crate::wire::FsPermissionScope;

pub type PatternPermissionScope = crate::wire::PatternPermissionScope;

pub type PermissionsPolicy = crate::wire::PermissionsPolicy;

pub type RootFilesystemEntryKind = crate::wire::RootFilesystemEntryKind;

pub type RootFilesystemMode = crate::wire::RootFilesystemMode;

pub type RootFilesystemLowerDescriptor = crate::wire::RootFilesystemLowerDescriptor;

pub type SnapshotRootFilesystemLower = crate::wire::SnapshotRootFilesystemLower;

pub type StreamChannel = crate::wire::StreamChannel;

pub type VmLifecycleState = crate::wire::VmLifecycleState;

pub type AuthenticateRequest = crate::wire::AuthenticateRequest;

pub type OpenSessionRequest = crate::wire::OpenSessionRequest;

pub type CloseSessionRequest = crate::wire::CloseSessionRequest;

pub type CreateVmRequest = crate::wire::CreateVmRequest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisposeVmRequest {
    pub reason: DisposeReason,
}

pub type BootstrapRootFilesystemRequest = crate::wire::BootstrapRootFilesystemRequest;

pub type RootFilesystemDescriptor = crate::wire::RootFilesystemDescriptor;

pub type RootFilesystemEntryEncoding = crate::wire::RootFilesystemEntryEncoding;

pub type RootFilesystemEntry = crate::wire::RootFilesystemEntry;

pub type ConfigureVmRequest = crate::wire::ConfigureVmRequest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CreateLayerRequest {}

pub type SealLayerRequest = crate::wire::SealLayerRequest;

pub type ImportSnapshotRequest = crate::wire::ImportSnapshotRequest;

pub type ExportSnapshotRequest = crate::wire::ExportSnapshotRequest;

pub type CreateOverlayRequest = crate::wire::CreateOverlayRequest;

pub type GuestFilesystemCallRequest = crate::wire::GuestFilesystemCallRequest;

pub type GuestKernelCallRequest = crate::wire::GuestKernelCallRequest;
pub type ResizePtyRequest = crate::wire::ResizePtyRequest;
pub type PackageDescriptor = crate::wire::PackageDescriptor;
pub type AgentosProjectedAgent = crate::wire::AgentosProjectedAgent;
pub type PackageCommands = crate::wire::PackageCommands;
pub type ProjectedCommand = crate::wire::ProjectedCommand;
pub type LinkPackageRequest = crate::wire::LinkPackageRequest;
pub type CronOverlap = crate::wire::CronOverlap;
pub type ScheduleCronRequest = crate::wire::ScheduleCronRequest;
pub type CancelCronJobRequest = crate::wire::CancelCronJobRequest;
pub type WakeCronRequest = crate::wire::WakeCronRequest;
pub type CompleteCronRunRequest = crate::wire::CompleteCronRunRequest;
pub type ImportCronStateRequest = crate::wire::ImportCronStateRequest;
pub type InitializeVmRequest = crate::wire::InitializeVmRequest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProvidedCommandsRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ListCronJobsRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExportCronStateRequest {}

pub type GuestKernelResultResponse = crate::wire::GuestKernelResultResponse;
pub type PtyResizedResponse = crate::wire::PtyResizedResponse;
pub type PackageLinkedResponse = crate::wire::PackageLinkedResponse;
pub type ProvidedCommandsResponse = crate::wire::ProvidedCommandsResponse;
pub type CronAlarm = crate::wire::CronAlarm;
pub type CronJobEntry = crate::wire::CronJobEntry;
pub type CronRun = crate::wire::CronRun;
pub type CronEventKind = crate::wire::CronEventKind;
pub type CronEventRecord = crate::wire::CronEventRecord;
pub type CronDispatchEvent = crate::wire::CronDispatchEvent;
pub type CronScheduledResponse = crate::wire::CronScheduledResponse;
pub type CronJobsResponse = crate::wire::CronJobsResponse;
pub type CronCancelledResponse = crate::wire::CronCancelledResponse;
pub type CronWakeResponse = crate::wire::CronWakeResponse;
pub type CronRunCompletedResponse = crate::wire::CronRunCompletedResponse;
pub type CronStateExportedResponse = crate::wire::CronStateExportedResponse;
pub type CronStateImportedResponse = crate::wire::CronStateImportedResponse;
pub type VmInitializedResponse = crate::wire::VmInitializedResponse;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SnapshotRootFilesystemRequest {}

pub type MountDescriptor = crate::wire::MountDescriptor;

pub type MountPluginDescriptor = crate::wire::MountPluginDescriptor;

pub type WasmPermissionTier = crate::wire::WasmPermissionTier;

pub type ExecuteRequest = crate::wire::ExecuteRequest;

pub type WriteStdinRequest = crate::wire::WriteStdinRequest;

pub type CloseStdinRequest = crate::wire::CloseStdinRequest;

pub type KillProcessRequest = crate::wire::KillProcessRequest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GetProcessSnapshotRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GetResourceSnapshotRequest {}

pub type FindListenerRequest = crate::wire::FindListenerRequest;

pub type FindBoundUdpRequest = crate::wire::FindBoundUdpRequest;

pub type VmFetchRequest = crate::wire::VmFetchRequest;

pub type GetSignalStateRequest = crate::wire::GetSignalStateRequest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GetZombieTimerCountRequest {}

pub type HostFilesystemCallRequest = crate::wire::HostFilesystemCallRequest;

pub type PersistenceLoadRequest = crate::wire::PersistenceLoadRequest;

pub type PersistenceFlushRequest = crate::wire::PersistenceFlushRequest;

pub type RegisterHostCallbacksRequest = crate::wire::RegisterHostCallbacksRequest;

pub type RegisteredHostCallbackDefinition = crate::wire::RegisteredHostCallbackDefinition;

pub type RegisteredHostCallbackExample = crate::wire::RegisteredHostCallbackExample;

pub type HostCallbackRequest = crate::wire::HostCallbackRequest;

pub type JsBridgeCallRequest = crate::wire::JsBridgeCallRequest;

pub type AuthenticatedResponse = crate::wire::AuthenticatedResponse;

pub type SessionOpenedResponse = crate::wire::SessionOpenedResponse;

pub type SessionClosedResponse = crate::wire::SessionClosedResponse;

pub type VmCreatedResponse = crate::wire::VmCreatedResponse;

pub type VmDisposedResponse = crate::wire::VmDisposedResponse;

pub type RootFilesystemBootstrappedResponse = crate::wire::RootFilesystemBootstrappedResponse;

pub type VmConfiguredResponse = crate::wire::VmConfiguredResponse;

pub type HostCallbacksRegisteredResponse = crate::wire::HostCallbacksRegisteredResponse;

pub type GuestFilesystemStat = crate::wire::GuestFilesystemStat;

pub type GuestDirEntry = crate::wire::GuestDirEntry;

pub type GuestFilesystemResultResponse = crate::wire::GuestFilesystemResultResponse;

pub type RootFilesystemSnapshotResponse = crate::wire::RootFilesystemSnapshotResponse;

pub type LayerCreatedResponse = crate::wire::LayerCreatedResponse;

pub type LayerSealedResponse = crate::wire::LayerSealedResponse;

pub type SnapshotImportedResponse = crate::wire::SnapshotImportedResponse;

pub type SnapshotExportedResponse = crate::wire::SnapshotExportedResponse;

pub type OverlayCreatedResponse = crate::wire::OverlayCreatedResponse;

pub type ProcessStartedResponse = crate::wire::ProcessStartedResponse;

pub type StdinWrittenResponse = crate::wire::StdinWrittenResponse;

pub type StdinClosedResponse = crate::wire::StdinClosedResponse;

pub type ProcessKilledResponse = crate::wire::ProcessKilledResponse;

pub type ProcessSnapshotStatus = crate::wire::ProcessSnapshotStatus;

pub type ProcessSnapshotEntry = crate::wire::ProcessSnapshotEntry;

pub type ProcessSnapshotResponse = crate::wire::ProcessSnapshotResponse;

pub type QueueSnapshotEntry = crate::wire::QueueSnapshotEntry;

pub type ResourceSnapshotResponse = crate::wire::ResourceSnapshotResponse;

pub type SocketStateEntry = crate::wire::SocketStateEntry;

pub type ListenerSnapshotResponse = crate::wire::ListenerSnapshotResponse;

pub type BoundUdpSnapshotResponse = crate::wire::BoundUdpSnapshotResponse;

pub type VmFetchResponse = crate::wire::VmFetchResponse;

pub type SignalDispositionAction = crate::wire::SignalDispositionAction;

pub type SignalHandlerRegistration = crate::wire::SignalHandlerRegistration;

pub type SignalStateResponse = crate::wire::SignalStateResponse;

pub type ZombieTimerCountResponse = crate::wire::ZombieTimerCountResponse;

pub type FilesystemResultResponse = crate::wire::FilesystemResultResponse;

pub type PermissionDecisionResponse = crate::wire::PermissionDecisionResponse;

pub type PersistenceStateResponse = crate::wire::PersistenceStateResponse;

pub type PersistenceFlushedResponse = crate::wire::PersistenceFlushedResponse;

pub type HostCallbackResultResponse = crate::wire::HostCallbackResultResponse;

pub type JsBridgeResultResponse = crate::wire::JsBridgeResultResponse;

pub type RejectedResponse = crate::wire::RejectedResponse;

pub type VmLifecycleEvent = crate::wire::VmLifecycleEvent;

pub type ProcessOutputEvent = crate::wire::ProcessOutputEvent;

pub type ProcessExitedEvent = crate::wire::ProcessExitedEvent;

pub type StructuredEvent = crate::wire::StructuredEvent;

impl_bare_newtype_union_enum!(
    ProtocolFrame,
    JsonProtocolFrame,
    #[serde(tag = "frame_type", rename_all = "snake_case")]
    {
        Request(RequestFrame) = 0,
        Response(ResponseFrame) = 1,
        Event(EventFrame) = 2,
        SidecarRequest(SidecarRequestFrame) = 3,
        SidecarResponse(SidecarResponseFrame) = 4,
    }
);

impl_bare_newtype_union_enum!(
    RequestPayload,
    JsonRequestPayload,
    #[serde(tag = "type", rename_all = "snake_case")]
    {
        Authenticate(AuthenticateRequest) = 0,
        OpenSession(OpenSessionRequest) = 1,
        CreateVm(CreateVmRequest) = 2,
        DisposeVm(DisposeVmRequest) = 3,
        BootstrapRootFilesystem(BootstrapRootFilesystemRequest) = 4,
        ConfigureVm(ConfigureVmRequest) = 5,
        RegisterHostCallbacks(RegisterHostCallbacksRequest) = 6,
        CreateLayer(CreateLayerRequest) = 7,
        SealLayer(SealLayerRequest) = 8,
        ImportSnapshot(ImportSnapshotRequest) = 9,
        ExportSnapshot(ExportSnapshotRequest) = 10,
        CreateOverlay(CreateOverlayRequest) = 11,
        GuestFilesystemCall(GuestFilesystemCallRequest) = 12,
        SnapshotRootFilesystem(SnapshotRootFilesystemRequest) = 13,
        Execute(ExecuteRequest) = 14,
        WriteStdin(WriteStdinRequest) = 15,
        CloseStdin(CloseStdinRequest) = 16,
        KillProcess(KillProcessRequest) = 17,
        GetProcessSnapshot(GetProcessSnapshotRequest) = 18,
        FindListener(FindListenerRequest) = 19,
        FindBoundUdp(FindBoundUdpRequest) = 20,
        GetSignalState(GetSignalStateRequest) = 21,
        GetZombieTimerCount(GetZombieTimerCountRequest) = 22,
        HostFilesystemCall(HostFilesystemCallRequest) = 23,
        PersistenceLoad(PersistenceLoadRequest) = 24,
        PersistenceFlush(PersistenceFlushRequest) = 25,
        VmFetch(VmFetchRequest) = 26,
        Ext(ExtEnvelope) = 27,
        GuestKernelCall(GuestKernelCallRequest) = 28,
        ResizePty(ResizePtyRequest) = 29,
        GetResourceSnapshot(GetResourceSnapshotRequest) = 30,
        LinkPackage(LinkPackageRequest) = 31,
        ProvidedCommands(ProvidedCommandsRequest) = 32,
        ScheduleCron(ScheduleCronRequest) = 33,
        ListCronJobs(ListCronJobsRequest) = 34,
        CancelCronJob(CancelCronJobRequest) = 35,
        WakeCron(WakeCronRequest) = 36,
        CompleteCronRun(CompleteCronRunRequest) = 37,
        ExportCronState(ExportCronStateRequest) = 38,
        ImportCronState(ImportCronStateRequest) = 39,
        InitializeVm(InitializeVmRequest) = 40,
        CloseSession(CloseSessionRequest) = 41,
    }
);

impl_bare_newtype_union_enum!(
    ResponsePayload,
    JsonResponsePayload,
    #[serde(tag = "type", rename_all = "snake_case")]
    {
        Authenticated(AuthenticatedResponse) = 0,
        SessionOpened(SessionOpenedResponse) = 1,
        VmCreated(VmCreatedResponse) = 2,
        VmDisposed(VmDisposedResponse) = 3,
        RootFilesystemBootstrapped(RootFilesystemBootstrappedResponse) = 4,
        VmConfigured(VmConfiguredResponse) = 5,
        HostCallbacksRegistered(HostCallbacksRegisteredResponse) = 6,
        LayerCreated(LayerCreatedResponse) = 7,
        LayerSealed(LayerSealedResponse) = 8,
        SnapshotImported(SnapshotImportedResponse) = 9,
        SnapshotExported(SnapshotExportedResponse) = 10,
        OverlayCreated(OverlayCreatedResponse) = 11,
        GuestFilesystemResult(GuestFilesystemResultResponse) = 12,
        RootFilesystemSnapshot(RootFilesystemSnapshotResponse) = 13,
        ProcessStarted(ProcessStartedResponse) = 14,
        StdinWritten(StdinWrittenResponse) = 15,
        StdinClosed(StdinClosedResponse) = 16,
        ProcessKilled(ProcessKilledResponse) = 17,
        ProcessSnapshot(ProcessSnapshotResponse) = 18,
        ListenerSnapshot(ListenerSnapshotResponse) = 19,
        BoundUdpSnapshot(BoundUdpSnapshotResponse) = 20,
        SignalState(SignalStateResponse) = 21,
        ZombieTimerCount(ZombieTimerCountResponse) = 22,
        FilesystemResult(FilesystemResultResponse) = 23,
        PermissionDecision(PermissionDecisionResponse) = 24,
        PersistenceState(PersistenceStateResponse) = 25,
        PersistenceFlushed(PersistenceFlushedResponse) = 26,
        Rejected(RejectedResponse) = 27,
        VmFetchResult(VmFetchResponse) = 28,
        ExtResult(ExtEnvelope) = 29,
        GuestKernelResult(GuestKernelResultResponse) = 30,
        PtyResized(PtyResizedResponse) = 31,
        ResourceSnapshot(ResourceSnapshotResponse) = 32,
        PackageLinked(PackageLinkedResponse) = 33,
        ProvidedCommands(ProvidedCommandsResponse) = 34,
        CronScheduled(CronScheduledResponse) = 35,
        CronJobs(CronJobsResponse) = 36,
        CronCancelled(CronCancelledResponse) = 37,
        CronWake(CronWakeResponse) = 38,
        CronRunCompleted(CronRunCompletedResponse) = 39,
        CronStateExported(CronStateExportedResponse) = 40,
        CronStateImported(CronStateImportedResponse) = 41,
        VmInitialized(VmInitializedResponse) = 42,
        SessionClosed(SessionClosedResponse) = 43,
    }
);

impl_bare_newtype_union_enum!(
    SidecarRequestPayload,
    JsonSidecarRequestPayload,
    #[serde(tag = "type", rename_all = "snake_case")]
    {
        HostCallback(HostCallbackRequest) = 0,
        JsBridgeCall(JsBridgeCallRequest) = 1,
        Ext(ExtEnvelope) = 2,
    }
);

impl_bare_newtype_union_enum!(
    SidecarResponsePayload,
    JsonSidecarResponsePayload,
    #[allow(clippy::enum_variant_names)]
    #[serde(tag = "type", rename_all = "snake_case")]
    {
        HostCallbackResult(HostCallbackResultResponse) = 0,
        JsBridgeResult(JsBridgeResultResponse) = 1,
        ExtResult(ExtEnvelope) = 2,
    }
);

impl_bare_newtype_union_enum!(
    EventPayload,
    JsonEventPayload,
    #[serde(tag = "type", rename_all = "snake_case")]
    {
        VmLifecycle(VmLifecycleEvent) = 0,
        ProcessOutput(ProcessOutputEvent) = 1,
        ProcessExited(ProcessExitedEvent) = 2,
        CronDispatch(CronDispatchEvent) = 3,
        Structured(StructuredEvent) = 4,
        Ext(ExtEnvelope) = 5,
    }
);

fn serialize_payload(
    frame: &ProtocolFrame,
    payload_codec: NativePayloadCodec,
) -> Result<Vec<u8>, ProtocolCodecError> {
    match payload_codec {
        NativePayloadCodec::Json => serde_json::to_vec(frame)
            .map_err(|error| ProtocolCodecError::SerializeFailure(error.to_string())),
        NativePayloadCodec::Bare => serde_bare::to_vec(&to_generated_protocol_frame(frame)?)
            .map_err(|error| ProtocolCodecError::SerializeFailure(error.to_string())),
    }
}

fn deserialize_payload(
    payload: &[u8],
    payload_codec: NativePayloadCodec,
) -> Result<ProtocolFrame, ProtocolCodecError> {
    match payload_codec {
        NativePayloadCodec::Json => serde_json::from_slice(payload)
            .map_err(|error| ProtocolCodecError::DeserializeFailure(error.to_string())),
        NativePayloadCodec::Bare => {
            let frame: generated_protocol::ProtocolFrame = serde_bare::from_slice(payload)
                .map_err(|error| ProtocolCodecError::DeserializeFailure(error.to_string()))?;
            from_generated_protocol_frame(frame)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativePayloadCodec {
    Json,
    Bare,
}

impl NativePayloadCodec {
    pub fn sniff(payload: &[u8]) -> Self {
        match payload.first() {
            Some(b'{') => Self::Json,
            _ => Self::Bare,
        }
    }

    pub fn alternate(self) -> Self {
        match self {
            Self::Json => Self::Bare,
            Self::Bare => Self::Json,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NativeFrameCodec {
    max_frame_bytes: usize,
    payload_codec: NativePayloadCodec,
}

impl NativeFrameCodec {
    pub fn new(max_frame_bytes: usize) -> Self {
        Self::with_payload_codec(max_frame_bytes, NativePayloadCodec::Json)
    }

    pub fn with_payload_codec(max_frame_bytes: usize, payload_codec: NativePayloadCodec) -> Self {
        Self {
            max_frame_bytes,
            payload_codec,
        }
    }

    pub fn max_frame_bytes(&self) -> usize {
        self.max_frame_bytes
    }

    pub fn payload_codec(&self) -> NativePayloadCodec {
        self.payload_codec
    }

    pub fn encode(&self, frame: &ProtocolFrame) -> Result<Vec<u8>, ProtocolCodecError> {
        self.encode_with_codec(frame, self.payload_codec)
    }

    pub fn encode_with_codec(
        &self,
        frame: &ProtocolFrame,
        payload_codec: NativePayloadCodec,
    ) -> Result<Vec<u8>, ProtocolCodecError> {
        validate_frame(frame)?;

        let payload = serialize_payload(frame, payload_codec)?;
        if payload.len() > self.max_frame_bytes {
            return Err(ProtocolCodecError::FrameTooLarge {
                size: payload.len(),
                max: self.max_frame_bytes,
            });
        }

        let length =
            u32::try_from(payload.len()).map_err(|_| ProtocolCodecError::FrameTooLarge {
                size: payload.len(),
                max: u32::MAX as usize,
            })?;

        let mut encoded = Vec::with_capacity(4 + payload.len());
        encoded.extend_from_slice(&length.to_be_bytes());
        encoded.extend_from_slice(&payload);
        Ok(encoded)
    }

    pub fn decode(&self, bytes: &[u8]) -> Result<ProtocolFrame, ProtocolCodecError> {
        self.decode_detected(bytes).map(|(frame, _)| frame)
    }

    pub fn decode_with_codec(
        &self,
        bytes: &[u8],
        payload_codec: NativePayloadCodec,
    ) -> Result<ProtocolFrame, ProtocolCodecError> {
        let payload = self.checked_payload(bytes)?;
        let frame = deserialize_payload(payload, payload_codec)?;
        validate_frame(&frame)?;
        Ok(frame)
    }

    pub fn decode_detected(
        &self,
        bytes: &[u8],
    ) -> Result<(ProtocolFrame, NativePayloadCodec), ProtocolCodecError> {
        let payload = self.checked_payload(bytes)?;
        let primary = NativePayloadCodec::sniff(payload);

        match deserialize_payload(payload, primary) {
            Ok(frame) => {
                validate_frame(&frame)?;
                Ok((frame, primary))
            }
            Err(primary_error) => {
                let alternate = primary.alternate();
                let frame = deserialize_payload(payload, alternate).map_err(|_| primary_error)?;
                validate_frame(&frame)?;
                Ok((frame, alternate))
            }
        }
    }

    fn checked_payload<'a>(&self, bytes: &'a [u8]) -> Result<&'a [u8], ProtocolCodecError> {
        if bytes.len() < 4 {
            return Err(ProtocolCodecError::TruncatedFrame {
                actual: bytes.len(),
            });
        }

        let declared =
            u32::from_be_bytes(bytes[..4].try_into().expect("length prefix is four bytes"))
                as usize;
        if declared > self.max_frame_bytes {
            return Err(ProtocolCodecError::FrameTooLarge {
                size: declared,
                max: self.max_frame_bytes,
            });
        }

        let actual = bytes.len() - 4;
        if declared != actual {
            return Err(ProtocolCodecError::LengthPrefixMismatch { declared, actual });
        }
        Ok(&bytes[4..])
    }
}

impl Default for NativeFrameCodec {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_FRAME_BYTES)
    }
}

#[derive(Debug)]
pub struct ResponseTracker {
    pending: HashMap<RequestId, PendingRequest>,
    completed: HashSet<RequestId>,
    completed_order: VecDeque<RequestId>,
    completed_cap: usize,
}

#[derive(Debug)]
pub struct SidecarResponseTracker {
    pending: HashMap<RequestId, PendingSidecarRequest>,
    completed: HashSet<RequestId>,
    completed_order: VecDeque<RequestId>,
    completed_cap: usize,
}

impl ResponseTracker {
    pub fn with_completed_cap(completed_cap: usize) -> Self {
        Self {
            pending: HashMap::new(),
            completed: HashSet::new(),
            completed_order: VecDeque::new(),
            completed_cap: completed_cap.max(1),
        }
    }

    pub fn completed_count(&self) -> usize {
        self.completed.len()
    }

    pub fn register_request(&mut self, request: &RequestFrame) -> Result<(), ResponseTrackerError> {
        if self.pending.contains_key(&request.request_id)
            || self.completed.contains(&request.request_id)
        {
            return Err(ResponseTrackerError::DuplicateRequestId {
                request_id: request.request_id,
            });
        }

        self.pending.insert(
            request.request_id,
            PendingRequest {
                ownership: request.ownership.clone(),
                expected_response: request.payload.expected_response(),
            },
        );
        Ok(())
    }

    pub fn accept_response(
        &mut self,
        response: &ResponseFrame,
    ) -> Result<(), ResponseTrackerError> {
        if self.completed.contains(&response.request_id) {
            return Err(ResponseTrackerError::DuplicateResponse {
                request_id: response.request_id,
            });
        }

        let pending = self.pending.get(&response.request_id).ok_or(
            ResponseTrackerError::UnmatchedResponse {
                request_id: response.request_id,
            },
        )?;

        if pending.ownership != response.ownership {
            return Err(ResponseTrackerError::OwnershipMismatch {
                request_id: response.request_id,
                expected: Box::new(pending.ownership.clone()),
                actual: Box::new(response.ownership.clone()),
            });
        }

        if !pending.expected_response.matches(&response.payload) {
            return Err(ResponseTrackerError::ResponseKindMismatch {
                request_id: response.request_id,
                expected: pending.expected_response.as_str().to_string(),
                actual: response.payload.kind_name().to_string(),
            });
        }

        self.pending
            .remove(&response.request_id)
            .expect("pending response should still exist after validation");
        self.completed.insert(response.request_id);
        self.completed_order.push_back(response.request_id);
        while self.completed.len() > self.completed_cap {
            if let Some(evicted) = self.completed_order.pop_front() {
                self.completed.remove(&evicted);
            }
        }
        Ok(())
    }
}

impl Default for ResponseTracker {
    fn default() -> Self {
        Self::with_completed_cap(DEFAULT_COMPLETED_RESPONSE_CAP)
    }
}

impl SidecarResponseTracker {
    pub fn with_completed_cap(completed_cap: usize) -> Self {
        Self {
            pending: HashMap::new(),
            completed: HashSet::new(),
            completed_order: VecDeque::new(),
            completed_cap: completed_cap.max(1),
        }
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn completed_count(&self) -> usize {
        self.completed.len()
    }

    pub fn register_request(
        &mut self,
        request: &SidecarRequestFrame,
    ) -> Result<(), SidecarResponseTrackerError> {
        if self.pending.contains_key(&request.request_id)
            || self.completed.contains(&request.request_id)
        {
            return Err(SidecarResponseTrackerError::DuplicateRequestId {
                request_id: request.request_id,
            });
        }

        self.pending.insert(
            request.request_id,
            PendingSidecarRequest {
                ownership: request.ownership.clone(),
                expected_response: request.payload.expected_response(),
            },
        );
        Ok(())
    }

    pub fn accept_response(
        &mut self,
        response: &SidecarResponseFrame,
    ) -> Result<(), SidecarResponseTrackerError> {
        if self.completed.contains(&response.request_id) {
            return Err(SidecarResponseTrackerError::DuplicateResponse {
                request_id: response.request_id,
            });
        }

        let pending = self.pending.get(&response.request_id).ok_or(
            SidecarResponseTrackerError::UnmatchedResponse {
                request_id: response.request_id,
            },
        )?;

        if pending.ownership != response.ownership {
            return Err(SidecarResponseTrackerError::OwnershipMismatch {
                request_id: response.request_id,
                expected: Box::new(pending.ownership.clone()),
                actual: Box::new(response.ownership.clone()),
            });
        }

        if !pending.expected_response.matches(&response.payload) {
            return Err(SidecarResponseTrackerError::ResponseKindMismatch {
                request_id: response.request_id,
                expected: pending.expected_response.as_str().to_string(),
                actual: response.payload.kind_name().to_string(),
            });
        }

        self.pending
            .remove(&response.request_id)
            .expect("pending sidecar response should still exist after validation");
        self.completed.insert(response.request_id);
        self.completed_order.push_back(response.request_id);
        while self.completed.len() > self.completed_cap {
            if let Some(evicted) = self.completed_order.pop_front() {
                self.completed.remove(&evicted);
            }
        }
        Ok(())
    }
}

impl Default for SidecarResponseTracker {
    fn default() -> Self {
        Self::with_completed_cap(DEFAULT_COMPLETED_RESPONSE_CAP)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseTrackerError {
    DuplicateRequestId {
        request_id: RequestId,
    },
    UnmatchedResponse {
        request_id: RequestId,
    },
    DuplicateResponse {
        request_id: RequestId,
    },
    OwnershipMismatch {
        request_id: RequestId,
        expected: Box<OwnershipScope>,
        actual: Box<OwnershipScope>,
    },
    ResponseKindMismatch {
        request_id: RequestId,
        expected: String,
        actual: String,
    },
}

impl fmt::Display for ResponseTrackerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateRequestId { request_id } => {
                write!(f, "request id {request_id} is already tracked")
            }
            Self::UnmatchedResponse { request_id } => {
                write!(
                    f,
                    "response id {request_id} does not match any pending request"
                )
            }
            Self::DuplicateResponse { request_id } => {
                write!(f, "response id {request_id} has already been completed")
            }
            Self::OwnershipMismatch {
                request_id,
                expected,
                actual,
            } => write!(
                f,
                "response id {request_id} used ownership {:?}, expected {:?}",
                actual, expected
            ),
            Self::ResponseKindMismatch {
                request_id,
                expected,
                actual,
            } => write!(
                f,
                "response id {request_id} carried {actual}, expected {expected}",
            ),
        }
    }
}

impl Error for ResponseTrackerError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarResponseTrackerError {
    DuplicateRequestId {
        request_id: RequestId,
    },
    UnmatchedResponse {
        request_id: RequestId,
    },
    DuplicateResponse {
        request_id: RequestId,
    },
    OwnershipMismatch {
        request_id: RequestId,
        expected: Box<OwnershipScope>,
        actual: Box<OwnershipScope>,
    },
    ResponseKindMismatch {
        request_id: RequestId,
        expected: String,
        actual: String,
    },
}

impl fmt::Display for SidecarResponseTrackerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateRequestId { request_id } => {
                write!(f, "sidecar request id {request_id} is already tracked")
            }
            Self::UnmatchedResponse { request_id } => {
                write!(
                    f,
                    "sidecar response id {request_id} does not match any pending request"
                )
            }
            Self::DuplicateResponse { request_id } => {
                write!(
                    f,
                    "sidecar response id {request_id} has already been completed"
                )
            }
            Self::OwnershipMismatch {
                request_id,
                expected,
                actual,
            } => write!(
                f,
                "sidecar response id {request_id} used ownership {:?}, expected {:?}",
                actual, expected
            ),
            Self::ResponseKindMismatch {
                request_id,
                expected,
                actual,
            } => write!(
                f,
                "sidecar response id {request_id} carried {actual}, expected {expected}",
            ),
        }
    }
}

impl Error for SidecarResponseTrackerError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingRequest {
    ownership: OwnershipScope,
    expected_response: ExpectedResponseKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingSidecarRequest {
    ownership: OwnershipScope,
    expected_response: ExpectedSidecarResponseKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedResponseKind {
    Authenticated,
    SessionOpened,
    SessionClosed,
    VmCreated,
    VmDisposed,
    RootFilesystemBootstrapped,
    VmConfigured,
    HostCallbacksRegistered,
    LayerCreated,
    LayerSealed,
    SnapshotImported,
    SnapshotExported,
    OverlayCreated,
    GuestFilesystemResult,
    RootFilesystemSnapshot,
    ProcessStarted,
    StdinWritten,
    StdinClosed,
    ProcessKilled,
    ProcessSnapshot,
    ResourceSnapshot,
    ListenerSnapshot,
    BoundUdpSnapshot,
    VmFetchResult,
    SignalState,
    ZombieTimerCount,
    FilesystemResult,
    // `PermissionDecision` is a sidecar-initiated callback response, so no host
    // request maps to it in `expected_response_kind`; kept for protocol symmetry.
    #[allow(dead_code)]
    PermissionDecision,
    PersistenceState,
    PersistenceFlushed,
    ExtResult,
    GuestKernelResult,
    PtyResized,
    PackageLinked,
    ProvidedCommands,
    CronScheduled,
    CronJobs,
    CronCancelled,
    CronWake,
    CronRunCompleted,
    CronStateExported,
    CronStateImported,
    VmInitialized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedSidecarResponseKind {
    HostCallback,
    JsBridge,
    Ext,
}

impl ExpectedResponseKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Authenticated => "authenticated",
            Self::SessionOpened => "session_opened",
            Self::SessionClosed => "session_closed",
            Self::VmCreated => "vm_created",
            Self::VmDisposed => "vm_disposed",
            Self::RootFilesystemBootstrapped => "root_filesystem_bootstrapped",
            Self::VmConfigured => "vm_configured",
            Self::HostCallbacksRegistered => "host_callbacks_registered",
            Self::LayerCreated => "layer_created",
            Self::LayerSealed => "layer_sealed",
            Self::SnapshotImported => "snapshot_imported",
            Self::SnapshotExported => "snapshot_exported",
            Self::OverlayCreated => "overlay_created",
            Self::GuestFilesystemResult => "guest_filesystem_result",
            Self::RootFilesystemSnapshot => "root_filesystem_snapshot",
            Self::ProcessStarted => "process_started",
            Self::StdinWritten => "stdin_written",
            Self::StdinClosed => "stdin_closed",
            Self::ProcessKilled => "process_killed",
            Self::ProcessSnapshot => "process_snapshot",
            Self::ResourceSnapshot => "resource_snapshot",
            Self::ListenerSnapshot => "listener_snapshot",
            Self::BoundUdpSnapshot => "bound_udp_snapshot",
            Self::VmFetchResult => "vm_fetch_result",
            Self::SignalState => "signal_state",
            Self::ZombieTimerCount => "zombie_timer_count",
            Self::FilesystemResult => "filesystem_result",
            Self::PermissionDecision => "permission_decision",
            Self::PersistenceState => "persistence_state",
            Self::PersistenceFlushed => "persistence_flushed",
            Self::ExtResult => "ext_result",
            Self::GuestKernelResult => "guest_kernel_result",
            Self::PtyResized => "pty_resized",
            Self::PackageLinked => "package_linked",
            Self::ProvidedCommands => "provided_commands_response",
            Self::CronScheduled => "cron_scheduled",
            Self::CronJobs => "cron_jobs",
            Self::CronCancelled => "cron_cancelled",
            Self::CronWake => "cron_wake",
            Self::CronRunCompleted => "cron_run_completed",
            Self::CronStateExported => "cron_state_exported",
            Self::CronStateImported => "cron_state_imported",
            Self::VmInitialized => "vm_initialized",
        }
    }

    fn matches(self, payload: &ResponsePayload) -> bool {
        match payload {
            ResponsePayload::Rejected(_) => true,
            _ => payload.kind_name() == self.as_str(),
        }
    }
}

impl ExpectedSidecarResponseKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::HostCallback => "host_callback_result",
            Self::JsBridge => "js_bridge_result",
            Self::Ext => "ext_result",
        }
    }

    fn matches(self, payload: &SidecarResponsePayload) -> bool {
        payload.kind_name() == self.as_str()
    }
}

impl RequestPayload {
    fn ownership_requirement(&self) -> OwnershipRequirement {
        match self {
            Self::Authenticate(_) | Self::OpenSession(_) | Self::CloseSession(_) => {
                OwnershipRequirement::Connection
            }
            Self::CreateVm(_)
            | Self::InitializeVm(_)
            | Self::PersistenceLoad(_)
            | Self::PersistenceFlush(_) => OwnershipRequirement::Session,
            Self::DisposeVm(_)
            | Self::BootstrapRootFilesystem(_)
            | Self::ConfigureVm(_)
            | Self::RegisterHostCallbacks(_)
            | Self::CreateLayer(_)
            | Self::SealLayer(_)
            | Self::ImportSnapshot(_)
            | Self::ExportSnapshot(_)
            | Self::CreateOverlay(_)
            | Self::GuestFilesystemCall(_)
            | Self::SnapshotRootFilesystem(_)
            | Self::Execute(_)
            | Self::WriteStdin(_)
            | Self::CloseStdin(_)
            | Self::KillProcess(_)
            | Self::GetProcessSnapshot(_)
            | Self::GetResourceSnapshot(_)
            | Self::FindListener(_)
            | Self::FindBoundUdp(_)
            | Self::VmFetch(_)
            | Self::GetSignalState(_)
            | Self::GetZombieTimerCount(_)
            | Self::GuestKernelCall(_)
            | Self::ResizePty(_)
            | Self::LinkPackage(_)
            | Self::ProvidedCommands(_)
            | Self::ScheduleCron(_)
            | Self::ListCronJobs(_)
            | Self::CancelCronJob(_)
            | Self::WakeCron(_)
            | Self::CompleteCronRun(_)
            | Self::ExportCronState(_)
            | Self::ImportCronState(_)
            | Self::HostFilesystemCall(_) => OwnershipRequirement::Vm,
            Self::Ext(_) => OwnershipRequirement::Any,
        }
    }

    fn expected_response(&self) -> ExpectedResponseKind {
        match self {
            Self::Authenticate(_) => ExpectedResponseKind::Authenticated,
            Self::OpenSession(_) => ExpectedResponseKind::SessionOpened,
            Self::CloseSession(_) => ExpectedResponseKind::SessionClosed,
            Self::CreateVm(_) => ExpectedResponseKind::VmCreated,
            Self::DisposeVm(_) => ExpectedResponseKind::VmDisposed,
            Self::BootstrapRootFilesystem(_) => ExpectedResponseKind::RootFilesystemBootstrapped,
            Self::ConfigureVm(_) => ExpectedResponseKind::VmConfigured,
            Self::RegisterHostCallbacks(_) => ExpectedResponseKind::HostCallbacksRegistered,
            Self::CreateLayer(_) => ExpectedResponseKind::LayerCreated,
            Self::SealLayer(_) => ExpectedResponseKind::LayerSealed,
            Self::ImportSnapshot(_) => ExpectedResponseKind::SnapshotImported,
            Self::ExportSnapshot(_) => ExpectedResponseKind::SnapshotExported,
            Self::CreateOverlay(_) => ExpectedResponseKind::OverlayCreated,
            Self::GuestFilesystemCall(_) => ExpectedResponseKind::GuestFilesystemResult,
            Self::SnapshotRootFilesystem(_) => ExpectedResponseKind::RootFilesystemSnapshot,
            Self::Execute(_) => ExpectedResponseKind::ProcessStarted,
            Self::WriteStdin(_) => ExpectedResponseKind::StdinWritten,
            Self::CloseStdin(_) => ExpectedResponseKind::StdinClosed,
            Self::KillProcess(_) => ExpectedResponseKind::ProcessKilled,
            Self::GetProcessSnapshot(_) => ExpectedResponseKind::ProcessSnapshot,
            Self::GetResourceSnapshot(_) => ExpectedResponseKind::ResourceSnapshot,
            Self::FindListener(_) => ExpectedResponseKind::ListenerSnapshot,
            Self::FindBoundUdp(_) => ExpectedResponseKind::BoundUdpSnapshot,
            Self::VmFetch(_) => ExpectedResponseKind::VmFetchResult,
            Self::GetSignalState(_) => ExpectedResponseKind::SignalState,
            Self::GetZombieTimerCount(_) => ExpectedResponseKind::ZombieTimerCount,
            Self::HostFilesystemCall(_) => ExpectedResponseKind::FilesystemResult,
            Self::PersistenceLoad(_) => ExpectedResponseKind::PersistenceState,
            Self::PersistenceFlush(_) => ExpectedResponseKind::PersistenceFlushed,
            Self::Ext(_) => ExpectedResponseKind::ExtResult,
            Self::GuestKernelCall(_) => ExpectedResponseKind::GuestKernelResult,
            Self::ResizePty(_) => ExpectedResponseKind::PtyResized,
            Self::LinkPackage(_) => ExpectedResponseKind::PackageLinked,
            Self::ProvidedCommands(_) => ExpectedResponseKind::ProvidedCommands,
            Self::ScheduleCron(_) => ExpectedResponseKind::CronScheduled,
            Self::ListCronJobs(_) => ExpectedResponseKind::CronJobs,
            Self::CancelCronJob(_) => ExpectedResponseKind::CronCancelled,
            Self::WakeCron(_) => ExpectedResponseKind::CronWake,
            Self::CompleteCronRun(_) => ExpectedResponseKind::CronRunCompleted,
            Self::ExportCronState(_) => ExpectedResponseKind::CronStateExported,
            Self::ImportCronState(_) => ExpectedResponseKind::CronStateImported,
            Self::InitializeVm(_) => ExpectedResponseKind::VmInitialized,
        }
    }
}

impl SidecarRequestPayload {
    fn ownership_requirement(&self) -> OwnershipRequirement {
        OwnershipRequirement::Vm
    }

    fn expected_response(&self) -> ExpectedSidecarResponseKind {
        match self {
            Self::HostCallback(_) => ExpectedSidecarResponseKind::HostCallback,
            Self::JsBridgeCall(_) => ExpectedSidecarResponseKind::JsBridge,
            Self::Ext(_) => ExpectedSidecarResponseKind::Ext,
        }
    }
}

impl ResponsePayload {
    fn ownership_requirement(&self) -> OwnershipRequirement {
        match self {
            Self::Authenticated(_) | Self::SessionOpened(_) | Self::SessionClosed(_) => {
                OwnershipRequirement::Connection
            }
            Self::VmCreated(_)
            | Self::VmInitialized(_)
            | Self::PersistenceState(_)
            | Self::PersistenceFlushed(_) => OwnershipRequirement::Session,
            Self::Rejected(_) => OwnershipRequirement::Any,
            Self::VmDisposed(_)
            | Self::RootFilesystemBootstrapped(_)
            | Self::VmConfigured(_)
            | Self::HostCallbacksRegistered(_)
            | Self::LayerCreated(_)
            | Self::LayerSealed(_)
            | Self::SnapshotImported(_)
            | Self::SnapshotExported(_)
            | Self::OverlayCreated(_)
            | Self::GuestFilesystemResult(_)
            | Self::RootFilesystemSnapshot(_)
            | Self::ProcessStarted(_)
            | Self::StdinWritten(_)
            | Self::StdinClosed(_)
            | Self::ProcessKilled(_)
            | Self::ProcessSnapshot(_)
            | Self::ResourceSnapshot(_)
            | Self::ListenerSnapshot(_)
            | Self::BoundUdpSnapshot(_)
            | Self::VmFetchResult(_)
            | Self::SignalState(_)
            | Self::ZombieTimerCount(_)
            | Self::FilesystemResult(_)
            | Self::PermissionDecision(_)
            | Self::GuestKernelResult(_)
            | Self::PtyResized(_)
            | Self::PackageLinked(_)
            | Self::ProvidedCommands(_)
            | Self::CronScheduled(_)
            | Self::CronJobs(_)
            | Self::CronCancelled(_)
            | Self::CronWake(_)
            | Self::CronRunCompleted(_)
            | Self::CronStateExported(_)
            | Self::CronStateImported(_) => OwnershipRequirement::Vm,
            Self::ExtResult(_) => OwnershipRequirement::Any,
        }
    }

    fn kind_name(&self) -> &'static str {
        match self {
            Self::Authenticated(_) => "authenticated",
            Self::SessionOpened(_) => "session_opened",
            Self::SessionClosed(_) => "session_closed",
            Self::VmCreated(_) => "vm_created",
            Self::VmDisposed(_) => "vm_disposed",
            Self::RootFilesystemBootstrapped(_) => "root_filesystem_bootstrapped",
            Self::VmConfigured(_) => "vm_configured",
            Self::HostCallbacksRegistered(_) => "host_callbacks_registered",
            Self::LayerCreated(_) => "layer_created",
            Self::LayerSealed(_) => "layer_sealed",
            Self::SnapshotImported(_) => "snapshot_imported",
            Self::SnapshotExported(_) => "snapshot_exported",
            Self::OverlayCreated(_) => "overlay_created",
            Self::GuestFilesystemResult(_) => "guest_filesystem_result",
            Self::RootFilesystemSnapshot(_) => "root_filesystem_snapshot",
            Self::ProcessStarted(_) => "process_started",
            Self::StdinWritten(_) => "stdin_written",
            Self::StdinClosed(_) => "stdin_closed",
            Self::ProcessKilled(_) => "process_killed",
            Self::ProcessSnapshot(_) => "process_snapshot",
            Self::ResourceSnapshot(_) => "resource_snapshot",
            Self::ListenerSnapshot(_) => "listener_snapshot",
            Self::BoundUdpSnapshot(_) => "bound_udp_snapshot",
            Self::VmFetchResult(_) => "vm_fetch_result",
            Self::SignalState(_) => "signal_state",
            Self::ZombieTimerCount(_) => "zombie_timer_count",
            Self::FilesystemResult(_) => "filesystem_result",
            Self::PermissionDecision(_) => "permission_decision",
            Self::PersistenceState(_) => "persistence_state",
            Self::PersistenceFlushed(_) => "persistence_flushed",
            Self::Rejected(_) => "rejected",
            Self::ExtResult(_) => "ext_result",
            Self::GuestKernelResult(_) => "guest_kernel_result",
            Self::PtyResized(_) => "pty_resized",
            Self::PackageLinked(_) => "package_linked",
            Self::ProvidedCommands(_) => "provided_commands_response",
            Self::CronScheduled(_) => "cron_scheduled",
            Self::CronJobs(_) => "cron_jobs",
            Self::CronCancelled(_) => "cron_cancelled",
            Self::CronWake(_) => "cron_wake",
            Self::CronRunCompleted(_) => "cron_run_completed",
            Self::CronStateExported(_) => "cron_state_exported",
            Self::CronStateImported(_) => "cron_state_imported",
            Self::VmInitialized(_) => "vm_initialized",
        }
    }
}

impl SidecarResponsePayload {
    fn ownership_requirement(&self) -> OwnershipRequirement {
        OwnershipRequirement::Vm
    }

    fn kind_name(&self) -> &'static str {
        match self {
            Self::HostCallbackResult(_) => "host_callback_result",
            Self::JsBridgeResult(_) => "js_bridge_result",
            Self::ExtResult(_) => "ext_result",
        }
    }
}

impl EventPayload {
    fn ownership_requirement(&self) -> OwnershipRequirement {
        match self {
            Self::Structured(_) => OwnershipRequirement::SessionOrVm,
            Self::VmLifecycle(_)
            | Self::ProcessOutput(_)
            | Self::ProcessExited(_)
            | Self::CronDispatch(_) => OwnershipRequirement::Vm,
            Self::Ext(_) => OwnershipRequirement::Any,
        }
    }
}

pub fn validate_frame(frame: &ProtocolFrame) -> Result<(), ProtocolCodecError> {
    match frame {
        ProtocolFrame::Request(request) => validate_request(request),
        ProtocolFrame::Response(response) => validate_response(response),
        ProtocolFrame::Event(event) => validate_event(event),
        ProtocolFrame::SidecarRequest(request) => validate_sidecar_request(request),
        ProtocolFrame::SidecarResponse(response) => validate_sidecar_response(response),
    }
}

fn validate_request(request: &RequestFrame) -> Result<(), ProtocolCodecError> {
    validate_schema(&request.schema)?;
    validate_request_id_direction(request.request_id, RequestDirection::Host)?;

    validate_ownership(&request.ownership)?;
    validate_requirement(request.payload.ownership_requirement(), &request.ownership)?;
    if let RequestPayload::Authenticate(authenticate) = &request.payload {
        if authenticate.auth_token.is_empty() {
            return Err(ProtocolCodecError::EmptyAuthToken);
        }
    }

    Ok(())
}

fn validate_response(response: &ResponseFrame) -> Result<(), ProtocolCodecError> {
    validate_schema(&response.schema)?;
    validate_request_id_direction(response.request_id, RequestDirection::Host)?;

    validate_ownership(&response.ownership)?;
    validate_requirement(
        response.payload.ownership_requirement(),
        &response.ownership,
    )?;
    Ok(())
}

fn validate_sidecar_request(request: &SidecarRequestFrame) -> Result<(), ProtocolCodecError> {
    validate_schema(&request.schema)?;
    validate_request_id_direction(request.request_id, RequestDirection::Sidecar)?;
    validate_ownership(&request.ownership)?;
    validate_requirement(request.payload.ownership_requirement(), &request.ownership)?;
    Ok(())
}

fn validate_sidecar_response(response: &SidecarResponseFrame) -> Result<(), ProtocolCodecError> {
    validate_schema(&response.schema)?;
    validate_request_id_direction(response.request_id, RequestDirection::Sidecar)?;
    validate_ownership(&response.ownership)?;
    validate_requirement(
        response.payload.ownership_requirement(),
        &response.ownership,
    )?;
    Ok(())
}

fn validate_event(event: &EventFrame) -> Result<(), ProtocolCodecError> {
    validate_schema(&event.schema)?;
    validate_ownership(&event.ownership)?;
    validate_requirement(event.payload.ownership_requirement(), &event.ownership)?;
    Ok(())
}

fn validate_schema(schema: &ProtocolSchema) -> Result<(), ProtocolCodecError> {
    if schema.name != PROTOCOL_NAME || schema.version != PROTOCOL_VERSION {
        return Err(ProtocolCodecError::UnsupportedSchema {
            name: schema.name.clone(),
            version: schema.version,
        });
    }

    Ok(())
}

fn validate_ownership(ownership: &OwnershipScope) -> Result<(), ProtocolCodecError> {
    match ownership {
        OwnershipScope::ConnectionOwnership(inner) => {
            validate_non_empty("connection_id", &inner.connection_id)
        }
        OwnershipScope::SessionOwnership(inner) => {
            validate_non_empty("connection_id", &inner.connection_id)?;
            validate_non_empty("session_id", &inner.session_id)
        }
        OwnershipScope::VmOwnership(inner) => {
            validate_non_empty("connection_id", &inner.connection_id)?;
            validate_non_empty("session_id", &inner.session_id)?;
            validate_non_empty("vm_id", &inner.vm_id)
        }
    }
}

fn validate_non_empty(field: &'static str, value: &str) -> Result<(), ProtocolCodecError> {
    if value.is_empty() {
        return Err(ProtocolCodecError::EmptyOwnershipField { field });
    }

    Ok(())
}

fn validate_request_id_direction(
    request_id: RequestId,
    direction: RequestDirection,
) -> Result<(), ProtocolCodecError> {
    if request_id == 0 {
        return Err(ProtocolCodecError::InvalidRequestId);
    }

    let matches_direction = match direction {
        RequestDirection::Host => request_id > 0,
        RequestDirection::Sidecar => request_id < 0,
    };
    if matches_direction {
        Ok(())
    } else {
        Err(ProtocolCodecError::InvalidRequestDirection {
            request_id,
            expected: direction,
        })
    }
}

fn validate_requirement(
    required: OwnershipRequirement,
    ownership: &OwnershipScope,
) -> Result<(), ProtocolCodecError> {
    let actual = match ownership {
        OwnershipScope::ConnectionOwnership(..) => OwnershipRequirement::Connection,
        OwnershipScope::SessionOwnership(..) => OwnershipRequirement::Session,
        OwnershipScope::VmOwnership(..) => OwnershipRequirement::Vm,
    };

    let valid = match required {
        OwnershipRequirement::Any => true,
        OwnershipRequirement::Connection => {
            matches!(ownership, OwnershipScope::ConnectionOwnership(..))
        }
        OwnershipRequirement::Session => matches!(ownership, OwnershipScope::SessionOwnership(..)),
        OwnershipRequirement::Vm => matches!(ownership, OwnershipScope::VmOwnership(..)),
        OwnershipRequirement::SessionOrVm => {
            matches!(
                ownership,
                OwnershipScope::SessionOwnership(..) | OwnershipScope::VmOwnership(..)
            )
        }
    };

    if valid {
        Ok(())
    } else {
        Err(ProtocolCodecError::InvalidOwnershipScope { required, actual })
    }
}

// ---------------------------------------------------------------------------
// JavaScript sync-RPC request types (deserialized from guest Node.js processes)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
pub struct JavascriptChildProcessSpawnOptions {
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(rename = "internalBootstrapEnv", default)]
    pub internal_bootstrap_env: BTreeMap<String, String>,
    #[serde(default)]
    pub input: Option<Value>,
    #[serde(default)]
    pub shell: bool,
    #[serde(default)]
    pub detached: bool,
    #[serde(default)]
    pub stdio: Vec<String>,
    #[serde(default)]
    pub timeout: Option<u64>,
    #[serde(rename = "killSignal", default)]
    pub kill_signal: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct JavascriptChildProcessSpawnRequest {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub options: JavascriptChildProcessSpawnOptions,
}

#[derive(Debug, Deserialize)]
pub struct JavascriptNetConnectRequest {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(rename = "localAddress", default)]
    pub local_address: Option<String>,
    #[serde(rename = "localPort", default)]
    pub local_port: Option<u16>,
    #[serde(rename = "localReservation", default)]
    pub local_reservation: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct JavascriptNetReserveTcpPortRequest {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub struct JavascriptNetListenRequest {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub backlog: Option<u32>,
    #[serde(rename = "localReservation", default)]
    pub local_reservation: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct JavascriptDgramCreateSocketRequest {
    #[serde(rename = "type")]
    pub socket_type: String,
}

#[derive(Debug, Deserialize)]
pub struct JavascriptDgramBindRequest {
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub port: u16,
}

#[derive(Debug, Deserialize)]
pub struct JavascriptDgramSendRequest {
    #[serde(default)]
    pub address: Option<String>,
    pub port: u16,
}

#[derive(Debug, Deserialize)]
pub struct JavascriptDnsLookupRequest {
    pub hostname: String,
    #[serde(default)]
    pub family: Option<u8>,
}

#[derive(Debug, Deserialize)]
pub struct JavascriptDnsResolveRequest {
    pub hostname: String,
    #[serde(default)]
    pub rrtype: Option<String>,
}
