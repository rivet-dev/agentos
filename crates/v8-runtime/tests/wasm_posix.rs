use agentos_v8_runtime::{isolate, wasm_posix::V8PosixImportObject};
use agentos_wasm_posix_host::{
    GuestMemory, KernelDispatcher, PosixProvider, ProviderError, ProviderLimits, SyscallDescriptor,
    WasmValue,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

struct TestKernel {
    calls: Arc<AtomicUsize>,
}

impl KernelDispatcher for TestKernel {
    fn invoke(
        &mut self,
        syscall: &'static SyscallDescriptor,
        arguments: &[WasmValue],
        memory: &mut GuestMemory<'_>,
    ) -> Result<Option<WasmValue>, ProviderError> {
        assert_eq!(syscall.name, "fd_write");
        assert_eq!(
            arguments,
            &[
                WasmValue::I32(1),
                WasmValue::I32(0),
                WasmValue::I32(1),
                WasmValue::I32(8),
            ]
        );
        let iovec_pointer = memory.read_u32_le(0)?;
        let iovec_length = memory.read_u32_le(4)?;
        assert_eq!(memory.copy_in(iovec_pointer, iovec_length)?, b"hello");
        memory.write_u32_le(8, iovec_length)?;
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(Some(WasmValue::I32(0)))
    }
}

#[test]
fn generated_v8_object_dispatches_wasm_import_through_shared_provider() {
    isolate::init_v8_platform();
    let mut isolate = isolate::create_isolate(Some(64));
    let context = isolate::create_context(&mut isolate);
    let calls = Arc::new(AtomicUsize::new(0));
    {
        let scope = &mut v8::HandleScope::new(&mut isolate);
        let context = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let module_bytes = wat::parse_str(
            r#"(module
              (import "env" "memory" (memory 1 1 shared))
              (import "agentos_posix_v1" "fd_write"
                (func $fd_write (param i32 i32 i32 i32) (result i32)))
              (data (i32.const 16) "hello")
              (func (export "run") (result i32)
                i32.const 0 i32.const 16 i32.store
                i32.const 4 i32.const 5 i32.store
                i32.const 1 i32.const 0 i32.const 1 i32.const 8
                call $fd_write
                if (result i32)
                  i32.const -1
                else
                  i32.const 8 i32.load
                end))"#,
        )
        .unwrap();
        let module = v8::WasmModuleObject::compile(scope, &module_bytes).unwrap();
        let memory_source = v8::String::new(
            scope,
            "new WebAssembly.Memory({initial:1, maximum:1, shared:true})",
        )
        .unwrap();
        let memory = v8::Script::compile(scope, memory_source, None)
            .and_then(|script| script.run(scope))
            .and_then(|value| v8::Local::<v8::WasmMemoryObject>::try_from(value).ok())
            .unwrap();
        let provider = Arc::new(PosixProvider::new(ProviderLimits::default()).unwrap());
        let kernel: Arc<Mutex<Box<dyn KernelDispatcher>>> =
            Arc::new(Mutex::new(Box::new(TestKernel {
                calls: Arc::clone(&calls),
            })));
        let imports = V8PosixImportObject::new(scope, memory, provider, kernel).unwrap();
        let factory_source = v8::String::new(
            scope,
            "(module,memory,posix) => new WebAssembly.Instance(module, {env:{memory}, agentos_posix_v1:posix}).exports.run()",
        )
        .unwrap();
        let factory = v8::Script::compile(scope, factory_source, None)
            .and_then(|script| script.run(scope))
            .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
            .unwrap();
        let undefined = v8::undefined(scope).into();
        let posix = imports.local(scope);
        let result = factory
            .call(
                scope,
                undefined,
                &[module.into(), memory.into(), posix.into()],
            )
            .unwrap();
        assert_eq!(result.int32_value(scope), Some(5));
    }
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    isolate::drop_isolate(Some(isolate));
}
