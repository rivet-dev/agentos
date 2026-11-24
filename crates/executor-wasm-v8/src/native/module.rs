//! Module admission and the process-wide admitted-image cache.
//!
//! Every image (the first and each exec replacement) passes the locked
//! proposal profile, has its linear-memory maximum capped at the configured
//! limit, and is then compiled once per distinct byte image. V8 shares one
//! compiled module across isolates, so a command's code is compiled once per
//! process rather than once per run.

use crate::{
    extract_wasm_module_limits, record_validated_profile_digest, validated_profile_digests,
    WASM_PAGE_BYTES,
};
use agentos_executor_contract::backend::HostServiceError;
use agentos_executor_wasm_abi::{profile, StartWasmExecutionRequest, WasmPermissionTier};
use agentos_executor_wasm_host::host::ExecImportTiers;
use agentos_executor_wasm_host::imports;
use agentos_executor_wasm_host::module_info::inspect_module;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

/// Distinct admitted images retained for reuse, each holding its compiled
/// module. Commands are few and large; eviction only costs a re-admission,
/// never a failure.
const ADMITTED_IMAGE_CACHE_CAPACITY: usize = 64;

/// Image bytes the cache may retain across all entries. Each entry keeps V8's
/// copy of the module and its compiled code alive for the whole process, and
/// guests can admit images they write themselves, so the cache is bounded by
/// size as well as count. Matches the per-VM module cache it replaced.
const ADMITTED_IMAGE_CACHE_MAX_BYTES: usize = 256 * 1024 * 1024;

/// One admitted, compiled guest image.
pub struct NativeImage {
    pub(super) compiled: Arc<v8::CompiledWasmModule>,
    /// `(module, name)` of every import, in declaration order.
    pub(super) imports: Vec<(String, String)>,
    /// The permission tiers whose import sets this image satisfies.
    pub(super) tiers: ExecImportTiers,
    pub(super) has_signal_trampoline: bool,
    pub(super) has_initial_sigmask_setter: bool,
}

impl std::fmt::Debug for NativeImage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeImage")
            .field("imports", &self.imports.len())
            .finish_non_exhaustive()
    }
}

/// An admitted image plus what its callers decide on.
pub(super) struct Admitted {
    pub(super) image: Arc<NativeImage>,
    pub(super) tiers: ExecImportTiers,
    /// Whether `_start` is exported as `() -> ()`.
    pub(super) has_valid_start: bool,
    /// Whether the compiled module came from the process cache.
    pub(super) cache_hit: bool,
}

/// SHA-256 of an image's bytes, the key every cache below uses.
pub fn image_digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Profile-validate `bytes`, apply the request's memory limit, and compile it
/// in `scope`'s isolate. An image is admitted once per process and memory
/// limit; later launches only look it up, so a large command is neither
/// re-parsed nor re-hashed. With `launch_tier`, the imports must also be
/// linkable at that tier (the first image); an exec replacement instead
/// reports the tiers it satisfies for the kernel to pick. `digest` is the
/// image's [`image_digest`] when the caller already computed it.
pub(super) fn admit_image(
    scope: &mut v8::PinScope<'_, '_>,
    bytes: &[u8],
    digest: Option<[u8; 32]>,
    request: &StartWasmExecutionRequest,
    launch_tier: Option<WasmPermissionTier>,
) -> Result<Admitted, HostServiceError> {
    let digest = digest.unwrap_or_else(|| image_digest(bytes));
    let key = (digest, request.limits.max_memory_bytes);
    if let Some(entry) = admitted_images()
        .lock()
        .map_err(cache_poisoned)?
        .entries
        .iter()
        .find(|(cached, _, _)| *cached == key)
        .map(|(_, _, entry)| Arc::clone(entry))
    {
        if let Some(tier) = launch_tier {
            entry.launch[tier_index(tier)].clone()?;
        }
        return Ok(Admitted {
            tiers: entry.image.tiers,
            image: Arc::clone(&entry.image),
            has_valid_start: entry.has_valid_start,
            cache_hit: true,
        });
    }

    validate_profile_digest(bytes, digest)?;
    let capped = cap_memory_maximum(bytes, request)?;
    let bytes = capped.as_deref().unwrap_or(bytes);
    let info = inspect_module(bytes)?;
    let launch =
        ALL_TIERS.map(|tier| imports::validate_imports(info.imports.iter().cloned(), tier, false));
    // The first image reports unsupported imports the way Wasmtime does,
    // before any exec-tier classification.
    if let Some(tier) = launch_tier {
        launch[tier_index(tier)].clone()?;
    }
    let tiers = imports::exec_import_tiers(&info.imports, false)?;
    let imports = info
        .imports
        .iter()
        .map(|import| (import.module.to_owned(), import.name.to_owned()))
        .collect();
    v8::tc_scope!(let tc, scope);
    let module = v8::WasmModuleObject::compile(tc, bytes).ok_or_else(|| {
        HostServiceError::new(
            "ERR_AGENTOS_WASM_INVALID_MODULE",
            "WebAssembly module failed to compile",
        )
    })?;
    let entry = Arc::new(AdmittedEntry {
        image: Arc::new(NativeImage {
            compiled: Arc::new(module.get_compiled_module()),
            imports,
            tiers,
            has_signal_trampoline: info.has_signal_trampoline,
            has_initial_sigmask_setter: info.has_initial_sigmask_setter,
        }),
        has_valid_start: info.has_valid_start,
        launch,
    });
    let size = bytes.len();
    let mut cache = admitted_images().lock().map_err(cache_poisoned)?;
    if size <= ADMITTED_IMAGE_CACHE_MAX_BYTES
        && !cache.entries.iter().any(|(cached, _, _)| *cached == key)
    {
        while cache.entries.len() >= ADMITTED_IMAGE_CACHE_CAPACITY
            || cache.bytes + size > ADMITTED_IMAGE_CACHE_MAX_BYTES
        {
            let Some((_, evicted, _)) = cache.entries.pop_front() else {
                break;
            };
            cache.bytes -= evicted;
        }
        cache.bytes += size;
        cache.entries.push_back((key, size, Arc::clone(&entry)));
    }
    Ok(Admitted {
        tiers,
        image: Arc::clone(&entry.image),
        has_valid_start: entry.has_valid_start,
        cache_hit: false,
    })
}

const ALL_TIERS: [WasmPermissionTier; 4] = [
    WasmPermissionTier::Isolated,
    WasmPermissionTier::ReadOnly,
    WasmPermissionTier::ReadWrite,
    WasmPermissionTier::Full,
];

fn tier_index(tier: WasmPermissionTier) -> usize {
    match tier {
        WasmPermissionTier::Isolated => 0,
        WasmPermissionTier::ReadOnly => 1,
        WasmPermissionTier::ReadWrite => 2,
        WasmPermissionTier::Full => 3,
    }
}

/// What admission learned about one image under one memory limit.
struct AdmittedEntry {
    image: Arc<NativeImage>,
    has_valid_start: bool,
    /// The launch import check at each tier, in [`ALL_TIERS`] order.
    launch: [Result<(), HostServiceError>; 4],
}

pub(crate) fn validate_profile(bytes: &[u8]) -> Result<[u8; 32], HostServiceError> {
    let digest = image_digest(bytes);
    validate_profile_digest(bytes, digest)?;
    Ok(digest)
}

fn validate_profile_digest(bytes: &[u8], digest: [u8; 32]) -> Result<(), HostServiceError> {
    let known = validated_profile_digests()
        .lock()
        .map_err(|_| {
            HostServiceError::new(
                "ERR_AGENTOS_WASM_PROFILE_CACHE_POISONED",
                "validated profile digest cache lock is poisoned",
            )
        })?
        .contains(&digest);
    if !known {
        profile::validate_locked_profile(bytes)?;
        record_validated_profile_digest(digest);
    }
    Ok(())
}

/// An image digest and the memory limit it was admitted under.
type AdmittedKey = ([u8; 32], Option<u64>);

/// Admitted images in insertion order, with their byte sizes and total.
#[derive(Default)]
struct AdmittedCache {
    entries: VecDeque<(AdmittedKey, usize, Arc<AdmittedEntry>)>,
    bytes: usize,
}

fn admitted_images() -> &'static Mutex<AdmittedCache> {
    static CACHE: OnceLock<Mutex<AdmittedCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(AdmittedCache::default()))
}

fn cache_poisoned<T>(_: T) -> HostServiceError {
    HostServiceError::new(
        "ERR_AGENTOS_WASM_MODULE_CACHE_POISONED",
        "admitted image cache lock is poisoned",
    )
}

fn memory_limit_error(message: String) -> HostServiceError {
    HostServiceError::new("ERR_AGENTOS_WASM_MEMORY_LIMIT", message)
        .with_details(serde_json::json!({ "limitName": "limits.resources.maxWasmMemoryBytes" }))
}

/// V8 has no per-store memory limiter, so the configured limit becomes each
/// memory's declared maximum. Returns `None` when no limit is configured.
fn cap_memory_maximum(
    bytes: &[u8],
    request: &StartWasmExecutionRequest,
) -> Result<Option<Vec<u8>>, HostServiceError> {
    let Some(limit_bytes) = request.limits.max_memory_bytes else {
        return Ok(None);
    };
    let limits = extract_wasm_module_limits(bytes).map_err(|error| {
        HostServiceError::new("ERR_AGENTOS_WASM_INVALID_MODULE", error.to_string())
    })?;
    if limits.imports_memory {
        return Err(memory_limit_error(String::from(
            "configured WebAssembly memory limit does not support imported memories",
        )));
    }
    let limit_pages = limit_bytes / WASM_PAGE_BYTES;
    rewrite_memory_section(bytes, limit_pages).map(Some)
}

fn rewrite_memory_section(bytes: &[u8], limit_pages: u64) -> Result<Vec<u8>, HostServiceError> {
    let invalid = |message: &str| {
        HostServiceError::new("ERR_AGENTOS_WASM_INVALID_MODULE", message.to_owned())
    };
    if bytes.len() < 8 || &bytes[..4] != b"\0asm" {
        return Err(invalid("module is not a valid WebAssembly binary"));
    }
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(&bytes[..8]);
    let mut offset = 8;
    while offset < bytes.len() {
        let section_start = offset;
        let id = bytes[offset];
        offset += 1;
        let size =
            read_varuint(bytes, &mut offset).ok_or_else(|| invalid("section size truncated"))?;
        let end = offset
            .checked_add(usize::try_from(size).map_err(|_| invalid("section size overflow"))?)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| invalid("section extends past end of module"))?;
        if id != 5 {
            out.extend_from_slice(&bytes[section_start..end]);
            offset = end;
            continue;
        }
        let mut cursor = offset;
        let count =
            read_varuint(bytes, &mut cursor).ok_or_else(|| invalid("memory count truncated"))?;
        let mut section = Vec::new();
        write_varuint(&mut section, count);
        for _ in 0..count {
            let flags = read_varuint(bytes, &mut cursor)
                .ok_or_else(|| invalid("memory flags truncated"))?;
            if flags & !1 != 0 {
                return Err(memory_limit_error(format!(
                    "configured WebAssembly memory limit does not support memory flags {flags}"
                )));
            }
            let initial = read_varuint(bytes, &mut cursor)
                .ok_or_else(|| invalid("memory minimum truncated"))?;
            let maximum = if flags & 1 != 0 {
                Some(
                    read_varuint(bytes, &mut cursor)
                        .ok_or_else(|| invalid("memory maximum truncated"))?,
                )
            } else {
                None
            };
            if initial > limit_pages {
                return Err(memory_limit_error(String::from(
                    "WebAssembly initial memory exceeds the configured linear-memory limit",
                )));
            }
            write_varuint(&mut section, 1);
            write_varuint(&mut section, initial);
            write_varuint(
                &mut section,
                maximum.map_or(limit_pages, |max| max.min(limit_pages)),
            );
        }
        if cursor != end {
            return Err(invalid(
                "memory section parsing did not consume the full section",
            ));
        }
        out.push(5);
        write_varuint(&mut out, section.len() as u64);
        out.extend_from_slice(&section);
        offset = end;
    }
    Ok(out)
}

fn read_varuint(bytes: &[u8], offset: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.get(*offset)?;
        *offset += 1;
        value |= u64::from(byte & 0x7f).checked_shl(shift)?;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn write_varuint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_section(bytes: &[u8]) -> Vec<u8> {
        let mut offset = 8;
        while offset < bytes.len() {
            let id = bytes[offset];
            offset += 1;
            let size = read_varuint(bytes, &mut offset).unwrap() as usize;
            if id == 5 {
                return bytes[offset..offset + size].to_vec();
            }
            offset += size;
        }
        panic!("no memory section");
    }

    #[test]
    fn unbounded_memory_gets_the_limit_as_maximum() {
        let bytes = wat::parse_str("(module (memory 1))").unwrap();
        let rewritten = rewrite_memory_section(&bytes, 4).unwrap();
        assert_eq!(memory_section(&rewritten), vec![1, 1, 1, 4]);
    }

    #[test]
    fn smaller_declared_maximum_is_kept() {
        let bytes = wat::parse_str("(module (memory 1 2))").unwrap();
        let rewritten = rewrite_memory_section(&bytes, 300).unwrap();
        assert_eq!(memory_section(&rewritten), vec![1, 1, 1, 2]);
        let mut large = Vec::new();
        write_varuint(&mut large, 300);
        assert_eq!(large, vec![0xac, 0x02]);
    }

    #[test]
    fn initial_memory_over_the_limit_fails() {
        let bytes = wat::parse_str("(module (memory 8))").unwrap();
        let error = rewrite_memory_section(&bytes, 4).unwrap_err();
        assert_eq!(error.code, "ERR_AGENTOS_WASM_MEMORY_LIMIT");
    }
}
