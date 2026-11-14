//! Script sources shared by every isolate as external one-byte strings.
//!
//! V8 keeps a script's source for lazy compilation and `Function#toString`.
//! Restored from a snapshot, every isolate gets its own heap copy of each
//! source, and the bridge alone is ~2 MB. A snapshot instead compiles large
//! sources from a process-lifetime external string resource (a
//! `v8::OneByteConst` for Latin-1 text, a C++ two-byte resource otherwise)
//! whose address is in the isolate's external reference table. V8 serializes
//! such a string as a reference, so each restored isolate points at the one
//! process-wide copy.
//!
//! The choice to share a source depends on its content only, so the snapshot
//! helper and the restoring sidecar always build the same reference table.
//!
//! Lifetime: a snapshot blob owns [`SnapshotSources`], and each isolate
//! restored from it keeps that set alive. Source bytes that no blob or
//! isolate uses anymore are freed, oldest first, when a new source would pass
//! [`MAX_SHARED_SOURCE_BYTES`]. The small resource header V8 sees (a
//! `OneByteConst` or a C++ resource) is never freed: rusty_v8 drops an
//! isolate's allocations before V8 disposes the isolate, and V8 may still call
//! the header then. Headers cost a few dozen bytes per distinct source.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use sha2::{Digest, Sha256};

/// Sources below this size are cheaper to copy than to share.
pub const MIN_SHARED_SOURCE_BYTES: usize = 64 * 1024;
/// Bound on source bytes copied into the process to be shared. Sources the
/// embedder registers as `'static` do not count.
pub const MAX_SHARED_SOURCE_BYTES: usize = 64 * 1024 * 1024;
pub const SHARED_SOURCE_LIMIT_ERROR_CODE: &str = "ERR_AGENTOS_V8_SHARED_SOURCE_LIMIT";

type Digest32 = [u8; 32];

extern "C" {
    fn agentos_v8_static_twobyte_resource(data: *const u16, length: usize) -> *mut c_void;
    fn agentos_v8_new_static_twobyte_string(
        isolate: *mut v8::Isolate,
        resource: *mut c_void,
    ) -> *const v8::String;
}

/// A process-lifetime external string resource for one script source.
#[derive(Clone, Copy)]
pub(crate) enum SharedSource {
    /// ASCII or Latin-1 text.
    OneByte(&'static v8::OneByteConst),
    /// UTF-16 text behind a C++ resource header that is never freed.
    TwoByte(*mut c_void),
}

// SAFETY: both variants point at leaked, immutable headers; their text lives
// as long as any holder of its `SourceBacking`.
unsafe impl Send for SharedSource {}
unsafe impl Sync for SharedSource {}

impl SharedSource {
    /// The address V8 matches against the external reference table.
    pub(crate) fn address(self) -> *mut c_void {
        match self {
            Self::OneByte(constant) => constant as *const v8::OneByteConst as *mut c_void,
            Self::TwoByte(resource) => resource,
        }
    }

    fn new_string<'s>(
        self,
        scope: &mut v8::HandleScope<'s, ()>,
    ) -> Option<v8::Local<'s, v8::String>> {
        match self {
            Self::OneByte(constant) => v8::String::new_from_onebyte_const(scope, constant),
            Self::TwoByte(resource) => {
                let isolate: &mut v8::Isolate = scope.as_mut();
                // SAFETY: the resource came from `agentos_v8_static_twobyte_resource`
                // and is never freed. The string is created in `scope`'s handle
                // scope, and `Local` is a `repr(C)` wrapper of the handle pointer.
                unsafe {
                    let raw = agentos_v8_new_static_twobyte_string(isolate, resource);
                    let raw = std::ptr::NonNull::new(raw as *mut v8::String)?;
                    Some(std::mem::transmute::<
                        std::ptr::NonNull<v8::String>,
                        v8::Local<'s, v8::String>,
                    >(raw))
                }
            }
        }
    }
}

/// Owned copy of a source's text; V8 reads it through the source's header.
pub(crate) struct SourceBacking {
    _bytes: Box<[u8]>,
    _units: Box<[u16]>,
}

struct Entry {
    source: SharedSource,
    /// `None` for a `'static` source.
    backing: Option<Arc<SourceBacking>>,
    copied: usize,
    /// Snapshot creation compiles from the source in a live isolate that
    /// holds no [`SnapshotSources`], so such a source is never freed.
    pinned: bool,
    last_used: u64,
}

#[derive(Default)]
struct Interner {
    by_digest: HashMap<Digest32, Entry>,
    copied_bytes: usize,
    clock: u64,
}

impl Interner {
    /// Free unused sources, oldest first, until `needed` more bytes fit.
    fn make_room(&mut self, needed: usize) -> bool {
        while self.copied_bytes.saturating_add(needed) > MAX_SHARED_SOURCE_BYTES {
            let victim = self
                .by_digest
                .iter()
                .filter(|(_, entry)| {
                    !entry.pinned
                        && entry
                            .backing
                            .as_ref()
                            .is_some_and(|backing| Arc::strong_count(backing) == 1)
                })
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(digest, _)| *digest);
            let Some(digest) = victim else {
                return false;
            };
            if let Some(entry) = self.by_digest.remove(&digest) {
                self.copied_bytes -= entry.copied;
            }
        }
        true
    }
}

fn interner() -> &'static Mutex<Interner> {
    static INTERNER: OnceLock<Mutex<Interner>> = OnceLock::new();
    INTERNER.get_or_init(|| Mutex::new(Interner::default()))
}

/// Whether a snapshot compiles `source` as a shared external string. The
/// rule depends only on the content, so the snapshot helper and the
/// restoring sidecar build the same external reference table.
pub(crate) fn is_shareable(source: &str) -> bool {
    source.len() >= MIN_SHARED_SOURCE_BYTES
}

/// Encode `source` for V8. The header is leaked (see the module docs); the
/// text is copied into the returned backing unless it is an ASCII `'static`
/// source.
fn make_shared(
    source: &str,
    static_text: Option<&'static str>,
) -> (SharedSource, Option<Arc<SourceBacking>>, usize) {
    // SAFETY (for each `'static` view below): the backing owns the bytes, and
    // every user of the header holds the backing (the interner entry, a
    // `SnapshotSources`, or a pinned entry for a live snapshot creator).
    if source.is_ascii() {
        if let Some(text) = static_text {
            let constant = Box::leak(Box::new(v8::String::create_external_onebyte_const(
                text.as_bytes(),
            )));
            return (SharedSource::OneByte(constant), None, 0);
        }
        let bytes: Box<[u8]> = source.as_bytes().into();
        let view: &'static [u8] =
            unsafe { std::slice::from_raw_parts(bytes.as_ptr(), bytes.len()) };
        let constant = Box::leak(Box::new(v8::String::create_external_onebyte_const(view)));
        let copied = bytes.len();
        let backing = SourceBacking {
            _bytes: bytes,
            _units: Box::new([]),
        };
        return (
            SharedSource::OneByte(constant),
            Some(Arc::new(backing)),
            copied,
        );
    }
    if source.chars().all(|character| (character as u32) <= 0xff) {
        let latin1: Box<[u8]> = source.chars().map(|character| character as u8).collect();
        let view: &'static [u8] =
            unsafe { std::slice::from_raw_parts(latin1.as_ptr(), latin1.len()) };
        // SAFETY: Latin-1 bytes are valid one-byte V8 string content; the
        // length is far below V8's string limit (sources are bounded).
        let constant = Box::leak(Box::new(unsafe {
            v8::String::create_external_onebyte_const_unchecked(view)
        }));
        let copied = latin1.len();
        let backing = SourceBacking {
            _bytes: latin1,
            _units: Box::new([]),
        };
        return (
            SharedSource::OneByte(constant),
            Some(Arc::new(backing)),
            copied,
        );
    }
    let units: Box<[u16]> = source.encode_utf16().collect();
    let copied = units.len() * 2;
    // SAFETY: the backing owns `units` for as long as the resource is used.
    let resource = unsafe { agentos_v8_static_twobyte_resource(units.as_ptr(), units.len()) };
    let backing = SourceBacking {
        _bytes: Box::new([]),
        _units: units,
    };
    (
        SharedSource::TwoByte(resource),
        Some(Arc::new(backing)),
        copied,
    )
}

/// Register a `'static` source (for example the embedded bridge) so sharing
/// it costs no copy when it is ASCII, and does not count toward the bound.
pub fn register_static_source(source: &'static str) {
    if !is_shareable(source) {
        return;
    }
    let digest: Digest32 = Sha256::digest(source.as_bytes()).into();
    let mut interner = interner().lock().expect("shared source interner poisoned");
    interner.by_digest.entry(digest).or_insert_with(|| {
        let (source, backing, copied) = make_shared(source, Some(source));
        Entry {
            source,
            backing,
            copied,
            pinned: true,
            last_used: 0,
        }
    });
}

/// The process-wide resource for a shareable `source`, and the backing that
/// keeps it valid. `pin` keeps the source for the process lifetime.
fn shared_source_entry(
    source: &str,
    pin: bool,
) -> Result<(SharedSource, Option<Arc<SourceBacking>>), String> {
    debug_assert!(is_shareable(source));
    let digest: Digest32 = Sha256::digest(source.as_bytes()).into();
    let mut interner = interner().lock().expect("shared source interner poisoned");
    interner.clock += 1;
    let now = interner.clock;
    if let Some(entry) = interner.by_digest.get_mut(&digest) {
        entry.last_used = now;
        entry.pinned |= pin;
        return Ok((entry.source, entry.backing.clone()));
    }
    if !interner.make_room(source.len()) {
        return Err(format!(
            "{SHARED_SOURCE_LIMIT_ERROR_CODE}: shared V8 script sources of snapshots still in use would exceed {MAX_SHARED_SOURCE_BYTES} bytes; sources are freed as their snapshots leave the snapshot cache and their isolates exit"
        ));
    }
    let (shared, backing, copied) = make_shared(source, None);
    interner.copied_bytes += copied;
    interner.by_digest.insert(
        digest,
        Entry {
            source: shared,
            backing: backing.clone(),
            copied,
            pinned: pin,
            last_used: now,
        },
    );
    Ok((shared, backing))
}

/// Shared sources and the external reference table of one snapshot. A
/// snapshot blob owns it and each restored isolate keeps it alive.
pub(crate) struct SnapshotSources {
    refs: SnapshotRefs,
    _backings: Vec<Arc<SourceBacking>>,
}

enum SnapshotRefs {
    Base(&'static v8::ExternalReferences),
    Shared(Arc<v8::ExternalReferences>),
}

impl SnapshotSources {
    /// The external references for restoring this snapshot.
    pub(crate) fn refs(&self) -> &v8::ExternalReferences {
        match &self.refs {
            SnapshotRefs::Base(refs) => refs,
            SnapshotRefs::Shared(refs) => refs,
        }
    }
}

/// Shared sources and the external reference table for a snapshot that
/// compiles `sources` (each shareable), in order.
pub(crate) fn snapshot_sources(sources: &[&str]) -> Result<SnapshotSources, String> {
    let mut shared = Vec::with_capacity(sources.len());
    let mut backings = Vec::with_capacity(sources.len());
    for source in sources {
        let (source, backing) = shared_source_entry(source, false)?;
        shared.push(source);
        backings.extend(backing);
    }
    Ok(SnapshotSources {
        refs: external_refs_with_sources(&shared),
        _backings: backings,
    })
}

/// A V8 string for script `source`: shared when it is shareable, a heap copy
/// otherwise. Only snapshot creation should compile from shared strings; the
/// resource must be in the creator's external reference table.
pub(crate) fn snapshot_source_string<'s>(
    scope: &mut v8::HandleScope<'s, ()>,
    source: &str,
) -> Result<v8::Local<'s, v8::String>, String> {
    if is_shareable(source) {
        // The creator isolate holds no `SnapshotSources`, so pin the source.
        return shared_source_entry(source, true)?
            .0
            .new_string(scope)
            .ok_or_else(|| String::from("failed to create shared V8 source string"));
    }
    v8::String::new(scope, source).ok_or_else(|| String::from("source too large for V8"))
}

/// External references for a snapshot whose shared sources are `sources`, in
/// order. Interned while in use, so each distinct table is built once.
fn external_refs_with_sources(sources: &[SharedSource]) -> SnapshotRefs {
    if sources.is_empty() {
        return SnapshotRefs::Base(crate::bridge::external_refs());
    }
    static TABLES: OnceLock<Mutex<HashMap<Vec<usize>, Weak<v8::ExternalReferences>>>> =
        OnceLock::new();
    let key: Vec<usize> = sources
        .iter()
        .map(|source| source.address() as usize)
        .collect();
    let mut tables = TABLES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("external reference tables poisoned");
    if let Some(table) = tables.get(&key).and_then(Weak::upgrade) {
        return SnapshotRefs::Shared(table);
    }
    tables.retain(|_, table| table.strong_count() > 0);
    let mut references = crate::bridge::base_external_references();
    for source in sources {
        references.push(v8::ExternalReference {
            pointer: source.address(),
        });
    }
    let table = Arc::new(v8::ExternalReferences::new(&references));
    tables.insert(key, Arc::downgrade(&table));
    SnapshotRefs::Shared(table)
}
