use agentos_v8_runtime::embedded_runtime::{shared_embedded_runtime, EmbeddedV8Runtime};
use agentos_v8_runtime::isolate;
use agentos_v8_runtime::runtime_protocol::{RuntimeCommand, RuntimeEvent, SessionMessage};
use agentos_v8_runtime::wasm_workers::{
    V8SharedWasmWorkerExecutor, WasmWorkerLimits, WasmWorkerManager,
};
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

// Timing-sensitive assertions flake under the CPU contention of a parallel test
// run (see CLAUDE.md > Testing). Gated off by default; the nightly timing lane
// sets AGENTOS_RUN_TIMING_TESTS=1 to enforce them.
fn run_timing_sensitive_tests() -> bool {
    std::env::var_os("AGENTOS_RUN_TIMING_TESTS").is_some()
}

static NEXT_TEST_SESSION_ID: AtomicU64 = AtomicU64::new(1);
const NESTED_BOOTSTRAP_PROBE_BYTES: &[u8] =
    include_bytes!("../../node-runtime-wasm/artifacts/nested-bootstrap.wasm");

const PTHREAD_PROBE_WORKER_BOOTSTRAP: &str = r#"(function(module, memory, tid, startArg) {
  const failSpawn = () => -1;
  const imports = Object.freeze({
    agentos_posix_v1: Object.freeze({
      proc_exit(status) { throw new Error(`worker proc_exit(${status})`); },
      sched_yield() { return 0; },
      'thread-spawn': failSpawn,
    }),
    env: Object.freeze({ memory }),
  });
  const instance = new WebAssembly.Instance(module, imports);
  instance.exports.wasi_thread_start(tid, startArg);
})"#;

const TERMINATION_PROBE_WORKER_BOOTSTRAP: &str = r#"(function(module, memory, tid, startArg) {
  const instance = new WebAssembly.Instance(module, {
    env: Object.freeze({ memory }),
  });
  instance.exports.wasi_thread_start(tid, startArg);
})"#;

fn pthread_probe_manager() -> &'static Mutex<Option<Arc<WasmWorkerManager>>> {
    static MANAGER: OnceLock<Mutex<Option<Arc<WasmWorkerManager>>>> = OnceLock::new();
    MANAGER.get_or_init(|| Mutex::new(None))
}

fn pthread_probe_register_callback<'s>(
    scope: &mut v8::HandleScope<'s>,
    args: v8::FunctionCallbackArguments<'s>,
    mut return_value: v8::ReturnValue,
) {
    let module = match v8::Local::<v8::WasmModuleObject>::try_from(args.get(0)) {
        Ok(module) => module,
        Err(_) => {
            let message =
                v8::String::new(scope, "pthread probe registration requires a module").unwrap();
            let exception = v8::Exception::type_error(scope, message);
            scope.throw_exception(exception);
            return;
        }
    };
    let memory = match v8::Local::<v8::WasmMemoryObject>::try_from(args.get(1)) {
        Ok(memory) => memory,
        Err(_) => {
            let message =
                v8::String::new(scope, "pthread probe registration requires a memory").unwrap();
            let exception = v8::Exception::type_error(scope, message);
            scope.throw_exception(exception);
            return;
        }
    };

    let executor = match V8SharedWasmWorkerExecutor::capture(
        scope,
        module,
        memory,
        PTHREAD_PROBE_WORKER_BOOTSTRAP,
        Some(64),
    ) {
        Ok(executor) => Arc::new(executor),
        Err(error) => {
            let message = v8::String::new(scope, &error).unwrap();
            let exception = v8::Exception::error(scope, message);
            scope.throw_exception(exception);
            return;
        }
    };
    let manager = WasmWorkerManager::new(WasmWorkerLimits::default(), executor)
        .expect("pthread probe worker limits should be valid");
    *pthread_probe_manager()
        .lock()
        .expect("pthread probe manager lock poisoned") = Some(Arc::new(manager));
    return_value.set(v8::Boolean::new(scope, true).into());
}

fn pthread_probe_spawn_callback<'s>(
    scope: &mut v8::HandleScope<'s>,
    args: v8::FunctionCallbackArguments<'s>,
    mut return_value: v8::ReturnValue,
) {
    let Some(start_arg) = args.get(0).int32_value(scope) else {
        return_value.set_int32(-1);
        return;
    };
    let Some(manager) = pthread_probe_manager()
        .lock()
        .expect("pthread probe manager lock poisoned")
        .clone()
    else {
        return_value.set_int32(-1);
        return;
    };
    match manager.spawn(start_arg) {
        Ok(tid) => return_value.set_int32(tid),
        Err(_) => return_value.set_int32(-1),
    }
}

fn next_session_id() -> String {
    format!(
        "embedded-runtime-session-{}",
        NEXT_TEST_SESSION_ID.fetch_add(1, Ordering::Relaxed)
    )
}

fn register_and_create_session(
    runtime: &Arc<EmbeddedV8Runtime>,
    session_id: &str,
) -> io::Result<mpsc::Receiver<RuntimeEvent>> {
    let receiver = runtime.register_session(session_id)?;
    runtime.dispatch(RuntimeCommand::CreateSession {
        session_id: session_id.to_owned(),
        heap_limit_mb: None,
        cpu_time_limit_ms: None,
        wall_clock_limit_ms: None,
        warm_hint: None,
    })?;
    Ok(receiver)
}

fn register_and_create_session_with_cpu_time_limit(
    runtime: &Arc<EmbeddedV8Runtime>,
    session_id: &str,
    cpu_time_limit_ms: Option<u32>,
) -> io::Result<mpsc::Receiver<RuntimeEvent>> {
    let receiver = runtime.register_session(session_id)?;
    runtime.dispatch(RuntimeCommand::CreateSession {
        session_id: session_id.to_owned(),
        heap_limit_mb: None,
        cpu_time_limit_ms,
        wall_clock_limit_ms: None,
        warm_hint: None,
    })?;
    Ok(receiver)
}

fn dispatch_execute(
    runtime: &EmbeddedV8Runtime,
    session_id: &str,
    mode: u8,
    bridge_code: &str,
    user_code: &str,
) -> io::Result<()> {
    dispatch_execute_with_wasm(runtime, session_id, mode, bridge_code, user_code, None)
}

fn dispatch_execute_with_wasm(
    runtime: &EmbeddedV8Runtime,
    session_id: &str,
    mode: u8,
    bridge_code: &str,
    user_code: &str,
    wasm_module_bytes: Option<Arc<Vec<u8>>>,
) -> io::Result<()> {
    runtime.dispatch(RuntimeCommand::SendToSession {
        session_id: session_id.to_owned(),
        message: SessionMessage::Execute {
            mode,
            file_path: String::new(),
            bridge_code: bridge_code.to_owned(),
            post_restore_script: String::new(),
            userland_code: String::new(),
            high_resolution_time: false,
            user_code: user_code.to_owned(),
            wasm_module_bytes,
        },
    })
}

fn wait_for_execution_result(
    receiver: &mpsc::Receiver<RuntimeEvent>,
    session_id: &str,
) -> RuntimeEvent {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("timed out waiting for execution result");
        let event = receiver
            .recv_timeout(remaining)
            .expect("runtime event should arrive before timeout");
        if matches!(
            &event,
            RuntimeEvent::ExecutionResult {
                session_id: event_session_id,
                ..
            } if event_session_id == session_id
        ) {
            return event;
        }
    }
}

fn wait_for_bridge_call(receiver: &mpsc::Receiver<RuntimeEvent>, session_id: &str) -> RuntimeEvent {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("timed out waiting for bridge call");
        let event = receiver
            .recv_timeout(remaining)
            .expect("bridge call should arrive before timeout");
        if matches!(
            &event,
            RuntimeEvent::BridgeCall {
                session_id: event_session_id,
                ..
            } if event_session_id == session_id
        ) {
            return event;
        }
    }
}

fn assert_execution_ok(receiver: &mpsc::Receiver<RuntimeEvent>, session_id: &str) {
    let event = wait_for_execution_result(receiver, session_id);
    match event {
        RuntimeEvent::ExecutionResult {
            exit_code,
            error,
            exports,
            ..
        } => {
            assert_eq!(
                exit_code, 0,
                "expected successful execution result for {session_id}: {error:?}"
            );
            assert!(error.is_none(), "unexpected execution error: {error:?}");
            assert!(
                exports.is_none(),
                "script execution should not export values"
            );
        }
        other => panic!("expected execution result, got {other:?}"),
    }
}

fn wait_until(message: &str, predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if predicate() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("{message}");
}

fn assert_create_destroy_reuses_session_ids() -> io::Result<()> {
    let runtime = shared_embedded_runtime()?;
    let session_id = next_session_id();

    let _receiver = register_and_create_session(&runtime, &session_id)?;
    assert!(
        runtime.session_count() >= 1,
        "embedded runtime should track created sessions"
    );

    let duplicate_error = runtime
        .dispatch(RuntimeCommand::CreateSession {
            session_id: session_id.clone(),
            heap_limit_mb: None,
            cpu_time_limit_ms: None,
            wall_clock_limit_ms: None,
            warm_hint: None,
        })
        .expect_err("duplicate sessions should be rejected");
    assert_eq!(duplicate_error.kind(), io::ErrorKind::Other);

    runtime.session_handle(session_id.clone()).destroy()?;
    assert_eq!(
        runtime.session_count(),
        0,
        "destroying the only test session should return the runtime to zero sessions"
    );

    let _receiver = register_and_create_session(&runtime, &session_id)?;
    runtime.session_handle(session_id).destroy()?;
    assert_eq!(
        runtime.session_count(),
        0,
        "recreated sessions should also tear down cleanly"
    );

    Ok(())
}

fn assert_warmed_snapshot_bridge_state() -> io::Result<()> {
    let runtime = Arc::new(EmbeddedV8Runtime::new(Some(1))?);
    let session_id = next_session_id();
    let receiver = register_and_create_session(&runtime, &session_id)?;
    let bridge_code = "(function() { globalThis.__snapshotMarker = 'warm'; })();";

    runtime.dispatch(RuntimeCommand::WarmSnapshot {
        bridge_code: bridge_code.to_owned(),
        userland_code: String::new(),
    })?;
    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        bridge_code,
        "if (globalThis.__snapshotMarker !== 'warm') { throw new Error(`saw ${globalThis.__snapshotMarker}`); }",
    )?;
    assert_execution_ok(&receiver, &session_id);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    Ok(())
}

fn assert_snapshot_rebuild_on_bridge_change() -> io::Result<()> {
    let runtime = Arc::new(EmbeddedV8Runtime::new(Some(1))?);
    let session_id = next_session_id();
    let receiver = register_and_create_session(&runtime, &session_id)?;
    let bridge_a = "(function() { globalThis.__bridgeSnapshot = 'A'; })();";
    let bridge_b = "(function() { globalThis.__bridgeSnapshot = 'B'; })();";

    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        bridge_a,
        "if (globalThis.__bridgeSnapshot !== 'A') { throw new Error(`saw ${globalThis.__bridgeSnapshot}`); }",
    )?;
    assert_execution_ok(&receiver, &session_id);

    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        bridge_b,
        "if (globalThis.__bridgeSnapshot !== 'B') { throw new Error(`saw ${globalThis.__bridgeSnapshot}`); }",
    )?;
    assert_execution_ok(&receiver, &session_id);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    Ok(())
}

fn assert_execute_rejects_oversized_bridge_code() -> io::Result<()> {
    let runtime = Arc::new(EmbeddedV8Runtime::new(Some(1))?);
    let session_id = next_session_id();
    let receiver = register_and_create_session(&runtime, &session_id)?;
    let oversized_bridge_code = " ".repeat(16 * 1024 * 1024 + 1);

    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        &oversized_bridge_code,
        "globalThis.__should_not_run = true;",
    )?;

    let event = wait_for_execution_result(&receiver, &session_id);
    match event {
        RuntimeEvent::ExecutionResult {
            exit_code,
            error: Some(error),
            ..
        } => {
            assert_eq!(exit_code, 1);
            assert_eq!(error.code, "ERR_V8_BRIDGE_CODE_LIMIT");
            assert!(error
                .message
                .contains("bridge code too large for V8 bridge setup"));
        }
        other => panic!("expected bridge-code limit execution error, got {other:?}"),
    }

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    wait_until(
        "expected oversized-bridge session to drain after rejection",
        || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
    );
    Ok(())
}

fn assert_direct_zero_cpu_time_limit_disables_timeout() -> io::Result<()> {
    let runtime = Arc::new(EmbeddedV8Runtime::new(Some(1))?);
    let session_id = next_session_id();
    let receiver = register_and_create_session_with_cpu_time_limit(&runtime, &session_id, Some(0))?;

    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        "",
        "let total = 0; for (let i = 0; i < 100000; i++) { total += i; }",
    )?;
    assert_execution_ok(&receiver, &session_id);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    wait_until(
        "expected zero-timeout session to drain after successful execution",
        || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
    );
    Ok(())
}

fn assert_nested_node_runtime_probe_uses_same_isolate() -> io::Result<()> {
    let runtime = Arc::new(EmbeddedV8Runtime::new(Some(1))?);
    let session_id = next_session_id();
    let receiver = register_and_create_session(&runtime, &session_id)?;

    dispatch_execute_with_wasm(
        runtime.as_ref(),
        &session_id,
        0,
        "",
        r#"
        (() => {
          const bytes = globalThis.__agentOSWasmModuleBytes;
          delete globalThis.__agentOSWasmModuleBytes;

          let instance;
          let throwOnCall = false;
          const imports = Object.freeze({
            agentos_napi_v1: Object.freeze({
              call_js(value) {
                if (throwOnCall) throw new Error('probe-import-error');
                if (!instance) throw new Error('probe-instance-unavailable');
                return instance.exports.reenter(value + 1);
              },
            }),
          });

          const module = new WebAssembly.Module(bytes);
          const imported = WebAssembly.Module.imports(module);
          if (imported.length !== 1 ||
              imported[0].module !== 'agentos_napi_v1' ||
              imported[0].name !== 'call_js' ||
              imported[0].kind !== 'function') {
            throw new Error(`unexpected probe imports: ${JSON.stringify(imported)}`);
          }

          instance = new WebAssembly.Instance(module, imports);
          if (instance.exports.start(20) !== 43) {
            throw new Error('nested same-instance callback returned the wrong value');
          }

          throwOnCall = true;
          try {
            instance.exports.start(1);
            throw new Error('expected isolate-local import exception');
          } catch (error) {
            if (error?.message !== 'probe-import-error') throw error;
          }
          throwOnCall = false;

          try {
            instance.exports.trap();
            throw new Error('expected WebAssembly trap');
          } catch (error) {
            if (!(error instanceof WebAssembly.RuntimeError)) throw error;
          }

          const beforePages = instance.exports.grow_memory(1);
          if (beforePages !== 2 || instance.exports.memory.buffer.byteLength !== 3 * 65536) {
            throw new Error('bounded memory growth did not match the module declaration');
          }
          if (instance.exports.grow_memory(2) !== -1) {
            throw new Error('memory growth beyond the declared maximum should fail');
          }

          instance = null;
          if ('agentos_napi_v1' in globalThis || '__agentOSWasmModuleBytes' in globalThis) {
            throw new Error('closure-private bootstrap state leaked onto globalThis');
          }
        })();
        "#,
        Some(Arc::new(NESTED_BOOTSTRAP_PROBE_BYTES.to_vec())),
    )?;
    assert_execution_ok(&receiver, &session_id);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    wait_until("expected nested-probe session to drain", || {
        runtime.session_count() == 0 && runtime.active_slot_count() == 0
    });
    Ok(())
}

fn assert_v8_wasm_required_features() -> io::Result<()> {
    let runtime = Arc::new(EmbeddedV8Runtime::new(Some(1))?);
    let session_id = next_session_id();
    let receiver = register_and_create_session(&runtime, &session_id)?;
    let module = wat::parse_str(
        r#"
        (module
          (memory (export "memory") 1 2 shared)
          (tag (export "tag") (param i32))
          (func (export "atomic_add") (param i32) (result i32)
            i32.const 0
            local.get 0
            i32.atomic.rmw.add)
          (func (export "simd_lane") (result i32)
            v128.const i32x4 1 2 3 4
            i32x4.extract_lane 2)
          (func (export "throw_tagged")
            i32.const 7
            throw 0))
        "#,
    )
    .expect("required V8 WebAssembly feature probe should compile from WAT");

    dispatch_execute_with_wasm(
        runtime.as_ref(),
        &session_id,
        0,
        "",
        r#"
        (() => {
          const bytes = globalThis.__agentOSWasmModuleBytes;
          delete globalThis.__agentOSWasmModuleBytes;
          const module = new WebAssembly.Module(bytes);
          if (WebAssembly.Module.imports(module).length !== 0) {
            throw new Error('feature probe unexpectedly imports host behavior');
          }
          const instance = new WebAssembly.Instance(module, Object.freeze({}));
          if (!(instance.exports.memory.buffer instanceof SharedArrayBuffer)) {
            throw new Error('V8 did not create shared WebAssembly memory');
          }
          if (instance.exports.atomic_add(5) !== 0 ||
              Atomics.load(new Int32Array(instance.exports.memory.buffer), 0) !== 5) {
            throw new Error('WebAssembly atomics did not update shared memory');
          }
          if (instance.exports.simd_lane() !== 3) {
            throw new Error('WebAssembly SIMD returned the wrong lane');
          }
          try {
            instance.exports.throw_tagged();
            throw new Error('expected a WebAssembly exception');
          } catch (error) {
            if (!(error instanceof WebAssembly.Exception) ||
                !error.is(instance.exports.tag) ||
                error.getArg(instance.exports.tag, 0) !== 7) {
              throw error;
            }
          }
        })();
        "#,
        Some(Arc::new(module)),
    )?;
    assert_execution_ok(&receiver, &session_id);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    wait_until(
        "expected V8 WebAssembly feature-probe session to drain",
        || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
    );
    Ok(())
}

fn assert_node_reactor_instantiates_in_existing_v8_isolate() -> io::Result<()> {
    let wasm_path = std::env::var_os("AGENTOS_NODE_RUNTIME_WASM").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "AGENTOS_NODE_RUNTIME_WASM must name the built reactor module",
        )
    })?;
    let module_bytes = Arc::new(std::fs::read(wasm_path)?);
    let runtime = Arc::new(EmbeddedV8Runtime::new(Some(1))?);
    let session_id = next_session_id();
    let receiver = register_and_create_session(&runtime, &session_id)?;

    dispatch_execute_with_wasm(
        runtime.as_ref(),
        &session_id,
        0,
        "",
        r#"
        (() => {
          const bytes = globalThis.__agentOSWasmModuleBytes;
          delete globalThis.__agentOSWasmModuleBytes;
          const module = new WebAssembly.Module(bytes);
          const descriptors = WebAssembly.Module.imports(module);
          const modules = [...new Set(descriptors.map(({ module }) => module))].sort();
          const expectedModules = [
            'agentos_napi_v1',
            'agentos_node_engine_v1',
            'agentos_posix_v1',
            'env',
          ];
          if (JSON.stringify(modules) !== JSON.stringify(expectedModules)) {
            throw new Error(`unexpected Node reactor imports: ${JSON.stringify(modules)}`);
          }

          const trace = [];
          const tracedNames = new Set();
          let totalImportCalls = 0;
          let memory;
          let instance;
          const handles = [undefined];
          const callbackInfos = new Map();
          const wrappedData = new WeakMap();
          const references = new Map();
          let nextCallbackInfo = 1;
          let nextReference = 1;
          let pendingException = null;
          const addHandle = (value) => {
            handles.push(value);
            return handles.length - 1;
          };
          const readCString = (pointer) => {
            const bytes = new Uint8Array(memory.buffer);
            let end = pointer >>> 0;
            while (end < bytes.length && bytes[end] !== 0) end += 1;
            let result = '';
            for (let index = pointer >>> 0; index < end; index += 1) {
              result += String.fromCharCode(bytes[index]);
            }
            return result;
          };
          const readString = (pointer, length) => {
            if ((length >>> 0) === 0xffffffff) return readCString(pointer);
            const bytes = new Uint8Array(memory.buffer, pointer >>> 0, length >>> 0);
            let result = '';
            for (const byte of bytes) result += String.fromCharCode(byte);
            return result;
          };
          const invokeGuestCallback = (callback, data, thisValue, args) => {
            const info = nextCallbackInfo++;
            callbackInfos.set(info, { args, data, thisValue });
            try {
              const result = instance.exports.__indirect_function_table.get(callback)(1, info);
              return handles[result];
            } finally {
              callbackInfos.delete(info);
            }
          };
          const makeFunction = (callback, data) => function(...args) {
            return invokeGuestCallback(callback, data, this, args);
          };
          const defineProperties = (target, count, pointer) => {
            const view = new DataView(memory.buffer);
            for (let index = 0; index < (count >>> 0); index += 1) {
              const descriptor = (pointer >>> 0) + index * 32;
              const utf8Name = view.getUint32(descriptor, true);
              const nameHandle = view.getUint32(descriptor + 4, true);
              const method = view.getUint32(descriptor + 8, true);
              const getter = view.getUint32(descriptor + 12, true);
              const setter = view.getUint32(descriptor + 16, true);
              const valueHandle = view.getUint32(descriptor + 20, true);
              const attributes = view.getUint32(descriptor + 24, true);
              const data = view.getUint32(descriptor + 28, true);
              const key = utf8Name !== 0 ? readCString(utf8Name) : handles[nameHandle];
              const property = {
                enumerable: (attributes & 2) !== 0,
                configurable: (attributes & 4) !== 0,
              };
              if (method !== 0) {
                property.value = makeFunction(method, data);
                property.writable = (attributes & 1) !== 0;
              } else if (getter !== 0 || setter !== 0) {
                if (getter !== 0) property.get = makeFunction(getter, data);
                if (setter !== 0) property.set = makeFunction(setter, data);
              } else {
                property.value = handles[valueHandle];
                property.writable = (attributes & 1) !== 0;
              }
              Object.defineProperty(target, key, property);
            }
          };
          const write32 = (pointer, value) =>
            new DataView(memory.buffer).setUint32(pointer >>> 0, value >>> 0, true);
          const unimplemented = new Proxy(Object.create(null), {
            get(_target, name) {
              if (typeof name !== 'string') return undefined;
              return (...args) => {
                totalImportCalls += 1;
                if (trace.length < 128 && !tracedNames.has(name)) {
                  tracedNames.add(name);
                  trace.push([name, args.map(String)]);
                }
                if (totalImportCalls > 20000) {
                  throw new Error(`import-call-limit trace: ${JSON.stringify(trace)}`);
                }
                if (name === 'proc_exit') {
                  throw new Error(`proc_exit trace: ${JSON.stringify(trace)}`);
                }
                if (name === 'clock_time_get') {
                  new DataView(memory.buffer).setBigUint64(
                    args[2] >>> 0,
                    BigInt(Date.now()) * 1000000n,
                    true,
                  );
                  return 0;
                }
                if (name === 'environ_sizes_get') {
                  write32(args[0], 0);
                  write32(args[1], 0);
                  return 0;
                }
                if (name === 'environ_get') return 0;
                if (name === 'proc_sigaction') return 0;
                if (name === 'fd_mode') return 0o20666;
                if (name === 'fd_size' || name === 'path_size') return 0n;
                if (name === 'fd_close' || name === 'fd_fdstat_set_flags') return 0;
                if (name === 'fd_write') {
                  const view = new DataView(memory.buffer);
                  let written = 0;
                  for (let index = 0; index < (args[2] >>> 0); index += 1) {
                    written += view.getUint32((args[1] >>> 0) + index * 8 + 4, true);
                  }
                  write32(args[3], written);
                  return 0;
                }
                if (name === 'net_poll') {
                  const view = new DataView(memory.buffer);
                  for (let index = 0; index < (args[1] >>> 0); index += 1) {
                    view.setUint16((args[0] >>> 0) + index * 8 + 6, 0, true);
                  }
                  write32(args[3], 0);
                  return 0;
                }
                if (name === 'poll_oneoff') {
                  write32(args[3], 0);
                  return 0;
                }
                if (name === 'fd_prestat_get' || name === 'fd_prestat_dir_name') return 8;
                if (name === 'fd_filestat_get') {
                  new Uint8Array(memory.buffer, args[1] >>> 0, 64).fill(0);
                  return 0;
                }
                if (name === 'fd_fdstat_get') {
                  new Uint8Array(memory.buffer, args[1] >>> 0, 24).fill(0);
                  new DataView(memory.buffer).setUint8(args[1] >>> 0, 2);
                  return 0;
                }
                if (name === 'isatty') {
                  write32(args[1], 0);
                  return 0;
                }
                if (name === 'random_get') {
                  new Uint8Array(memory.buffer, args[0] >>> 0, args[1] >>> 0).fill(0x42);
                  return 0;
                }
                if (name === 'proc_getpid') {
                  write32(args[0], 1);
                  return 0;
                }
                if (name === 'proc_getppid') {
                  write32(args[0], 0);
                  return 0;
                }
                if (name === 'fd_pipe') {
                  write32(args[0], 100);
                  write32(args[1], 101);
                  return 0;
                }
                if (name === 'unofficial_napi_create_env') {
                  write32(args[1], 1);
                  write32(args[2], 1);
                  return 0;
                }
                if (name === 'napi_get_global') {
                  write32(args[1], addHandle(globalThis));
                  return 0;
                }
                if (name === 'napi_create_object') {
                  write32(args[1], addHandle({}));
                  return 0;
                }
                if (name === 'napi_create_array_with_length') {
                  write32(args[2], addHandle(new Array(args[1] >>> 0)));
                  return 0;
                }
                if (name === 'napi_create_array') {
                  write32(args[1], addHandle([]));
                  return 0;
                }
                if (name === 'napi_is_array') {
                  new DataView(memory.buffer).setUint8(
                    args[2] >>> 0,
                    Array.isArray(handles[args[1]]) ? 1 : 0,
                  );
                  return 0;
                }
                if (name === 'napi_is_typedarray') {
                  new DataView(memory.buffer).setUint8(
                    args[2] >>> 0,
                    ArrayBuffer.isView(handles[args[1]]) ? 1 : 0,
                  );
                  return 0;
                }
                if (name === 'napi_is_buffer') {
                  new DataView(memory.buffer).setUint8(
                    args[2] >>> 0,
                    handles[args[1]] instanceof Uint8Array ? 1 : 0,
                  );
                  return 0;
                }
                if (name === 'napi_get_arraybuffer_info') {
                  const value = handles[args[1]];
                  if (!ArrayBuffer.isView(value)) return 19;
                  if (args[2] !== 0) write32(args[2], value.byteOffset);
                  if (args[3] !== 0) write32(args[3], value.byteLength);
                  return 0;
                }
                if (name === 'napi_get_buffer_info') {
                  const value = handles[args[1]];
                  if (!(value instanceof Uint8Array)) return 1;
                  if (args[2] !== 0) write32(args[2], value.byteOffset);
                  if (args[3] !== 0) write32(args[3], value.byteLength);
                  return 0;
                }
                if (name === 'napi_get_typedarray_info') {
                  const value = handles[args[1]];
                  const constructors = [
                    Int8Array,
                    Uint8Array,
                    Uint8ClampedArray,
                    Int16Array,
                    Uint16Array,
                    Int32Array,
                    Uint32Array,
                    Float32Array,
                    Float64Array,
                    BigInt64Array,
                    BigUint64Array,
                  ];
                  if (args[2] !== 0) {
                    write32(args[2], constructors.findIndex((constructor) => value instanceof constructor));
                  }
                  if (args[3] !== 0) write32(args[3], value.length);
                  if (args[4] !== 0) write32(args[4], value.byteOffset);
                  if (args[5] !== 0) write32(args[5], addHandle(value));
                  if (args[6] !== 0) write32(args[6], value.byteOffset);
                  return 0;
                }
                if (name === 'napi_get_array_length') {
                  write32(args[2], handles[args[1]].length >>> 0);
                  return 0;
                }
                if (name === 'napi_get_element') {
                  write32(args[3], addHandle(handles[args[1]][args[2] >>> 0]));
                  return 0;
                }
                if (name === 'napi_set_element') {
                  handles[args[1]][args[2] >>> 0] = handles[args[3]];
                  return 0;
                }
                if (name === 'napi_get_undefined') {
                  write32(args[1], addHandle(undefined));
                  return 0;
                }
                if (name === 'napi_get_null') {
                  write32(args[1], addHandle(null));
                  return 0;
                }
                if (name === 'napi_get_boolean') {
                  write32(args[2], addHandle(args[1] !== 0));
                  return 0;
                }
                if (name === 'napi_create_string_utf8') {
                  write32(args[3], addHandle(readString(args[1], args[2])));
                  return 0;
                }
                if (name === 'napi_create_int32') {
                  write32(args[2], addHandle(args[1] | 0));
                  return 0;
                }
                if (name === 'napi_create_uint32') {
                  write32(args[2], addHandle(args[1] >>> 0));
                  return 0;
                }
                if (name === 'napi_create_double') {
                  write32(args[2], addHandle(args[1]));
                  return 0;
                }
                if (name === 'napi_create_int64') {
                  write32(args[2], addHandle(Number(args[1])));
                  return 0;
                }
                if (name === 'napi_typeof') {
                  const value = handles[args[1]];
                  let type = 6;
                  if (value === undefined) type = 0;
                  else if (value === null) type = 1;
                  else if (typeof value === 'boolean') type = 2;
                  else if (typeof value === 'number') type = 3;
                  else if (typeof value === 'string') type = 4;
                  else if (typeof value === 'symbol') type = 5;
                  else if (typeof value === 'function') type = 7;
                  else if (typeof value === 'bigint') type = 9;
                  write32(args[2], type);
                  return 0;
                }
                if (name === 'napi_coerce_to_string') {
                  write32(args[2], addHandle(String(handles[args[1]])));
                  return 0;
                }
                if (name === 'napi_coerce_to_number') {
                  write32(args[2], addHandle(Number(handles[args[1]])));
                  return 0;
                }
                if (name === 'napi_coerce_to_bool') {
                  write32(args[2], addHandle(Boolean(handles[args[1]])));
                  return 0;
                }
                if (name === 'napi_coerce_to_object') {
                  write32(args[2], addHandle(Object(handles[args[1]])));
                  return 0;
                }
                if (name === 'napi_get_value_double') {
                  new DataView(memory.buffer).setFloat64(args[2] >>> 0, Number(handles[args[1]]), true);
                  return 0;
                }
                if (name === 'napi_get_value_int32') {
                  new DataView(memory.buffer).setInt32(args[2] >>> 0, Number(handles[args[1]]) | 0, true);
                  return 0;
                }
                if (name === 'napi_get_value_uint32') {
                  write32(args[2], Number(handles[args[1]]) >>> 0);
                  return 0;
                }
                if (name === 'napi_get_value_int64') {
                  new DataView(memory.buffer).setBigInt64(
                    args[2] >>> 0,
                    BigInt(Math.trunc(Number(handles[args[1]]))),
                    true,
                  );
                  return 0;
                }
                if (name === 'napi_get_value_bool') {
                  new DataView(memory.buffer).setUint8(args[2] >>> 0, handles[args[1]] ? 1 : 0);
                  return 0;
                }
                if (name === 'napi_get_value_string_utf8') {
                  const value = String(handles[args[1]]);
                  const bytes = Array.from(value, (character) => character.charCodeAt(0) & 0xff);
                  if (args[2] === 0 || args[3] === 0) {
                    if (args[4] !== 0) write32(args[4], bytes.length);
                    return 0;
                  }
                  const copied = Math.min(bytes.length, (args[3] >>> 0) - 1);
                  new Uint8Array(memory.buffer, args[2] >>> 0, copied).set(bytes.slice(0, copied));
                  new DataView(memory.buffer).setUint8((args[2] >>> 0) + copied, 0);
                  if (args[4] !== 0) write32(args[4], copied);
                  return 0;
                }
                if (name === 'napi_is_exception_pending') {
                  new DataView(memory.buffer).setUint8(args[1] >>> 0, pendingException === null ? 0 : 1);
                  return 0;
                }
                if (name === 'napi_get_and_clear_last_exception') {
                  write32(args[1], addHandle(pendingException));
                  pendingException = null;
                  return 0;
                }
                if (name === 'napi_open_handle_scope') {
                  write32(args[1], 1);
                  return 0;
                }
                if (name === 'napi_close_handle_scope') return 0;
                if (name === 'napi_throw_error') {
                  pendingException = new Error(readCString(args[2]));
                  return 0;
                }
                if (name === 'napi_has_named_property') {
                  new DataView(memory.buffer).setUint8(
                    args[3] >>> 0,
                    readCString(args[2]) in handles[args[1]] ? 1 : 0,
                  );
                  return 0;
                }
                if (name === 'napi_get_named_property') {
                  write32(args[3], addHandle(handles[args[1]][readCString(args[2])]));
                  return 0;
                }
                if (name === 'napi_get_property') {
                  write32(args[3], addHandle(handles[args[1]][handles[args[2]]]));
                  return 0;
                }
                if (name === 'napi_get_property_names') {
                  write32(args[2], addHandle(Reflect.ownKeys(handles[args[1]])));
                  return 0;
                }
                if (name === 'napi_get_prototype') {
                  write32(args[2], addHandle(Object.getPrototypeOf(handles[args[1]])));
                  return 0;
                }
                if (name === 'napi_has_property') {
                  new DataView(memory.buffer).setUint8(
                    args[3] >>> 0,
                    handles[args[2]] in handles[args[1]] ? 1 : 0,
                  );
                  return 0;
                }
                if (name === 'napi_has_own_property') {
                  new DataView(memory.buffer).setUint8(
                    args[3] >>> 0,
                    Object.prototype.hasOwnProperty.call(handles[args[1]], handles[args[2]]) ? 1 : 0,
                  );
                  return 0;
                }
                if (name === 'napi_set_property') {
                  handles[args[1]][handles[args[2]]] = handles[args[3]];
                  return 0;
                }
                if (name === 'napi_delete_property') {
                  const deleted = delete handles[args[1]][handles[args[2]]];
                  if (args[3] !== 0) {
                    new DataView(memory.buffer).setUint8(args[3] >>> 0, deleted ? 1 : 0);
                  }
                  return 0;
                }
                if (name === 'napi_object_freeze') {
                  Object.freeze(handles[args[1]]);
                  return 0;
                }
                if (name === 'napi_strict_equals') {
                  new DataView(memory.buffer).setUint8(
                    args[3] >>> 0,
                    handles[args[1]] === handles[args[2]] ? 1 : 0,
                  );
                  return 0;
                }
                if (name === 'node_api_set_prototype') {
                  Object.setPrototypeOf(handles[args[1]], handles[args[2]]);
                  return 0;
                }
                if (name === 'unofficial_napi_create_private_symbol') {
                  write32(args[3], addHandle(Symbol(readString(args[1], args[2]))));
                  return 0;
                }
                if (name === 'unofficial_napi_contextify_compile_function') {
                  const view = new DataView(memory.buffer);
                  const sourceHandle = view.getUint32(args[1] >>> 0, true);
                  const source = String(handles[sourceHandle]);
                  const paramsValue = handles[args[7]];
                  const params = Array.isArray(paramsValue) ? paramsValue.map(String) : [];
                  try {
                    const compiled = Function(...params, source);
                    write32(args[9], addHandle({
                      function: compiled,
                      sourceURL: handles[args[2]],
                      sourceMapURL: undefined,
                    }));
                    return 0;
                  } catch (error) {
                    pendingException = error;
                    return 10;
                  }
                }
                if (name === 'napi_create_symbol') {
                  write32(args[2], addHandle(Symbol(handles[args[1]])));
                  return 0;
                }
                if (name === 'napi_set_named_property') {
                  handles[args[1]][readCString(args[2])] = handles[args[3]];
                  return 0;
                }
                if (name === 'napi_create_function') {
                  write32(args[5], addHandle(makeFunction(args[3], args[4])));
                  return 0;
                }
                if (name === 'napi_get_cb_info') {
                  const info = callbackInfos.get(args[1]);
                  if (!info) return 1;
                  const view = new DataView(memory.buffer);
                  const requested = args[2] === 0 ? 0 : view.getUint32(args[2] >>> 0, true);
                  if (args[3] !== 0) {
                    for (let index = 0; index < Math.min(requested, info.args.length); index += 1) {
                      write32((args[3] >>> 0) + index * 4, addHandle(info.args[index]));
                    }
                  }
                  if (args[2] !== 0) write32(args[2], info.args.length);
                  if (args[4] !== 0) write32(args[4], addHandle(info.thisValue));
                  if (args[5] !== 0) write32(args[5], info.data);
                  return 0;
                }
                if (name === 'napi_new_instance') {
                  const values = [];
                  for (let index = 0; index < (args[2] >>> 0); index += 1) {
                    const handle = new DataView(memory.buffer).getUint32(
                      (args[3] >>> 0) + index * 4,
                      true,
                    );
                    values.push(handles[handle]);
                  }
                  write32(args[4], addHandle(Reflect.construct(handles[args[1]], values)));
                  return 0;
                }
                if (name === 'napi_call_function') {
                  const values = [];
                  for (let index = 0; index < (args[3] >>> 0); index += 1) {
                    const handle = new DataView(memory.buffer).getUint32(
                      (args[4] >>> 0) + index * 4,
                      true,
                    );
                    values.push(handles[handle]);
                  }
                  try {
                    const result = handles[args[2]].apply(handles[args[1]], values);
                    if (args[5] !== 0) write32(args[5], addHandle(result));
                    return 0;
                  } catch (error) {
                    pendingException = error;
                    return 10;
                  }
                }
                if (name === 'napi_run_script') {
                  try {
                    const result = (0, eval)(String(handles[args[1]]));
                    write32(args[2], addHandle(result));
                    return 0;
                  } catch (error) {
                    pendingException = error;
                    return 10;
                  }
                }
                if (name === 'unofficial_napi_get_promise_details') {
                  if (!(handles[args[1]] instanceof Promise)) return 1;
                  write32(args[2], 0);
                  if (args[4] !== 0) {
                    new DataView(memory.buffer).setUint8(args[4] >>> 0, 0);
                  }
                  return 0;
                }
                if (name === 'napi_wrap') {
                  wrappedData.set(handles[args[1]], args[2] >>> 0);
                  if (args[5] !== 0) write32(args[5], addHandle(handles[args[1]]));
                  return 0;
                }
                if (name === 'napi_unwrap') {
                  write32(args[2], wrappedData.get(handles[args[1]]) ?? 0);
                  return 0;
                }
                if (name === 'napi_create_arraybuffer') {
                  const pointer = instance.exports.unofficial_napi_guest_malloc(args[1] >>> 0);
                  const value = new Uint8Array(memory.buffer, pointer, args[1] >>> 0);
                  if (args[2] !== 0) write32(args[2], pointer);
                  write32(args[3], addHandle(value));
                  return 0;
                }
                if (name === 'napi_create_external_arraybuffer') {
                  const value = new Uint8Array(memory.buffer, args[1] >>> 0, args[2] >>> 0);
                  write32(args[5], addHandle(value));
                  return 0;
                }
                if (name === 'napi_create_reference') {
                  const reference = nextReference++;
                  references.set(reference, { value: handles[args[1]], count: args[2] >>> 0 });
                  write32(args[3], reference);
                  return 0;
                }
                if (name === 'napi_get_reference_value') {
                  const reference = references.get(args[1]);
                  write32(args[2], addHandle(reference?.value));
                  return 0;
                }
                if (name === 'napi_reference_ref') {
                  const reference = references.get(args[1]);
                  if (!reference) return 1;
                  reference.count += 1;
                  if (args[2] !== 0) write32(args[2], reference.count);
                  return 0;
                }
                if (name === 'napi_reference_unref') {
                  const reference = references.get(args[1]);
                  if (!reference) return 1;
                  if (reference.count > 0) reference.count -= 1;
                  if (args[2] !== 0) write32(args[2], reference.count);
                  return 0;
                }
                if (name === 'napi_delete_reference') {
                  references.delete(args[1]);
                  return 0;
                }
                if (name === 'napi_create_typedarray') {
                  const constructors = [
                    Int8Array,
                    Uint8Array,
                    Uint8ClampedArray,
                    Int16Array,
                    Uint16Array,
                    Int32Array,
                    Uint32Array,
                    Float32Array,
                    Float64Array,
                    BigInt64Array,
                    BigUint64Array,
                  ];
                  const backing = handles[args[3]];
                  const constructor = constructors[args[1] >>> 0];
                  const value = new constructor(
                    backing.buffer,
                    backing.byteOffset + (args[4] >>> 0),
                    args[2] >>> 0,
                  );
                  write32(args[5], addHandle(value));
                  return 0;
                }
                if (name === 'napi_define_class') {
                  const constructor = makeFunction(args[3], args[4]);
                  const descriptorView = new DataView(memory.buffer);
                  for (let index = 0; index < (args[5] >>> 0); index += 1) {
                    const descriptor = (args[6] >>> 0) + index * 32;
                    const utf8Name = descriptorView.getUint32(descriptor, true);
                    const nameHandle = descriptorView.getUint32(descriptor + 4, true);
                    const attributes = descriptorView.getUint32(descriptor + 24, true);
                    const target = (attributes & 1024) !== 0 ? constructor : constructor.prototype;
                    defineProperties(target, 1, descriptor);
                  }
                  write32(args[7], addHandle(constructor));
                  return 0;
                }
                if (name === 'napi_define_properties') {
                  defineProperties(handles[args[1]], args[2], args[3]);
                  return 0;
                }
                if ([
                  'unofficial_napi_set_embedder_hooks',
                  'unofficial_napi_set_enqueue_foreground_task_callback',
                  'unofficial_napi_set_fatal_error_callbacks',
                  'unofficial_napi_set_source_maps_enabled',
                  'unofficial_napi_set_prepare_stack_trace_callback',
                  'unofficial_napi_process_microtasks',
                  'unofficial_napi_cancel_terminate_execution',
                  'unofficial_napi_release_env',
                ].includes(name)) {
                  return 0;
                }
                throw new Error(`unimplemented typed Node reactor import: ${name}`);
              };
            },
          });
          memory = new WebAssembly.Memory({
            initial: 1024,
            maximum: 4096,
            shared: true,
          });
          const imports = Object.freeze({
            agentos_napi_v1: unimplemented,
            agentos_node_engine_v1: unimplemented,
            agentos_posix_v1: unimplemented,
            env: Object.freeze({ memory }),
          });

          instance = new WebAssembly.Instance(module, imports);
          instance.exports._initialize();
          let createStatus = null;
          let createError = null;
          try {
            createStatus = instance.exports.agentos_node_runtime_create();
          } catch (error) {
            createError = `${error?.constructor?.name}:${error?.message}`;
          }
          let bootstrapStatus = null;
          let bootstrapError = null;
          let reactorError = null;
          if (createStatus === 0) {
            const allocationPointers = [];
            for (let index = 0; index < 32; index += 1) {
              const pointer = instance.exports.agentos_node_runtime_alloc(1);
              if (pointer === 0) throw new Error(`bounded allocation ${index} failed early`);
              allocationPointers.push(pointer);
            }
            if (instance.exports.agentos_node_runtime_allocation_count() !== 32 ||
                instance.exports.agentos_node_runtime_allocated_bytes() !== 32) {
              throw new Error('reactor allocation accounting drifted');
            }
            if (instance.exports.agentos_node_runtime_alloc(1) !== 0) {
              throw new Error('reactor accepted allocation 33 above its frozen limit');
            }
            for (const pointer of allocationPointers) {
              if (instance.exports.agentos_node_runtime_free(pointer) !== 0) {
                throw new Error('reactor rejected a live allocation during cleanup');
              }
            }
            if (instance.exports.agentos_node_runtime_free(allocationPointers[0]) !== -1 ||
                instance.exports.agentos_node_runtime_allocation_count() !== 0 ||
                instance.exports.agentos_node_runtime_allocated_bytes() !== 0) {
              throw new Error('reactor did not reject double free without accounting drift');
            }
            const sourcePointer = instance.exports.agentos_node_runtime_alloc(1);
            new Uint8Array(memory.buffer)[sourcePointer] = ';'.charCodeAt(0);
            try {
              bootstrapStatus = instance.exports.agentos_node_runtime_bootstrap(sourcePointer, 1);
            } catch (error) {
              bootstrapError = `${error?.constructor?.name}:${error?.message}`;
            }
            instance.exports.agentos_node_runtime_free(sourcePointer);
            const errorPointer = instance.exports.agentos_node_runtime_alloc(65536);
            const errorLength = instance.exports.agentos_node_runtime_last_error(
              errorPointer,
              65536,
            );
            if (errorLength >= 0) reactorError = readString(errorPointer, errorLength);
            instance.exports.agentos_node_runtime_free(errorPointer);
          }
          if (createStatus !== 0 || createError !== null ||
              bootstrapStatus !== 0 || bootstrapError !== null || reactorError !== '') {
            throw new Error(`reactor bootstrap failed create=${createStatus}/${createError} bootstrap=${bootstrapStatus}/${bootstrapError} reactorError=${reactorError}: ${JSON.stringify(trace)}`);
          }
          if (instance.exports.agentos_node_runtime_quiescence() !== 1) {
            throw new Error('freshly bootstrapped Node reactor is not quiescent');
          }
          const runSource = 'globalThis.__agentOSNodeReactorRunMarker = 42;';
          const runPointer = instance.exports.agentos_node_runtime_alloc(runSource.length);
          for (let index = 0; index < runSource.length; index += 1) {
            new Uint8Array(memory.buffer)[runPointer + index] = runSource.charCodeAt(index);
          }
          const runStatus = instance.exports.agentos_node_runtime_run(
            runPointer,
            runSource.length,
          );
          instance.exports.agentos_node_runtime_free(runPointer);
          if (runStatus !== 0 || globalThis.__agentOSNodeReactorRunMarker !== 42) {
            throw new Error(`persistent Node reactor run failed with ${runStatus}`);
          }
          delete globalThis.__agentOSNodeReactorRunMarker;
          for (const name of [
            'agentos_node_runtime_create',
            'agentos_node_runtime_allocated_bytes',
            'agentos_node_runtime_allocation_count',
            'agentos_node_runtime_bootstrap',
            'agentos_node_runtime_run',
            'agentos_node_runtime_interrupt',
            'agentos_node_runtime_quiescence',
            'agentos_node_runtime_teardown',
          ]) {
            if (typeof instance.exports[name] !== 'function') {
              throw new Error(`missing reactor export ${name}`);
            }
          }
          const teardownStatus = instance.exports.agentos_node_runtime_teardown();
          if (teardownStatus !== 0) {
            throw new Error(`Node reactor teardown failed with ${teardownStatus}: ${JSON.stringify(trace)}`);
          }
          instance = null;
          if ('agentos_napi_v1' in globalThis ||
              'agentos_node_engine_v1' in globalThis ||
              'agentos_posix_v1' in globalThis ||
              '__agentOSWasmModuleBytes' in globalThis) {
            throw new Error('closure-private Node reactor state leaked onto globalThis');
          }
        })();
        "#,
        Some(module_bytes),
    )?;
    assert_execution_ok(&receiver, &session_id);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    wait_until("expected Node reactor session to drain", || {
        runtime.session_count() == 0 && runtime.active_slot_count() == 0
    });
    Ok(())
}

fn assert_pthread_probe_runs_on_v8_worker_isolates() -> io::Result<()> {
    let wasm_path = std::env::var_os("AGENTOS_PTHREAD_PROBE_WASM").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "AGENTOS_PTHREAD_PROBE_WASM must name the built pthread probe",
        )
    })?;
    let module_bytes = std::fs::read(wasm_path)?;
    *pthread_probe_manager()
        .lock()
        .expect("pthread probe manager lock poisoned") = None;

    isolate::init_v8_platform();
    let mut main_isolate = isolate::create_isolate(Some(64));
    let context = isolate::create_context(&mut main_isolate);
    {
        let scope = &mut v8::HandleScope::new(&mut main_isolate);
        let context = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let try_catch = &mut v8::TryCatch::new(scope);
        let module = v8::WasmModuleObject::compile(try_catch, &module_bytes)
            .expect("V8 should compile the pthread probe");
        let register = v8::Function::new(try_catch, pthread_probe_register_callback)
            .expect("pthread register callback should compile");
        let spawn = v8::Function::new(try_catch, pthread_probe_spawn_callback)
            .expect("pthread spawn callback should compile");
        let source = v8::String::new(
            try_catch,
            r#"(function(module, registerThreadRuntime, spawnThread) {
              const memory = new WebAssembly.Memory({
                initial: 1024,
                maximum: 4096,
                shared: true,
              });
              if (registerThreadRuntime(module, memory) !== true) {
                throw new Error('pthread runtime registration failed');
              }
              const imports = Object.freeze({
                agentos_posix_v1: Object.freeze({
                  proc_exit(status) { throw new Error(`main proc_exit(${status})`); },
                  sched_yield() { return 0; },
                  'thread-spawn': spawnThread,
                }),
                env: Object.freeze({ memory }),
              });
              const instance = new WebAssembly.Instance(module, imports);
              instance.exports._initialize();
              const failures = instance.exports.agentos_pthread_probe_run();
              if (failures !== 0) {
                throw new Error(`pthread probe failure bitset: 0x${failures.toString(16)}`);
              }
              return failures;
            })"#,
        )
        .unwrap();
        let script = v8::Script::compile(try_catch, source, None)
            .expect("pthread main trampoline should compile");
        let function = script
            .run(try_catch)
            .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
            .expect("pthread main trampoline should be callable");
        let undefined = v8::undefined(try_catch).into();
        let result = function.call(
            try_catch,
            undefined,
            &[module.into(), register.into(), spawn.into()],
        );
        if result.is_none() {
            let message = try_catch
                .exception()
                .and_then(|exception| exception.to_string(try_catch))
                .map(|message| message.to_rust_string_lossy(try_catch))
                .unwrap_or_else(|| "unknown pthread probe exception".into());
            panic!("pthread probe failed in the main V8 isolate: {message}");
        }
        assert_eq!(result.unwrap().int32_value(try_catch), Some(0));
    }

    let manager = pthread_probe_manager()
        .lock()
        .expect("pthread probe manager lock poisoned")
        .take()
        .expect("pthread probe should register the production worker manager");
    manager
        .shutdown()
        .expect("production worker manager should join all pthread probe workers");
    assert_eq!(manager.spawned_worker_count(), 2);
    assert_eq!(manager.active_runtime_threads(), 1);
    isolate::drop_isolate(Some(main_isolate));
    Ok(())
}

fn assert_wasm_worker_compute_is_terminated_and_joined() -> io::Result<()> {
    let module_bytes = wat::parse_str(
        r#"
(module
  (memory (import "env" "memory") 1 1 shared)
  (func (export "wasi_thread_start") (param i32 i32)
    (loop $spin
      (br $spin)
    )
  )
)
"#,
    )
    .expect("compile worker termination probe");

    isolate::init_v8_platform();
    let mut root_isolate = isolate::create_isolate(Some(64));
    let context = isolate::create_context(&mut root_isolate);
    let executor = {
        let scope = &mut v8::HandleScope::new(&mut root_isolate);
        let context = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let module = v8::WasmModuleObject::compile(scope, &module_bytes)
            .expect("V8 should compile the worker termination probe");
        let source = v8::String::new(
            scope,
            "new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true })",
        )
        .unwrap();
        let memory = v8::Script::compile(scope, source, None)
            .and_then(|script| script.run(scope))
            .and_then(|value| v8::Local::<v8::WasmMemoryObject>::try_from(value).ok())
            .expect("create shared WebAssembly.Memory in the root isolate");
        V8SharedWasmWorkerExecutor::capture(
            scope,
            module,
            memory,
            TERMINATION_PROBE_WORKER_BOOTSTRAP,
            Some(64),
        )
        .expect("capture compiled module and shared memory")
    };
    let manager = WasmWorkerManager::new(
        WasmWorkerLimits {
            teardown_grace: Duration::from_secs(1),
            ..WasmWorkerLimits::default()
        },
        Arc::new(executor),
    )
    .expect("valid worker limits");
    manager.spawn(0).expect("start compute-bound WASM worker");
    thread::sleep(Duration::from_millis(25));

    let started = Instant::now();
    manager
        .shutdown()
        .expect("V8 termination should unwind and join compute-bound WASM");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "compute-bound WASM worker exceeded its teardown grace"
    );
    assert_eq!(manager.active_runtime_threads(), 1);
    isolate::drop_isolate(Some(root_isolate));
    Ok(())
}

fn assert_queued_work_waits_for_slot_release() -> io::Result<()> {
    let runtime = Arc::new(EmbeddedV8Runtime::new(Some(1))?);
    let session_a = next_session_id();
    let session_b = next_session_id();
    let receiver_a = register_and_create_session(&runtime, &session_a)?;

    wait_until(
        "expected the first embedded session to occupy the only slot before the second session is created",
        || runtime.active_slot_count() == 1 && runtime.session_count() == 1,
    );

    dispatch_execute(
        runtime.as_ref(),
        &session_a,
        1,
        "",
        "await new Promise(() => {});",
    )?;

    let receiver_b = register_and_create_session(&runtime, &session_b)?;
    dispatch_execute(
        runtime.as_ref(),
        &session_b,
        0,
        "(function() { globalThis.__queuedSession = 'released'; })();",
        "if (globalThis.__queuedSession !== 'released') { throw new Error(`saw ${globalThis.__queuedSession}`); }",
    )?;

    wait_until(
        "expected one active slot with the second session still queued",
        || runtime.active_slot_count() == 1 && runtime.session_count() == 2,
    );
    if run_timing_sensitive_tests() {
        assert!(
            receiver_b.recv_timeout(Duration::from_millis(150)).is_err(),
            "queued session should not emit an execution result before the first slot is released"
        );
    }

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_a.clone(),
    })?;
    let terminated = wait_for_execution_result(&receiver_a, &session_a);
    assert!(
        matches!(
            terminated,
            RuntimeEvent::ExecutionResult {
                exit_code: 1,
                ref error,
                ..
            } if error.as_ref().is_some_and(|error| error.message == "Execution terminated")
        ),
        "destroying the in-flight session should terminate its pending execution"
    );

    assert_execution_ok(&receiver_b, &session_b);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_b.clone(),
    })?;
    runtime.unregister_session(&session_a);
    runtime.unregister_session(&session_b);
    wait_until(
        "expected all embedded sessions and slots to drain after teardown",
        || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
    );
    Ok(())
}

fn assert_shared_runtime_handles_share_concurrency_quota() -> io::Result<()> {
    let runtime = Arc::new(EmbeddedV8Runtime::new(Some(3))?);
    let quota_bridge = "(function() { globalThis.__sharedQuota = 'released'; })();";
    let clients = (0..4)
        .map(|_| Arc::clone(&runtime))
        .collect::<Vec<Arc<EmbeddedV8Runtime>>>();
    let session_ids = (0..4).map(|_| next_session_id()).collect::<Vec<_>>();
    let mut receivers = clients
        .iter()
        .zip(session_ids.iter())
        .take(3)
        .map(|(client, session_id)| register_and_create_session(client, session_id))
        .collect::<io::Result<Vec<_>>>()?;

    wait_until(
        "expected the first three embedded sessions to occupy the shared slots before the fourth session is created",
        || runtime.active_slot_count() == 3 && runtime.session_count() == 3,
    );

    receivers.push(register_and_create_session(&clients[3], &session_ids[3])?);

    for (client, session_id) in clients.iter().zip(session_ids.iter()).take(3) {
        dispatch_execute(
            client.as_ref(),
            session_id,
            1,
            "",
            "await new Promise(() => {});",
        )?;
    }
    dispatch_execute(
        clients[3].as_ref(),
        &session_ids[3],
        0,
        quota_bridge,
        "if (globalThis.__sharedQuota !== 'released') { throw new Error(`saw ${globalThis.__sharedQuota}`); }",
    )?;

    wait_until(
        "expected one runtime-wide slot budget shared across all embedded runtime handles",
        || runtime.active_slot_count() == 3 && runtime.session_count() == 4,
    );
    if run_timing_sensitive_tests() {
        assert!(
            receivers[3]
                .recv_timeout(Duration::from_millis(150))
                .is_err(),
            "the fourth client should stay queued while the first three handles occupy the shared slots"
        );
    }

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_ids[0].clone(),
    })?;
    let terminated = wait_for_execution_result(&receivers[0], &session_ids[0]);
    assert!(
        matches!(
            terminated,
            RuntimeEvent::ExecutionResult {
                exit_code: 1,
                ref error,
                ..
            } if error.as_ref().is_some_and(|error| error.message == "Execution terminated")
        ),
        "destroying one in-flight session should release a shared slot for queued handles"
    );

    assert_execution_ok(&receivers[3], &session_ids[3]);

    for session_id in session_ids.iter().skip(1) {
        runtime.dispatch(RuntimeCommand::DestroySession {
            session_id: session_id.clone(),
        })?;
    }
    for session_id in &session_ids {
        runtime.unregister_session(session_id);
    }
    wait_until(
        "expected all shared-runtime sessions and slots to drain after teardown",
        || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
    );
    Ok(())
}

fn assert_terminate_interrupts_sync_bridge_wait() -> io::Result<()> {
    let runtime = Arc::new(EmbeddedV8Runtime::new(Some(1))?);
    let session_id = next_session_id();
    let receiver = register_and_create_session(&runtime, &session_id)?;

    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        "",
        "_loadFileSync('/never-responds');",
    )?;

    let bridge_call = wait_for_bridge_call(&receiver, &session_id);
    assert!(
        matches!(
            bridge_call,
            RuntimeEvent::BridgeCall { ref method, .. } if method == "_loadFileSync"
        ),
        "expected the blocked sync bridge call to be visible before termination"
    );

    let terminate_started = Instant::now();
    runtime.session_handle(session_id.clone()).terminate()?;
    let terminated = wait_for_execution_result(&receiver, &session_id);

    if run_timing_sensitive_tests() {
        assert!(
            terminate_started.elapsed() < Duration::from_secs(1),
            "terminate() should return promptly while the sync bridge call is blocked"
        );
    }
    assert!(
        matches!(
            terminated,
            RuntimeEvent::ExecutionResult {
                exit_code: 1,
                ref error,
                ..
            } if error.as_ref().is_some_and(|error| error.message == "Execution terminated")
        ),
        "terminate() should interrupt a blocked sync bridge call instead of waiting for a host response"
    );

    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        "",
        "globalThis.__afterExplicitTerminate = 'ok';",
    )?;
    assert_execution_ok(&receiver, &session_id);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    wait_until(
        "expected the terminated sync-bridge session to drain cleanly",
        || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
    );
    Ok(())
}

fn assert_cpu_terminated_session_can_execute_again() -> io::Result<()> {
    let runtime = Arc::new(EmbeddedV8Runtime::new(Some(1))?);
    let session_id = next_session_id();
    let receiver =
        register_and_create_session_with_cpu_time_limit(&runtime, &session_id, Some(25))?;

    dispatch_execute(runtime.as_ref(), &session_id, 0, "", "while (true) {}")?;
    let terminated = wait_for_execution_result(&receiver, &session_id);
    assert!(
        matches!(
            terminated,
            RuntimeEvent::ExecutionResult {
                exit_code: 1,
                ref error,
                ..
            } if error
                .as_ref()
                .is_some_and(|error| error.code == "ERR_SCRIPT_CPU_BUDGET_EXCEEDED")
        ),
        "CPU-budget termination should be attributed before reuse"
    );

    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        "",
        "globalThis.__afterCpuTerminate = 'ok';",
    )?;
    assert_execution_ok(&receiver, &session_id);

    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    wait_until(
        "expected CPU-terminated session to drain cleanly after reuse",
        || runtime.session_count() == 0 && runtime.active_slot_count() == 0,
    );
    Ok(())
}

fn assert_isolate_churn_recreates_embedded_sessions_without_segv() -> io::Result<()> {
    let runtime = Arc::new(EmbeddedV8Runtime::new(Some(1))?);
    let bridge_code = "(function() { globalThis.__churnBridgeReady = true; })();";

    for _ in 0..32 {
        let session_id = next_session_id();
        let receiver = register_and_create_session(&runtime, &session_id)?;
        dispatch_execute(
            runtime.as_ref(),
            &session_id,
            0,
            bridge_code,
            "if (globalThis.__churnBridgeReady !== true) { throw new Error('missing bridge'); }",
        )?;
        assert_execution_ok(&receiver, &session_id);
        runtime.dispatch(RuntimeCommand::DestroySession {
            session_id: session_id.clone(),
        })?;
        runtime.unregister_session(&session_id);
    }

    let session_id = next_session_id();
    let receiver = register_and_create_session(&runtime, &session_id)?;
    dispatch_execute(
        runtime.as_ref(),
        &session_id,
        0,
        bridge_code,
        "globalThis.__afterChurn = 42;",
    )?;
    assert_execution_ok(&receiver, &session_id);
    runtime.dispatch(RuntimeCommand::DestroySession {
        session_id: session_id.clone(),
    })?;
    runtime.unregister_session(&session_id);
    wait_until("expected isolate churn sessions to drain", || {
        runtime.session_count() == 0 && runtime.active_slot_count() == 0
    });
    Ok(())
}

#[test]
fn embedded_runtime_session_consolidated_behaviors() -> io::Result<()> {
    // Keep the embedded-runtime coverage in one test process. V8 teardown across
    // multiple integration tests still trips intermittent SIGSEGVs in this crate.
    if let Ok(case) = std::env::var("AGENTOS_V8_SESSION_CASE") {
        return match case.as_str() {
            "create-destroy" => assert_create_destroy_reuses_session_ids(),
            "warmed-snapshot" => assert_warmed_snapshot_bridge_state(),
            "snapshot-bridge-change" => assert_snapshot_rebuild_on_bridge_change(),
            "oversized-bridge" => assert_execute_rejects_oversized_bridge_code(),
            "zero-cpu-limit" => assert_direct_zero_cpu_time_limit_disables_timeout(),
            "nested-node-runtime-probe" => assert_nested_node_runtime_probe_uses_same_isolate(),
            "v8-wasm-features" => assert_v8_wasm_required_features(),
            "node-reactor-instantiation" => {
                assert_node_reactor_instantiates_in_existing_v8_isolate()
            }
            "pthread-runtime-probe" => assert_pthread_probe_runs_on_v8_worker_isolates(),
            "wasm-worker-termination" => assert_wasm_worker_compute_is_terminated_and_joined(),
            "queued-slot-release" => assert_queued_work_waits_for_slot_release(),
            "shared-runtime-quota" => assert_shared_runtime_handles_share_concurrency_quota(),
            "terminate-sync-bridge" => assert_terminate_interrupts_sync_bridge_wait(),
            "cpu-termination-reuse" => assert_cpu_terminated_session_can_execute_again(),
            "isolate-churn" => assert_isolate_churn_recreates_embedded_sessions_without_segv(),
            _ => panic!("unknown AGENTOS_V8_SESSION_CASE: {case}"),
        };
    }
    assert_create_destroy_reuses_session_ids()?;
    assert_warmed_snapshot_bridge_state()?;
    assert_snapshot_rebuild_on_bridge_change()?;
    assert_execute_rejects_oversized_bridge_code()?;
    assert_direct_zero_cpu_time_limit_disables_timeout()?;
    assert_nested_node_runtime_probe_uses_same_isolate()?;
    assert_v8_wasm_required_features()?;
    assert_queued_work_waits_for_slot_release()?;
    assert_shared_runtime_handles_share_concurrency_quota()?;
    assert_terminate_interrupts_sync_bridge_wait()?;
    assert_cpu_terminated_session_can_execute_again()?;
    assert_isolate_churn_recreates_embedded_sessions_without_segv()?;
    Ok(())
}
