//! Standalone WebAssembly on V8.
//!
//! Guest modules are instantiated from Rust in their own isolate, and every
//! host import is a Rust callback running the engine-neutral handlers in
//! `agentos-executor-wasm-host`, the same ones the Wasmtime executor binds.
//! See `native` for the execution and `docs/design/v8-native-wasm-imports.md`
//! for why agentOS binds V8's WebAssembly engine directly.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

mod native;
pub use native::{preflight_native_image, NativeWasmV8Execution, PreloadedImage};

pub use agentos_executor_wasm_abi::{
    detect_native_binary_format, guest_visible_wasm_env, CreateWasmContextRequest,
    NativeBinaryFormat, StandaloneWasmBackend, StartWasmExecutionRequest, WasmContext,
    WasmExecutionError, WasmExecutionEvent, WasmExecutionLimits, WasmExecutionResult,
    WasmPermissionTier,
};

const WASM_PAGE_BYTES: u64 = 65_536;
const MAX_WASM_IMPORT_SECTION_ENTRIES: usize = 16_384;
const MAX_WASM_MEMORY_SECTION_ENTRIES: usize = 1_024;
const MAX_WASM_VARUINT_BYTES: usize = 10;
const WASM_VALIDATED_PROFILE_DIGEST_CAPACITY: usize = 64;

/// Default V8 heap cap (MB) for the isolate running a WebAssembly guest,
/// tunable through typed `limits.wasm.runnerHeapLimitMb`.
///
/// Guest linear memory is not on this heap; it is bounded by the memory
/// maximum the executor writes into the module from `request.limits`. The
/// heap holds V8's own compilation and instantiation state, which grows with
/// module size, so the ceiling stays generous. It is committed only as used,
/// and the near-heap-limit guard terminates only the offending isolate.
const DEFAULT_WASM_ISOLATE_HEAP_LIMIT_MB: u32 = 2048;

// The locked WebAssembly feature profile is fixed for this process. Only exact
// bytes that passed the profile enter this bounded digest cache; failed modules
// are checked again.
fn validated_profile_digests() -> &'static Mutex<VecDeque<[u8; 32]>> {
    static DIGESTS: OnceLock<Mutex<VecDeque<[u8; 32]>>> = OnceLock::new();
    DIGESTS.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn record_validated_profile_digest(digest: [u8; 32]) {
    let mut digests = validated_profile_digests()
        .lock()
        .expect("validated profile digest cache lock poisoned");
    if digests.contains(&digest) {
        return;
    }
    if digests.len() == WASM_VALIDATED_PROFILE_DIGEST_CAPACITY * 4 / 5 {
        tracing::warn!(
            cache = "wasm.validatedProfileDigests",
            retained = digests.len(),
            capacity = WASM_VALIDATED_PROFILE_DIGEST_CAPACITY,
            "validated wasm profile digest FIFO near capacity; module admission is unaffected; evicted modules are validated again"
        );
    }
    if digests.len() == WASM_VALIDATED_PROFILE_DIGEST_CAPACITY {
        digests.pop_front();
        tracing::warn!(
            cache = "wasm.validatedProfileDigests",
            retained = digests.len(),
            capacity = WASM_VALIDATED_PROFILE_DIGEST_CAPACITY,
            "evicting oldest validated wasm profile digest; module admission is unaffected; this module will be validated again when needed"
        );
    }
    digests.push_back(digest);
}

fn reject_v8_deterministic_fuel(
    request: &StartWasmExecutionRequest,
) -> Result<(), WasmExecutionError> {
    match request.limits.deterministic_fuel {
        Some(fuel) => Err(WasmExecutionError::DeterministicFuelUnsupported { fuel }),
        None => Ok(()),
    }
}

/// Resolve the per-execution WASM stack cap from the typed wire limit. The V8
/// runner currently has no enforceable per-module stack lever, so every configured
/// value fails closed with a typed error that names the requested bound.
fn resolve_wasm_stack_limit_bytes(
    request: &StartWasmExecutionRequest,
) -> Result<Option<u64>, WasmExecutionError> {
    match request.limits.max_stack_bytes {
        Some(0) => Err(WasmExecutionError::InvalidLimit(String::from(
            "wasm max stack bytes must be greater than zero",
        ))),
        Some(limit) => Err(WasmExecutionError::InvalidLimit(format!(
            "configured wasm max stack byte limit {limit} cannot be enforced by the V8 engine"
        ))),
        None => Ok(None),
    }
}

#[derive(Debug, Default)]
struct WasmModuleLimits {
    imports_memory: bool,
    initial_memory_bytes: Option<u64>,
    maximum_memory_bytes: Option<u64>,
}

fn extract_wasm_module_limits(bytes: &[u8]) -> Result<WasmModuleLimits, WasmExecutionError> {
    if bytes.len() < 8 || &bytes[..4] != b"\0asm" {
        return Err(WasmExecutionError::InvalidModule(String::from(
            "module is not a valid WebAssembly binary",
        )));
    }

    let mut offset = 8;
    let mut limits = WasmModuleLimits::default();

    while offset < bytes.len() {
        let section_id = bytes[offset];
        offset += 1;
        let section_size = read_varuint_usize(bytes, &mut offset, "section size")?;
        let section_end = offset.checked_add(section_size).ok_or_else(|| {
            WasmExecutionError::InvalidModule(String::from("section size overflow"))
        })?;
        if section_end > bytes.len() {
            return Err(WasmExecutionError::InvalidModule(String::from(
                "section extends past end of module",
            )));
        }

        match section_id {
            2 => {
                let mut cursor = offset;
                let import_count = read_varuint_usize(bytes, &mut cursor, "import count")?;
                if import_count > MAX_WASM_IMPORT_SECTION_ENTRIES {
                    return Err(WasmExecutionError::InvalidModule(format!(
                        "import section contains {import_count} entries, which exceeds the parser cap of {MAX_WASM_IMPORT_SECTION_ENTRIES}"
                    )));
                }
                for _ in 0..import_count {
                    skip_name(bytes, &mut cursor)?;
                    skip_name(bytes, &mut cursor)?;
                    let kind = read_byte(bytes, &mut cursor)?;
                    match kind {
                        0x02 => {
                            let _ = read_memory_limits(bytes, &mut cursor)?;
                            limits.imports_memory = true;
                        }
                        0x00 => {
                            let _ = read_varuint(bytes, &mut cursor)?;
                        }
                        0x01 => {
                            skip_table_type(bytes, &mut cursor)?;
                        }
                        0x03 => {
                            let _ = read_byte(bytes, &mut cursor)?;
                            let _ = read_byte(bytes, &mut cursor)?;
                        }
                        other => {
                            return Err(WasmExecutionError::InvalidModule(format!(
                                "unsupported import kind {other}"
                            )));
                        }
                    }
                }
            }
            5 => {
                let mut cursor = offset;
                let memory_count = read_varuint_usize(bytes, &mut cursor, "memory count")?;
                if memory_count > MAX_WASM_MEMORY_SECTION_ENTRIES {
                    return Err(WasmExecutionError::InvalidModule(format!(
                        "memory section contains {memory_count} entries, which exceeds the parser cap of {MAX_WASM_MEMORY_SECTION_ENTRIES}"
                    )));
                }
                if memory_count > 0 {
                    let (initial_pages, maximum_pages) = read_memory_limits(bytes, &mut cursor)?;
                    limits.initial_memory_bytes =
                        Some(initial_pages.saturating_mul(WASM_PAGE_BYTES));
                    limits.maximum_memory_bytes =
                        maximum_pages.map(|pages| pages.saturating_mul(WASM_PAGE_BYTES));
                }
            }
            _ => {}
        }

        offset = section_end;
    }

    Ok(limits)
}

fn read_memory_limits(
    bytes: &[u8],
    offset: &mut usize,
) -> Result<(u64, Option<u64>), WasmExecutionError> {
    let flags = read_varuint(bytes, offset)?;
    let initial = read_varuint(bytes, offset)?;
    let maximum = if flags & 0x01 != 0 {
        Some(read_varuint(bytes, offset)?)
    } else {
        None
    };
    Ok((initial, maximum))
}

fn skip_name(bytes: &[u8], offset: &mut usize) -> Result<(), WasmExecutionError> {
    let length = read_varuint_usize(bytes, offset, "name length")?;
    let end = offset
        .checked_add(length)
        .ok_or_else(|| WasmExecutionError::InvalidModule(String::from("name length overflow")))?;
    if end > bytes.len() {
        return Err(WasmExecutionError::InvalidModule(String::from(
            "name extends past end of module",
        )));
    }
    *offset = end;
    Ok(())
}

fn skip_table_type(bytes: &[u8], offset: &mut usize) -> Result<(), WasmExecutionError> {
    let _ = read_byte(bytes, offset)?;
    let flags = read_varuint(bytes, offset)?;
    let _ = read_varuint(bytes, offset)?;
    if flags & 0x01 != 0 {
        let _ = read_varuint(bytes, offset)?;
    }
    Ok(())
}

fn read_byte(bytes: &[u8], offset: &mut usize) -> Result<u8, WasmExecutionError> {
    let Some(byte) = bytes.get(*offset).copied() else {
        return Err(WasmExecutionError::InvalidModule(String::from(
            "unexpected end of module",
        )));
    };
    *offset += 1;
    Ok(byte)
}

fn read_varuint(bytes: &[u8], offset: &mut usize) -> Result<u64, WasmExecutionError> {
    let mut shift = 0_u32;
    let mut value = 0_u64;
    let mut encoded_bytes = 0_usize;

    loop {
        let byte = read_byte(bytes, offset)?;
        encoded_bytes += 1;
        if encoded_bytes > MAX_WASM_VARUINT_BYTES {
            return Err(WasmExecutionError::InvalidModule(format!(
                "varuint exceeds the parser cap of {MAX_WASM_VARUINT_BYTES} bytes"
            )));
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        if encoded_bytes == MAX_WASM_VARUINT_BYTES {
            return Err(WasmExecutionError::InvalidModule(format!(
                "varuint exceeds the parser cap of {MAX_WASM_VARUINT_BYTES} bytes"
            )));
        }
        shift = shift.saturating_add(7);
        if shift >= 64 {
            return Err(WasmExecutionError::InvalidModule(String::from(
                "varuint is too large",
            )));
        }
    }
}

fn read_varuint_usize(
    bytes: &[u8],
    offset: &mut usize,
    label: &str,
) -> Result<usize, WasmExecutionError> {
    let value = read_varuint(bytes, offset)?;
    usize::try_from(value).map_err(|_| {
        WasmExecutionError::InvalidModule(format!(
            "{label} of {value} exceeds platform usize range"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validated_profile_digests_stay_bounded_and_evict_oldest_first() {
        let digests = (0..=WASM_VALIDATED_PROFILE_DIGEST_CAPACITY)
            .map(|index| {
                let mut digest = [0u8; 32];
                digest[..8].copy_from_slice(&(index as u64 + 0xA000).to_le_bytes());
                digest
            })
            .collect::<Vec<_>>();
        for digest in &digests {
            record_validated_profile_digest(*digest);
            record_validated_profile_digest(*digest);
        }
        let retained = validated_profile_digests().lock().unwrap();
        assert!(retained.len() <= WASM_VALIDATED_PROFILE_DIGEST_CAPACITY);
        assert!(!retained.contains(&digests[0]), "oldest digest is evicted");
        assert!(retained.contains(digests.last().unwrap()));
    }

    #[test]
    fn configured_stack_limits_fail_closed() {
        let mut request = StartWasmExecutionRequest {
            vm_id: String::new(),
            context_id: String::new(),
            managed_kernel_host: true,
            argv: Vec::new(),
            env: Default::default(),
            cwd: Default::default(),
            permission_tier: WasmPermissionTier::Full,
            limits: WasmExecutionLimits::default(),
            guest_runtime: Default::default(),
        };
        assert!(resolve_wasm_stack_limit_bytes(&request).unwrap().is_none());
        request.limits.max_stack_bytes = Some(0);
        assert!(resolve_wasm_stack_limit_bytes(&request).is_err());
        request.limits.max_stack_bytes = Some(1 << 20);
        assert!(resolve_wasm_stack_limit_bytes(&request).is_err());
    }
}
