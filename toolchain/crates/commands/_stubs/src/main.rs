fn main() {
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(secureexec_stubs::run(&args));
}
