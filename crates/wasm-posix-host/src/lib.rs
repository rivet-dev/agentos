//! Shared, fixed-table Linux/POSIX provider core for AgentOS WASM guests.
//!
//! V8-specific import construction and kernel-specific dispatch are adapters
//! around this crate. Both standalone WASM and `node-runtime.wasm` must use the
//! same generated [`SyscallId`] and validation path; guests never choose a raw
//! host syscall number or an arbitrary string operation.

use std::fmt;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

mod generated;
pub use generated::{SyscallDescriptor, SyscallId, SYSCALLS};

pub const MAX_PENDING_SYSCALLS_FIELD: &str = "limits.nodeRuntime.maxPendingSyscalls";
pub const MAX_TRANSFER_BYTES_FIELD: &str = "limits.nodeRuntime.maxTransferBytes";
pub const DEFAULT_MAX_PENDING_SYSCALLS: usize = 4_096;
pub const DEFAULT_MAX_TRANSFER_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmType {
    I32,
    I64,
    F32,
    F64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WasmValue {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl WasmValue {
    pub const fn value_type(self) -> WasmType {
        match self {
            Self::I32(_) => WasmType::I32,
            Self::I64(_) => WasmType::I64,
            Self::F32(_) => WasmType::F32,
            Self::F64(_) => WasmType::F64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    LimitExceeded {
        field: &'static str,
        configured: usize,
    },
    WrongArity {
        syscall: &'static str,
        expected: usize,
        actual: usize,
    },
    WrongType {
        syscall: &'static str,
        index: usize,
        expected: WasmType,
        actual: WasmType,
    },
    PointerOverflow {
        pointer: u32,
        length: u32,
    },
    MemoryOutOfBounds {
        pointer: u32,
        length: u32,
        memory_bytes: usize,
    },
    Kernel {
        syscall: &'static str,
        message: String,
    },
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitExceeded { field, configured } => write!(
                formatter,
                "{field}={configured} exhausted; raise the typed VM limit to admit more work"
            ),
            Self::WrongArity {
                syscall,
                expected,
                actual,
            } => write!(
                formatter,
                "{syscall} received {actual} arguments; expected {expected}"
            ),
            Self::WrongType {
                syscall,
                index,
                expected,
                actual,
            } => write!(
                formatter,
                "{syscall} argument {index} has type {actual:?}; expected {expected:?}"
            ),
            Self::PointerOverflow { pointer, length } => {
                write!(formatter, "guest range {pointer}+{length} overflows wasm32")
            }
            Self::MemoryOutOfBounds {
                pointer,
                length,
                memory_bytes,
            } => write!(
                formatter,
                "guest range {pointer}+{length} exceeds {memory_bytes}-byte linear memory"
            ),
            Self::Kernel { syscall, message } => write!(formatter, "{syscall}: {message}"),
        }
    }
}

impl std::error::Error for ProviderError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderLimits {
    pub max_pending_syscalls: usize,
    pub max_transfer_bytes: usize,
}

impl Default for ProviderLimits {
    fn default() -> Self {
        Self {
            max_pending_syscalls: DEFAULT_MAX_PENDING_SYSCALLS,
            max_transfer_bytes: DEFAULT_MAX_TRANSFER_BYTES,
        }
    }
}

/// Copy-only view of a WASM linear memory. Shared V8 memory implements this
/// with atomic byte access and never exposes a Rust slice while workers run.
pub trait LinearMemory {
    fn len(&self) -> usize;
    fn copy_in(&mut self, range: std::ops::Range<usize>, destination: &mut [u8]);
    fn copy_out(&mut self, range: std::ops::Range<usize>, source: &[u8]);

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub struct SliceMemory<'a> {
    bytes: &'a mut [u8],
}

impl<'a> SliceMemory<'a> {
    pub fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes }
    }
}

impl LinearMemory for SliceMemory<'_> {
    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn copy_in(&mut self, range: std::ops::Range<usize>, destination: &mut [u8]) {
        destination.copy_from_slice(&self.bytes[range]);
    }

    fn copy_out(&mut self, range: std::ops::Range<usize>, source: &[u8]) {
        self.bytes[range].copy_from_slice(source);
    }
}

/// A copy-only view over a live shared WebAssembly backing store.
///
/// The V8 adapter constructs a fresh view from the current `WebAssembly.Memory`
/// buffer for each import call and retains the backing-store owner until the
/// call returns. Byte-wise relaxed atomics avoid creating aliased Rust slices.
pub struct SharedLinearMemory<'a> {
    pointer: NonNull<AtomicU8>,
    len: usize,
    _backing_store_lifetime: PhantomData<&'a ()>,
}

impl<'a> SharedLinearMemory<'a> {
    /// # Safety
    ///
    /// `pointer..pointer+len` must remain allocated for `'a`, must refer to a
    /// shared WebAssembly backing store that permits atomic byte access, and
    /// the caller must retain the engine's backing-store owner for `'a`.
    pub unsafe fn from_raw_parts(pointer: NonNull<u8>, len: usize) -> Self {
        Self {
            pointer: pointer.cast(),
            len,
            _backing_store_lifetime: PhantomData,
        }
    }
}

impl LinearMemory for SharedLinearMemory<'_> {
    fn len(&self) -> usize {
        self.len
    }

    fn copy_in(&mut self, range: std::ops::Range<usize>, destination: &mut [u8]) {
        for (index, output) in destination.iter_mut().enumerate() {
            // SAFETY: GuestMemory validated the full range and construction
            // guarantees the backing store remains live.
            *output =
                unsafe { self.pointer.add(range.start + index).as_ref() }.load(Ordering::Relaxed);
        }
    }

    fn copy_out(&mut self, range: std::ops::Range<usize>, source: &[u8]) {
        for (index, byte) in source.iter().copied().enumerate() {
            // SAFETY: GuestMemory validated the full range and construction
            // guarantees the backing store remains live.
            unsafe { self.pointer.add(range.start + index).as_ref() }
                .store(byte, Ordering::Relaxed);
        }
    }
}

pub struct GuestMemory<'a> {
    memory: &'a mut dyn LinearMemory,
    max_transfer_bytes: usize,
}

impl<'a> GuestMemory<'a> {
    pub fn new(memory: &'a mut dyn LinearMemory, max_transfer_bytes: usize) -> Self {
        Self {
            memory,
            max_transfer_bytes,
        }
    }

    pub fn len(&self) -> usize {
        self.memory.len()
    }

    pub fn is_empty(&self) -> bool {
        self.memory.is_empty()
    }

    pub fn copy_in(&mut self, pointer: u32, length: u32) -> Result<Vec<u8>, ProviderError> {
        let range = self.checked_range(pointer, length)?;
        let mut destination = vec![0; range.len()];
        self.memory.copy_in(range, &mut destination);
        Ok(destination)
    }

    /// Commit only after validating the complete destination; invalid tails
    /// cannot partially mutate guest memory.
    pub fn copy_out(&mut self, pointer: u32, source: &[u8]) -> Result<(), ProviderError> {
        let length = u32::try_from(source.len()).map_err(|_| ProviderError::LimitExceeded {
            field: MAX_TRANSFER_BYTES_FIELD,
            configured: self.max_transfer_bytes,
        })?;
        let range = self.checked_range(pointer, length)?;
        self.memory.copy_out(range, source);
        Ok(())
    }

    pub fn read_u32_le(&mut self, pointer: u32) -> Result<u32, ProviderError> {
        let bytes = self.copy_in(pointer, 4)?;
        Ok(u32::from_le_bytes(
            bytes.try_into().expect("four-byte copy"),
        ))
    }

    pub fn write_u32_le(&mut self, pointer: u32, value: u32) -> Result<(), ProviderError> {
        self.copy_out(pointer, &value.to_le_bytes())
    }

    fn checked_range(
        &self,
        pointer: u32,
        length: u32,
    ) -> Result<std::ops::Range<usize>, ProviderError> {
        if length as usize > self.max_transfer_bytes {
            return Err(ProviderError::LimitExceeded {
                field: MAX_TRANSFER_BYTES_FIELD,
                configured: self.max_transfer_bytes,
            });
        }
        let end_u32 = pointer
            .checked_add(length)
            .ok_or(ProviderError::PointerOverflow { pointer, length })?;
        let start = pointer as usize;
        let end = end_u32 as usize;
        if end > self.memory.len() {
            return Err(ProviderError::MemoryOutOfBounds {
                pointer,
                length,
                memory_bytes: self.memory.len(),
            });
        }
        Ok(start..end)
    }
}

pub trait KernelDispatcher: Send {
    fn invoke(
        &mut self,
        syscall: &'static SyscallDescriptor,
        arguments: &[WasmValue],
        memory: &mut GuestMemory<'_>,
    ) -> Result<Option<WasmValue>, ProviderError>;
}

pub struct PosixProvider {
    limits: ProviderLimits,
    pending: AtomicUsize,
    warning_emitted: AtomicBool,
}

impl PosixProvider {
    pub fn new(limits: ProviderLimits) -> Result<Self, ProviderError> {
        if limits.max_pending_syscalls == 0 {
            return Err(ProviderError::LimitExceeded {
                field: MAX_PENDING_SYSCALLS_FIELD,
                configured: 0,
            });
        }
        Ok(Self {
            limits,
            pending: AtomicUsize::new(0),
            warning_emitted: AtomicBool::new(false),
        })
    }

    pub fn invoke(
        &self,
        id: SyscallId,
        arguments: &[WasmValue],
        memory: &mut dyn LinearMemory,
        kernel: &mut dyn KernelDispatcher,
    ) -> Result<Option<WasmValue>, ProviderError> {
        let descriptor = id.descriptor();
        validate_signature(descriptor, arguments)?;
        let _reservation = self.reserve()?;
        let mut memory = GuestMemory::new(memory, self.limits.max_transfer_bytes);
        let result = kernel.invoke(descriptor, arguments, &mut memory)?;
        validate_result(descriptor, result)?;
        Ok(result)
    }

    pub fn invoke_slice(
        &self,
        id: SyscallId,
        arguments: &[WasmValue],
        memory_bytes: &mut [u8],
        kernel: &mut dyn KernelDispatcher,
    ) -> Result<Option<WasmValue>, ProviderError> {
        let mut memory = SliceMemory::new(memory_bytes);
        self.invoke(id, arguments, &mut memory, kernel)
    }

    pub fn pending(&self) -> usize {
        self.pending.load(Ordering::Acquire)
    }

    fn reserve(&self) -> Result<PendingReservation<'_>, ProviderError> {
        let previous = self
            .pending
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                (pending < self.limits.max_pending_syscalls).then_some(pending + 1)
            })
            .map_err(|_| ProviderError::LimitExceeded {
                field: MAX_PENDING_SYSCALLS_FIELD,
                configured: self.limits.max_pending_syscalls,
            })?;
        let active = previous + 1;
        let warning_at = self
            .limits
            .max_pending_syscalls
            .saturating_mul(4)
            .div_ceil(5);
        if active >= warning_at && !self.warning_emitted.swap(true, Ordering::AcqRel) {
            eprintln!(
                "agentos-wasm-posix-host: {MAX_PENDING_SYSCALLS_FIELD} nearing limit: active={active} configured={}",
                self.limits.max_pending_syscalls
            );
        }
        Ok(PendingReservation { provider: self })
    }
}

struct PendingReservation<'a> {
    provider: &'a PosixProvider,
}

impl Drop for PendingReservation<'_> {
    fn drop(&mut self) {
        self.provider.pending.fetch_sub(1, Ordering::AcqRel);
    }
}

fn validate_signature(
    descriptor: &'static SyscallDescriptor,
    arguments: &[WasmValue],
) -> Result<(), ProviderError> {
    if arguments.len() != descriptor.params.len() {
        return Err(ProviderError::WrongArity {
            syscall: descriptor.name,
            expected: descriptor.params.len(),
            actual: arguments.len(),
        });
    }
    for (index, (argument, expected)) in arguments.iter().zip(descriptor.params).enumerate() {
        let actual = argument.value_type();
        if actual != *expected {
            return Err(ProviderError::WrongType {
                syscall: descriptor.name,
                index,
                expected: *expected,
                actual,
            });
        }
    }
    Ok(())
}

fn validate_result(
    descriptor: &'static SyscallDescriptor,
    result: Option<WasmValue>,
) -> Result<(), ProviderError> {
    match (descriptor.results, result) {
        ([], None) => Ok(()),
        ([expected], Some(actual)) if *expected == actual.value_type() => Ok(()),
        ([], Some(actual)) => Err(ProviderError::WrongType {
            syscall: descriptor.name,
            index: descriptor.params.len(),
            expected: WasmType::I32,
            actual: actual.value_type(),
        }),
        ([expected], Some(actual)) => Err(ProviderError::WrongType {
            syscall: descriptor.name,
            index: descriptor.params.len(),
            expected: *expected,
            actual: actual.value_type(),
        }),
        _ => Err(ProviderError::WrongArity {
            syscall: descriptor.name,
            expected: descriptor.results.len(),
            actual: usize::from(result.is_some()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoKernel;

    impl KernelDispatcher for EchoKernel {
        fn invoke(
            &mut self,
            _syscall: &'static SyscallDescriptor,
            _arguments: &[WasmValue],
            memory: &mut GuestMemory<'_>,
        ) -> Result<Option<WasmValue>, ProviderError> {
            let input = memory.copy_in(4, 4)?;
            memory.copy_out(8, &input)?;
            Ok(Some(WasmValue::I32(0)))
        }
    }

    #[test]
    fn generated_table_is_complete_and_unique() {
        assert_eq!(SYSCALLS.len(), 68);
        let mut names = SYSCALLS.iter().map(|entry| entry.name).collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), SYSCALLS.len());
        assert!(SYSCALLS
            .iter()
            .all(|entry| entry.test_id.starts_with("posix:")));
    }

    #[test]
    fn validates_signature_before_kernel_side_effects() {
        let provider = PosixProvider::new(ProviderLimits::default()).unwrap();
        let mut memory = [0_u8; 16];
        let mut kernel = EchoKernel;
        let error = provider
            .invoke_slice(SyscallId::FdWrite, &[], &mut memory, &mut kernel)
            .unwrap_err();
        assert!(matches!(error, ProviderError::WrongArity { .. }));
        assert_eq!(provider.pending(), 0);
    }

    #[test]
    fn checked_copy_rejects_wrap_oob_and_partial_write() {
        let mut bytes = [0x55_u8; 16];
        let before = bytes;
        let mut slice = SliceMemory::new(&mut bytes);
        let mut memory = GuestMemory::new(&mut slice, 8);
        assert!(matches!(
            memory.copy_in(u32::MAX - 1, 4),
            Err(ProviderError::PointerOverflow { .. })
        ));
        assert!(matches!(
            memory.copy_out(14, &[1, 2, 3, 4]),
            Err(ProviderError::MemoryOutOfBounds { .. })
        ));
        assert!(matches!(
            memory.copy_in(0, 9),
            Err(ProviderError::LimitExceeded {
                field: MAX_TRANSFER_BYTES_FIELD,
                ..
            })
        ));
        drop(memory);
        drop(slice);
        assert_eq!(bytes, before);
    }

    #[test]
    fn reservation_is_returned_after_success_and_error() {
        let provider = PosixProvider::new(ProviderLimits::default()).unwrap();
        let mut memory = [0_u8; 16];
        memory[4..8].copy_from_slice(b"test");
        let descriptor = SyscallId::FdWrite.descriptor();
        let arguments = descriptor
            .params
            .iter()
            .map(|kind| match kind {
                WasmType::I32 => WasmValue::I32(0),
                WasmType::I64 => WasmValue::I64(0),
                WasmType::F32 => WasmValue::F32(0.0),
                WasmType::F64 => WasmValue::F64(0.0),
            })
            .collect::<Vec<_>>();
        provider
            .invoke_slice(SyscallId::FdWrite, &arguments, &mut memory, &mut EchoKernel)
            .unwrap();
        assert_eq!(&memory[8..12], b"test");
        assert_eq!(provider.pending(), 0);
    }

    #[test]
    fn pending_syscall_cap_fails_before_dispatch_and_names_typed_limit() {
        let provider = PosixProvider::new(ProviderLimits {
            max_pending_syscalls: 1,
            max_transfer_bytes: DEFAULT_MAX_TRANSFER_BYTES,
        })
        .unwrap();
        let reservation = provider.reserve().unwrap();
        let error = match provider.reserve() {
            Ok(_) => panic!("second reservation unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            ProviderError::LimitExceeded {
                field: MAX_PENDING_SYSCALLS_FIELD,
                configured: 1,
            }
        );
        drop(reservation);
        assert_eq!(provider.pending(), 0);
    }

    #[test]
    fn shared_memory_copy_uses_atomic_bytes_without_exclusive_slice() {
        let bytes = (0..16)
            .map(|value| AtomicU8::new(value))
            .collect::<Vec<_>>();
        let pointer = NonNull::new(bytes.as_ptr() as *mut u8).unwrap();
        // SAFETY: `bytes` remains live for the view and contains AtomicU8 cells.
        let mut shared = unsafe { SharedLinearMemory::from_raw_parts(pointer, bytes.len()) };
        let mut memory = GuestMemory::new(&mut shared, 8);
        assert_eq!(memory.copy_in(3, 4).unwrap(), vec![3, 4, 5, 6]);
        memory.copy_out(5, &[91, 92]).unwrap();
        assert_eq!(bytes[5].load(Ordering::Relaxed), 91);
        assert_eq!(bytes[6].load(Ordering::Relaxed), 92);
    }
}
