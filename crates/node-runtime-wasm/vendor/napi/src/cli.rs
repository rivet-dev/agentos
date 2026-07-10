use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use virtual_fs::{AsyncReadExt, FileSystem};
use wasmer::sys::{EngineBuilder, Features};
use wasmer::{Module, Store};
use wasmer_cache::{Cache, FileSystemCache, Hash as CacheHash};
use wasmer_compiler_llvm::{LLVM, LLVMOptLevel};
use wasmer_types::ModuleHash;
use wasmer_wasix::{
    Pipe, PluggableRuntime, WasiError,
    runners::wasi::{RuntimeOrEngine, WasiRunner},
    runtime::task_manager::tokio::TokioTaskManager,
};

use crate::{NapiCtx, guest::napi::register_env_imports};
use wasmer_c_api_imports::WasmCapiRuntimeHooks;

#[derive(Debug, Clone)]
pub struct GuestMount {
    pub host_path: PathBuf,
    pub guest_path: PathBuf,
}

pub struct LoadedWasm {
    pub store: Store,
    pub module: Module,
    pub module_hash: ModuleHash,
}

fn create_cli_store() -> Store {
    let mut features = Features::default();
    features.exceptions(true);

    let mut compiler = LLVM::default();
    compiler.opt_level(LLVMOptLevel::Less);

    let engine = EngineBuilder::new(compiler)
        .set_features(Some(features))
        .engine();
    Store::new(engine)
}

fn wasmer_cache_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("wasmer-cache")
}

fn load_or_compile_module(store: &Store, wasm_bytes: &[u8]) -> Result<Module> {
    let key = CacheHash::generate(wasm_bytes);
    let mut cache = FileSystemCache::new(wasmer_cache_dir())
        .context("failed to create/access Wasmer cache directory")?;

    if let Ok(module) = unsafe { cache.load(store, key) } {
        return Ok(module);
    }

    let module = Module::new(store, wasm_bytes).context("failed to compile wasm module")?;
    let _ = cache.store(key, &module);
    Ok(module)
}

fn spawn_pipe_drain_thread(
    mut pipe: Pipe,
    mut sink: Box<dyn Write + Send>,
) -> std::thread::JoinHandle<Result<String>> {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to create stdio drain runtime")?;
        let mut captured = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let n = runtime
                .block_on(pipe.read(&mut chunk))
                .context("failed reading WASIX stdio pipe")?;
            if n == 0 {
                break;
            }
            sink.write_all(&chunk[..n])
                .context("failed writing drained WASIX stdio")?;
            sink.flush()
                .context("failed flushing drained WASIX stdio")?;
            captured.extend_from_slice(&chunk[..n]);
        }
        String::from_utf8(captured).context("WASIX stdio was not valid UTF-8")
    })
}

pub fn load_wasix_module(wasm_path: &Path) -> Result<LoadedWasm> {
    let wasm_bytes = std::fs::read(wasm_path)
        .with_context(|| format!("failed to read wasm file at {}", wasm_path.display()))?;

    let store = create_cli_store();

    let module = load_or_compile_module(&store, &wasm_bytes)?;

    let module_hash = ModuleHash::sha256(&wasm_bytes);

    Ok(LoadedWasm {
        store,
        module,
        module_hash,
    })
}

pub fn configure_runner_mounts(
    runner: &mut WasiRunner,
    _wasm_path: &Path,
    extra_mounts: &[GuestMount],
) -> Result<()> {
    if extra_mounts.is_empty() {
        return Ok(());
    }

    let host_handle = tokio::runtime::Handle::current();
    for mount in extra_mounts {
        let host_fs: Arc<dyn FileSystem + Send + Sync> = Arc::new(
            virtual_fs::host_fs::FileSystem::new(host_handle.clone(), mount.host_path.clone())
                .with_context(|| {
                    format!("failed to create host fs for {}", mount.host_path.display())
                })?,
        );
        runner.with_mount(mount.guest_path.display().to_string(), host_fs);
    }

    Ok(())
}

pub fn run_wasix_main_capture_stdio(
    wasm_path: &Path,
    args: &[String],
    extra_mounts: &[GuestMount],
) -> Result<(i32, String, String)> {
    let ctx = NapiCtx::default();
    run_wasix_main_capture_stdio_with_ctx(&ctx, wasm_path, args, extra_mounts)
}

pub fn run_wasix_main_capture_stdio_with_ctx(
    ctx: &NapiCtx,
    wasm_path: &Path,
    args: &[String],
    extra_mounts: &[GuestMount],
) -> Result<(i32, String, String)> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to create tokio runtime for WASIX")?;
    let _guard = runtime.enter();

    let (stdout_tx, stdout_rx) = Pipe::channel();
    let (stderr_tx, stderr_rx) = Pipe::channel();
    let stdout_thread = spawn_pipe_drain_thread(stdout_rx, Box::new(std::io::stdout()));
    let stderr_thread = spawn_pipe_drain_thread(stderr_rx, Box::new(std::io::stderr()));
    let exit_code = {
        let loaded = load_wasix_module(wasm_path)?;
        let engine = loaded.store.engine().clone();
        let module = loaded.module;
        let module_hash = loaded.module_hash;

        let mut runner = WasiRunner::new();
        runner
            .with_stdout(Box::new(stdout_tx))
            .with_stderr(Box::new(stderr_tx))
            .with_args(args.iter().cloned());
        configure_runner_mounts(&mut runner, wasm_path, extra_mounts)?;

        let task_manager = Arc::new(TokioTaskManager::new(tokio::runtime::Handle::current()));
        let mut runtime = PluggableRuntime::new(task_manager);
        runtime.set_engine(engine.clone());

        let (napi_version, napi_extension_version) = NapiCtx::module_needs_napi(&module);
        if napi_version.is_some() || napi_extension_version.is_some() {
            runner
                .capabilities_mut()
                .threading
                .enable_asynchronous_threading = false;
        }
        let hooks = ctx.runtime_hooks();
        let wasm_c_api_hooks = WasmCapiRuntimeHooks::new();
        runtime
            .with_additional_imports({
                let hooks = hooks.clone();
                let wasm_c_api_hooks = wasm_c_api_hooks.clone();
                move |module, store| {
                    let mut imports = hooks.additional_imports(module, store)?;
                    register_env_imports(store, &mut imports);
                    wasm_c_api_hooks.add_imports(module, store, &mut imports)?;
                    Ok(imports)
                }
            })
            .with_instance_setup(move |module, store, instance, imported_memory| {
                hooks.configure_instance(module, store, instance, imported_memory)?;
                wasm_c_api_hooks.configure_instance(module, store, instance, imported_memory)
            });

        match runner.run_wasm(
            RuntimeOrEngine::Runtime(Arc::new(runtime)),
            "guest-test",
            module,
            module_hash,
        ) {
            Ok(()) => 0,
            Err(err) => {
                if let Some(WasiError::Exit(code)) = err.downcast_ref::<WasiError>() {
                    i32::from(*code)
                } else {
                    return Err(err).context("failed to run WASIX module through WasiRunner");
                }
            }
        }
    };

    let stdout = stdout_thread
        .join()
        .map_err(|_| anyhow::anyhow!("stdout drain thread panicked"))??;
    let stderr = stderr_thread
        .join()
        .map_err(|_| anyhow::anyhow!("stderr drain thread panicked"))??;
    Ok((exit_code, stdout, stderr))
}

pub fn run_wasix_main_capture_stdout(
    wasm_path: &Path,
    args: &[String],
    extra_mounts: &[GuestMount],
) -> Result<(i32, String)> {
    let ctx = NapiCtx::default();
    run_wasix_main_capture_stdout_with_ctx(&ctx, wasm_path, args, extra_mounts)
}

pub fn run_wasix_main_capture_stdout_with_ctx(
    ctx: &NapiCtx,
    wasm_path: &Path,
    args: &[String],
    extra_mounts: &[GuestMount],
) -> Result<(i32, String)> {
    let (exit_code, stdout, _stderr) =
        run_wasix_main_capture_stdio_with_ctx(ctx, wasm_path, args, extra_mounts)?;
    Ok((exit_code, stdout))
}
