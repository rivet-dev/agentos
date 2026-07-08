fn main() {
    use std::io::Write;

    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let mut code = secureexec_grep::main(args);
    if let Err(error) = std::io::stdout().flush() {
        eprintln!("Error flushing stdout: {error}");
        if code == 0 {
            code = 2;
        }
    }
    std::process::exit(code);
}
