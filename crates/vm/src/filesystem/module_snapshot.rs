use super::*;
use crate::executor::{GuestModuleReader, LocalModuleResolutionCache};
use agentos_executor_v8_runtime::execution::ResolvedModuleFormat;
use agentos_executor_v8_runtime::module_snapshot::{
    discover_module_graph, module_graph_candidate, register_module_graph, source_digest,
    validate_module_graph, MAX_MODULE_SNAPSHOT_GRAPH_SOURCE_BYTES,
};
use agentos_vm_kernel::mount_table::MountEntry;
use std::sync::Arc;

const MAX_SNAPSHOT_FS_OPERATIONS: usize = 32_768;

#[derive(Default)]
struct SnapshotSources {
    source: BTreeMap<String, String>,
    format: BTreeMap<String, ResolvedModuleFormat>,
    resolutions: BTreeMap<String, BTreeMap<String, String>>,
}

struct RecordedSources(Arc<SnapshotSources>);

impl GuestModuleReader for RecordedSources {
    fn read_module_source(&mut self, path: &str) -> Option<String> {
        self.0.source.get(path).cloned()
    }

    fn resolve_module(&mut self, specifier: &str, referrer: &str) -> Option<String> {
        self.0.resolutions.get(referrer)?.get(specifier).cloned()
    }

    fn module_format(&mut self, path: &str) -> Option<ResolvedModuleFormat> {
        self.0.format.get(path).copied()
    }

    fn boxed_clone(&self) -> Option<Box<dyn GuestModuleReader>> {
        Some(Box::new(Self(Arc::clone(&self.0))))
    }
}

struct SnapshotVfs<'a> {
    kernel: &'a mut SidecarKernel,
    mounts: Vec<MountEntry>,
    pid: u32,
    cwd: &'a str,
    ops_left: usize,
    bytes_left: usize,
    invalid: bool,
}

impl SnapshotVfs<'_> {
    fn qualify(&mut self, path: &str) -> Result<String, HostServiceError> {
        // Do not normalize a path before process-scoped kernel resolution.
        // A lexical dot-segment collapse can bypass symlink traversal and DAC
        // checks, or turn a kernel ENOTDIR into a valid module selection.
        let path = if path.starts_with('/') {
            path.to_owned()
        } else {
            format!("{}/{path}", self.cwd.trim_end_matches('/'))
        };
        if path.split('/').any(|segment| matches!(segment, "." | "..")) {
            self.invalid = true;
            return Err(HostServiceError::new(
                "EINVAL",
                "ambiguous module snapshot path",
            ));
        }
        Ok(path)
    }

    fn readonly(&self, path: &str) -> bool {
        self.mounts
            .iter()
            .filter(|mount| {
                crate::service::path_is_within_root(
                    std::path::Path::new(path),
                    std::path::Path::new(&mount.path),
                )
            })
            .max_by_key(|mount| mount.path.len())
            .is_some_and(|mount| mount.read_only)
    }

    fn take_operation(&mut self) -> Result<(), HostServiceError> {
        if let Some(remaining) = self.ops_left.checked_sub(1) {
            self.ops_left = remaining;
            return Ok(());
        }
        self.invalid = true;
        Err(HostServiceError::new(
            "ERR_AGENTOS_MODULE_SNAPSHOT_GRAPH_LIMIT",
            format!("module snapshot capture exceeds {MAX_SNAPSHOT_FS_OPERATIONS} VFS operations"),
        ))
    }
}

impl ModuleFsReader for SnapshotVfs<'_> {
    fn canonical_guest_path(&mut self, path: &str) -> Result<Option<String>, HostServiceError> {
        self.take_operation()?;
        let path = self.qualify(path)?;
        module_kernel_optional(self.kernel.realpath_for_process(
            EXECUTION_DRIVER_NAME,
            self.pid,
            &path,
        ))
    }

    fn read_to_string(&mut self, path: &str) -> Result<Option<String>, HostServiceError> {
        self.take_operation()?;
        let path = self.qualify(path)?;
        let Some(canonical) = self.canonical_guest_path(&path)? else {
            return Ok(None);
        };
        let Some(length) =
            module_kernel_optional(self.kernel.preflight_regular_file_read_for_process(
                EXECUTION_DRIVER_NAME,
                self.pid,
                &path,
            ))?
        else {
            return Ok(None);
        };
        if !self.readonly(&path) || !self.readonly(&canonical) {
            self.invalid = true;
            return Err(HostServiceError::new(
                "EROFS",
                "module snapshot source and its canonical target must be on read-only mounts",
            ));
        }
        if length > self.bytes_left {
            self.invalid = true;
            return Err(HostServiceError::new(
                "ERR_AGENTOS_MODULE_SNAPSHOT_GRAPH_LIMIT",
                format!(
                    "module snapshot capture exceeds {MAX_MODULE_SNAPSHOT_GRAPH_SOURCE_BYTES} source bytes"
                ),
            ));
        }
        let bytes = self
            .kernel
            .pread_file_for_process(EXECUTION_DRIVER_NAME, self.pid, &path, 0, length)
            .map_err(|error| HostServiceError::new(error.code(), error.to_string()))?;
        self.bytes_left -= length;
        if bytes.len() != length {
            self.invalid = true;
            return Err(HostServiceError::new(
                "EIO",
                "module changed during snapshot capture",
            ));
        }
        module_utf8(&path, bytes).map(Some)
    }

    fn path_is_dir(&mut self, path: &str) -> Result<Option<bool>, HostServiceError> {
        self.take_operation()?;
        let path = self.qualify(path)?;
        Ok(module_kernel_optional(self.kernel.stat_for_process(
            EXECUTION_DRIVER_NAME,
            self.pid,
            &path,
        ))?
        .map(|stat| stat.is_directory))
    }

    fn path_exists(&mut self, path: &str) -> Result<bool, HostServiceError> {
        self.path_is_dir(path).map(|stat| stat.is_some())
    }
}

struct SnapshotCapture<'a> {
    vfs: SnapshotVfs<'a>,
    cache: LocalModuleResolutionCache,
    package_fallback: Option<String>,
    recorded: SnapshotSources,
}

impl SnapshotCapture<'_> {
    fn resolver(&mut self) -> ModuleResolver<'_, &mut dyn ModuleFsReader> {
        ModuleResolver::new(&mut self.vfs as &mut dyn ModuleFsReader, &mut self.cache)
    }
}

impl GuestModuleReader for SnapshotCapture<'_> {
    fn read_module_source(&mut self, path: &str) -> Option<String> {
        if let Some(source) = self.recorded.source.get(path) {
            return Some(source.clone());
        }
        self.vfs.take_operation().ok()?;
        let source = self.resolver().load_file(path).ok()??;
        self.recorded.source.insert(path.to_owned(), source.clone());
        Some(source)
    }

    fn resolve_module(&mut self, specifier: &str, referrer: &str) -> Option<String> {
        if let Some(path) = self
            .recorded
            .resolutions
            .get(referrer)
            .and_then(|edges| edges.get(specifier))
        {
            return Some(path.clone());
        }
        self.vfs.take_operation().ok()?;
        let path = self
            .resolver()
            .resolve_module(specifier, referrer, ModuleResolveMode::Import)
            .ok()?;
        let path = match path {
            Some(path) => path,
            None if is_bare_module_specifier(specifier) => {
                let fallback = self
                    .package_fallback
                    .clone()
                    .filter(|fallback| fallback != referrer)?;
                self.resolver()
                    .resolve_module(specifier, &fallback, ModuleResolveMode::Import)
                    .ok()??
            }
            None => return None,
        };
        self.recorded
            .resolutions
            .entry(referrer.to_owned())
            .or_default()
            .insert(specifier.to_owned(), path.clone());
        Some(path)
    }

    fn module_format(&mut self, path: &str) -> Option<ResolvedModuleFormat> {
        if let Some(format) = self.recorded.format.get(path) {
            return Some(*format);
        }
        self.vfs.take_operation().ok()?;
        let format = match self.resolver().module_format(path).ok()?? {
            LocalResolvedModuleFormat::Module => ResolvedModuleFormat::Module,
            LocalResolvedModuleFormat::Commonjs => ResolvedModuleFormat::Commonjs,
            LocalResolvedModuleFormat::Json => ResolvedModuleFormat::Json,
        };
        self.recorded.format.insert(path.to_owned(), format);
        Some(format)
    }
}

/// Capture a configured entry's authorized, static sources for an optional
/// V8 snapshot. After this call the executor owns a replay of those sources,
/// with no kernel or mount access and no reader installed on the guest.
pub(crate) fn capture_launch_module_snapshot(
    kernel: &mut SidecarKernel,
    pid: u32,
    cwd: &str,
    env: &BTreeMap<String, String>,
    entrypoints: &[String],
    inline_code: Option<&str>,
) -> Option<Box<dyn GuestModuleReader>> {
    let entry = env.get("AGENTOS_GUEST_ENTRYPOINT")?;
    if !entry.starts_with('/') || !entrypoints.contains(entry) {
        return None;
    }
    let mounts = kernel.mounted_filesystems();
    let mut capture = SnapshotCapture {
        vfs: SnapshotVfs {
            kernel,
            mounts,
            pid,
            cwd,
            ops_left: MAX_SNAPSHOT_FS_OPERATIONS,
            bytes_left: MAX_MODULE_SNAPSHOT_GRAPH_SOURCE_BYTES,
            invalid: false,
        },
        cache: LocalModuleResolutionCache::default(),
        package_fallback: agentos_package_version_root(entry),
        recorded: SnapshotSources::default(),
    };
    if capture.module_format(entry) != Some(ResolvedModuleFormat::Module) {
        return None;
    }
    let source = capture.read_module_source(entry)?;
    if inline_code.is_some_and(|code| {
        let code = if code.starts_with("#!") {
            code.find('\n').map_or("", |newline| &code[newline..])
        } else {
            code
        };
        code != source
    }) {
        return None;
    }

    let validated = module_graph_candidate(entry, &source_digest(&source))
        .is_some_and(|graph| validate_module_graph(&graph, &source, &mut capture).is_ok());
    if !validated {
        match discover_module_graph(entry, &source, &mut capture) {
            Ok(graph) if !capture.vfs.invalid => {
                // Discovery needed the borrowed kernel resolver. Register the
                // bounded graph now so the worker only receives the graph token.
                register_module_graph(graph);
            }
            Ok(_) => return None,
            Err(error) => {
                tracing::debug!(%error, "module snapshot capture fell back to the live resolver");
                return None;
            }
        }
    } else if capture.vfs.invalid {
        return None;
    }
    Some(Box::new(RecordedSources(Arc::new(capture.recorded))))
}
