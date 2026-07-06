use agentos_execution::{scaffold, GuestRuntime};

#[test]
fn execution_scaffold_is_native_and_depends_on_kernel() {
    let scaffold = scaffold();

    assert_eq!(scaffold.package_name, "agentos-execution");
    assert_eq!(scaffold.kernel_package, "agentos-kernel");
    assert_eq!(scaffold.target, "native");
    assert_eq!(
        scaffold.planned_guest_runtimes,
        [GuestRuntime::JavaScript, GuestRuntime::WebAssembly]
    );
}
