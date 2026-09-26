//! Wasmtime process ABI lifecycle regressions.
#[path = "common/process_lifecycle.rs"]
mod harness;
use harness::run;

#[test]
fn getppid_observes_kernel_reparenting() {
    let module = wat::parse_str(
        r#"(module
        (import "host_process" "proc_getppid" (func $getppid (param i32) (result i32)))
        (memory (export "memory") 1)
        (func (export "_start")
            (if (call $getppid (i32.const 0)) (then unreachable))
            (if (i32.ne (i32.load (i32.const 0)) (i32.const 7)) (then unreachable))
            (if (call $getppid (i32.const 0)) (then unreachable))
            (if (i32.ne (i32.load (i32.const 0)) (i32.const 1)) (then unreachable))))"#,
    )
    .unwrap();
    let result = run(&module, None);
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
}

fn exec_caller() -> Vec<u8> {
    wat::parse_str(r#"(module
        (import "host_process" "proc_exec" (func $exec (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
        (memory (export "memory") 1)
        (data (i32.const 0) "/replacement.wasm\00")
        (func (export "_start")
            (if (i32.ne
                (call $exec (i32.const 0) (i32.const 17) (i32.const 0) (i32.const 18)
                    (i32.const 64) (i32.const 0) (i32.const 64) (i32.const 0))
                (i32.const 45)) (then unreachable))))"#).unwrap()
}

#[test]
fn exec_rejects_unlinkable_image_before_host_commit() {
    let caller = exec_caller();
    let replacement = wat::parse_str(
        r#"(module
        (import "unavailable" "function" (func))
        (func (export "_start")))"#,
    )
    .unwrap();
    let result = run(&caller, Some(&replacement));
    assert_eq!(
        result.exec_commits, 0,
        "an unexecutable image must not commit: {}",
        result.stderr
    );
    assert_eq!(
        result.exit_code, 0,
        "exec must return ENOEXEC to its caller: {}",
        result.stderr
    );
}

#[test]
fn exec_rejects_missing_entrypoint_before_host_commit() {
    let caller = exec_caller();
    let replacement = wat::parse_str("(module (func (export \"not_start\")))").unwrap();
    let result = run(&caller, Some(&replacement));
    assert_eq!(
        result.exec_commits, 0,
        "missing _start must not commit: {}",
        result.stderr
    );
    assert_eq!(
        result.exit_code, 0,
        "exec must return ENOEXEC: {}",
        result.stderr
    );
}

#[test]
fn exec_rejects_wrong_import_type_before_host_commit() {
    let caller = exec_caller();
    let replacement = wat::parse_str(
        r#"(module
        (import "host_process" "proc_getppid" (func (param i64) (result i64)))
        (func (export "_start")))"#,
    )
    .unwrap();
    let result = run(&caller, Some(&replacement));
    assert_eq!(result.exec_commits, 0, "{}", result.stderr);
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
}
