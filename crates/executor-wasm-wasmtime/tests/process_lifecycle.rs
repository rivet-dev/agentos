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

#[test]
fn exec_direct_replacement_preserves_remaining_fuel() {
    let initial = wat::parse_str(r#"(module
        (import "host_process" "proc_exec" (func $exec (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
        (memory (export "memory") 1)
        (data (i32.const 0) "/replacement.wasm\00")
        (func (export "_start") (local $i i32)
            (loop $work (local.set $i (i32.add (local.get $i) (i32.const 1)))
                (br_if $work (i32.lt_u (local.get $i) (i32.const 500))))
            (drop (call $exec (i32.const 0) (i32.const 17) (i32.const 0) (i32.const 18)
                (i32.const 64) (i32.const 0) (i32.const 64) (i32.const 0)))))"#).unwrap();
    let replacement = wat::parse_str(
        r#"(module
        (func (export "_start") (local $i i32)
            (loop $work (local.set $i (i32.add (local.get $i) (i32.const 1)))
                (br_if $work (i32.lt_u (local.get $i) (i32.const 500))))))"#,
    )
    .unwrap();
    let result =
        harness::run_with_threads_and_fuel(&initial, Some(&replacement), false, Some(6000));
    assert_eq!(result.exec_commits, 1, "{}", result.stderr);
    assert_ne!(
        result.exit_code, 0,
        "exec must not reset the fuel allowance"
    );
    assert!(result.stderr.contains("FUEL"), "{}", result.stderr);
}

#[test]
fn exhausted_cpu_does_not_run_start_section_exec() {
    let initial = wat::parse_str(r#"(module
        (import "host_process" "proc_exec" (func $exec (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
        (memory (export "memory") 1)
        (data (i32.const 0) "/replacement.wasm\00")
        (func $init
            (drop (call $exec (i32.const 0) (i32.const 17) (i32.const 0) (i32.const 18)
                (i32.const 64) (i32.const 0) (i32.const 64) (i32.const 0))))
        (start $init)
        (func (export "_start")))"#).unwrap();
    let replacement = wat::parse_str("(module (func (export \"_start\")))").unwrap();
    let outcome = harness::run_with_limits(&initial, Some(&replacement), false, None, Some(0));
    assert_eq!(outcome.exec_commits, 0, "{}", outcome.stderr);
    assert_ne!(outcome.exit_code, 0);
    assert!(
        outcome
            .stderr
            .contains("ERR_AGENTOS_WASMTIME_ACTIVE_CPU_LIMIT"),
        "{}",
        outcome.stderr
    );
}
