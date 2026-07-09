use std::env;
use std::process::ExitCode;

const HOSTNAME: &str = "agentos";

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        None | Some("-s" | "--short" | "-f" | "--fqdn" | "--long") => {
            if args.next().is_some() {
                return usage_error("extra operand");
            }
            println!("{HOSTNAME}");
            ExitCode::SUCCESS
        }
        Some("-d" | "--domain") => {
            if args.next().is_some() {
                return usage_error("extra operand");
            }
            println!();
            ExitCode::SUCCESS
        }
        Some("-h" | "--help") => {
            println!("Usage: hostname [-s|--short] [-f|--fqdn] [-d|--domain]");
            ExitCode::SUCCESS
        }
        Some("-V" | "--version") => {
            println!("hostname (AgentOS) 0.0.1");
            ExitCode::SUCCESS
        }
        Some(value) if value.starts_with('-') => usage_error(&format!("unknown option {value}")),
        Some(_) => {
            eprintln!("hostname: setting the hostname is not permitted");
            ExitCode::from(1)
        }
    }
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("hostname: {message}");
    ExitCode::from(1)
}
