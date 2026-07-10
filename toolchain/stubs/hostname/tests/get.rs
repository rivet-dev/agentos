use std::ffi::OsString;

#[test]
fn get_returns_fixed_guest_safe_hostname() {
    let hostname = hostname::get().expect("stub hostname should be available");

    assert_eq!(hostname, OsString::from("wasm-host"));
}
