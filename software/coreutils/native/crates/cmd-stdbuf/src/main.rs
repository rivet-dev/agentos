fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    std::process::exit(shims::stdbuf::stdbuf(args));
}
