fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    std::process::exit(secureexec_builtins::test_cmd(args));
}
