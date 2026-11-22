//! Wasmtime guest-memory access for the shared host-import codecs.

use super::store::WasmtimeStoreState;
use std::ops::Range;
use std::sync::atomic::{AtomicU8, Ordering};
use wasmtime::{Caller, Extern, Memory, SharedMemory};

pub use agentos_executor_wasm_host::memory::GuestMemoryError;

#[derive(Clone)]
pub enum GuestMemory {
    Local(Memory),
    Shared(SharedMemory),
}

impl GuestMemory {
    pub fn data_size(&self, caller: &Caller<'_, WasmtimeStoreState>) -> usize {
        match self {
            Self::Local(memory) => memory.data_size(caller),
            Self::Shared(memory) => memory.data_size(),
        }
    }

    pub fn read(&self, caller: &Caller<'_, WasmtimeStoreState>, range: Range<usize>) -> Vec<u8> {
        match self {
            Self::Local(memory) => memory.data(caller)[range].to_vec(),
            Self::Shared(memory) => memory.data()[range]
                .iter()
                .map(|byte| {
                    // SAFETY: `AtomicU8` has alignment one and the Wasmtime
                    // shared-memory API guarantees the backing allocation
                    // remains valid. Atomic access is required because guest
                    // threads may concurrently mutate these bytes.
                    unsafe { &*byte.get().cast::<AtomicU8>() }.load(Ordering::SeqCst)
                })
                .collect(),
        }
    }

    pub fn write(
        &self,
        caller: &mut Caller<'_, WasmtimeStoreState>,
        range: Range<usize>,
        bytes: &[u8],
    ) {
        match self {
            Self::Local(memory) => memory.data_mut(caller)[range].copy_from_slice(bytes),
            Self::Shared(memory) => {
                for (destination, source) in memory.data()[range].iter().zip(bytes) {
                    // SAFETY: see the corresponding shared-memory read. Each
                    // byte is stored atomically so Rust never races with a
                    // guest load/store on another native worker.
                    unsafe { &*destination.get().cast::<AtomicU8>() }
                        .store(*source, Ordering::SeqCst);
                }
            }
        }
    }
}

pub fn exported_memory(
    caller: &mut Caller<'_, WasmtimeStoreState>,
) -> Result<GuestMemory, GuestMemoryError> {
    match caller.get_export("memory") {
        Some(Extern::Memory(memory)) => Ok(GuestMemory::Local(memory)),
        Some(Extern::SharedMemory(memory)) => Ok(GuestMemory::Shared(memory)),
        _ => Err(GuestMemoryError::MissingMemory),
    }
}

