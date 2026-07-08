fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("--help") => {
            println!("Usage: xu [ARG ...]");
            return;
        }
        Some("--version") => {
            println!("xu 0.1.0");
            return;
        }
        _ => {}
    }

    println!("xu-ok:{}", args.join(" "));
}
