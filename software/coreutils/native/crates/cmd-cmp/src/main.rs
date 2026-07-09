use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::PathBuf;

const BUFFER_SIZE: usize = 4 * 1024 * 1024;

#[derive(Debug, Default, PartialEq, Eq)]
struct Options {
    silent: bool,
    verbose: bool,
    print_bytes: bool,
    limit: Option<u64>,
    ignore_initial: [u64; 2],
}

#[derive(Debug, PartialEq, Eq)]
struct Invocation {
    options: Options,
    files: [PathBuf; 2],
    skips: [u64; 2],
}

enum ParseResult {
    Run(Invocation),
    Exit(i32),
}

fn usage() {
    println!("Usage: cmp [OPTION]... FILE1 [FILE2 [SKIP1 [SKIP2]]]");
}

fn parse_number(value: &str) -> Result<u64, String> {
    let split = value
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(value.len());
    let (digits, suffix) = value.split_at(split);
    if digits.is_empty() {
        return Err(format!("invalid number: {value}"));
    }

    let number = digits
        .parse::<u64>()
        .map_err(|_| format!("invalid number: {value}"))?;
    let multiplier = match suffix {
        "" => 1,
        "kB" => 1_000,
        "K" | "KiB" => 1_024,
        "MB" => 1_000_000,
        "M" | "MiB" => 1_048_576,
        "GB" => 1_000_000_000,
        "G" | "GiB" => 1_073_741_824,
        "TB" => 1_000_000_000_000,
        "T" | "TiB" => 1_099_511_627_776,
        _ => return Err(format!("invalid number: {value}")),
    };
    number
        .checked_mul(multiplier)
        .ok_or_else(|| format!("number too large: {value}"))
}

fn required_option_value(
    args: &[OsString],
    index: &mut usize,
    attached: Option<&str>,
    option: &str,
) -> Result<String, String> {
    if let Some(value) = attached {
        if !value.is_empty() {
            return Ok(value.to_owned());
        }
    }
    *index += 1;
    args.get(*index)
        .map(|value| value.to_string_lossy().into_owned())
        .ok_or_else(|| format!("option requires an argument: {option}"))
}

fn parse_args(args: &[OsString]) -> Result<ParseResult, String> {
    let mut options = Options::default();
    let mut operands = Vec::new();
    let mut index = 1;
    let mut options_enabled = true;

    while index < args.len() {
        let lossy = args[index].to_string_lossy();
        if options_enabled && lossy == "--" {
            options_enabled = false;
        } else if options_enabled && lossy == "--help" {
            usage();
            return Ok(ParseResult::Exit(0));
        } else if options_enabled && lossy == "--version" {
            println!("cmp (AgentOS) {}", env!("CARGO_PKG_VERSION"));
            return Ok(ParseResult::Exit(0));
        } else if options_enabled && matches!(lossy.as_ref(), "-s" | "--quiet" | "--silent") {
            options.silent = true;
        } else if options_enabled && matches!(lossy.as_ref(), "-l" | "--verbose") {
            options.verbose = true;
        } else if options_enabled && matches!(lossy.as_ref(), "-b" | "--print-bytes") {
            options.print_bytes = true;
        } else if options_enabled && (lossy == "-bl" || lossy == "-lb") {
            options.print_bytes = true;
            options.verbose = true;
        } else if options_enabled && (lossy == "-n" || lossy.starts_with("--bytes=")) {
            let attached = lossy.strip_prefix("--bytes=");
            let value = required_option_value(args, &mut index, attached, "--bytes")?;
            options.limit = Some(parse_number(&value)?);
        } else if options_enabled && (lossy == "-i" || lossy.starts_with("--ignore-initial=")) {
            let attached = lossy.strip_prefix("--ignore-initial=");
            let value = required_option_value(args, &mut index, attached, "--ignore-initial")?;
            let mut values = value.splitn(2, ':');
            let first = parse_number(values.next().unwrap_or_default())?;
            options.ignore_initial = [first, first];
            if let Some(second) = values.next() {
                options.ignore_initial[1] = parse_number(second)?;
            }
        } else if options_enabled && lossy.starts_with('-') && lossy != "-" {
            return Err(format!("unrecognized option: {lossy}"));
        } else {
            operands.push(args[index].clone());
        }
        index += 1;
    }

    if !(1..=4).contains(&operands.len()) {
        return Err("expected FILE1 [FILE2 [SKIP1 [SKIP2]]]".to_owned());
    }

    let file1 = PathBuf::from(&operands[0]);
    let file2 = operands
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("-"));
    if file1.as_os_str() == "-" && file2.as_os_str() == "-" {
        return Err("standard input may only be specified once".to_owned());
    }

    let mut skips = options.ignore_initial;
    if let Some(value) = operands.get(2) {
        skips[0] = skips[0]
            .checked_add(parse_number(&value.to_string_lossy())?)
            .ok_or_else(|| "SKIP1 is too large".to_owned())?;
    }
    if let Some(value) = operands.get(3) {
        skips[1] = skips[1]
            .checked_add(parse_number(&value.to_string_lossy())?)
            .ok_or_else(|| "SKIP2 is too large".to_owned())?;
    }

    Ok(ParseResult::Run(Invocation {
        options,
        files: [file1, file2],
        skips,
    }))
}

fn open(path: &PathBuf) -> io::Result<Box<dyn Read>> {
    if path.as_os_str() == "-" {
        Ok(Box::new(io::stdin()))
    } else {
        File::open(path).map(|file| Box::new(file) as Box<dyn Read>)
    }
}

fn skip(reader: &mut dyn Read, count: u64) -> io::Result<()> {
    io::copy(&mut reader.take(count), &mut io::sink()).map(|_| ())
}

fn printable(byte: u8) -> String {
    match byte {
        b'\n' => "\\n".to_owned(),
        b'\r' => "\\r".to_owned(),
        b'\t' => "\\t".to_owned(),
        0x20..=0x7e => char::from(byte).to_string(),
        _ => format!("\\{:03o}", byte),
    }
}

fn compare(
    first: &mut dyn BufRead,
    second: &mut dyn BufRead,
    names: [&str; 2],
    options: &Options,
) -> io::Result<bool> {
    let mut byte_number = 1_u64;
    let mut line_number = 1_u64;
    let mut remaining = options.limit.unwrap_or(u64::MAX);
    let mut equal = true;

    while remaining > 0 {
        let first_buffer = first.fill_buf()?;
        let second_buffer = second.fill_buf()?;
        let available = first_buffer
            .len()
            .min(second_buffer.len())
            .min(remaining.min(usize::MAX as u64) as usize);

        if available == 0 {
            if first_buffer.is_empty() && second_buffer.is_empty() {
                break;
            }
            equal = false;
            if !options.silent {
                let eof_name = if first_buffer.is_empty() {
                    names[0]
                } else {
                    names[1]
                };
                eprintln!(
                    "cmp: EOF on {eof_name} after byte {}, line {line_number}",
                    byte_number - 1
                );
            }
            break;
        }

        for offset in 0..available {
            let left = first_buffer[offset];
            let right = second_buffer[offset];
            if left != right {
                equal = false;
                if options.silent {
                    return Ok(false);
                }
                if options.verbose {
                    if options.print_bytes {
                        println!(
                            "{byte_number:>6} {left:03o} {right:03o} {} {}",
                            printable(left),
                            printable(right)
                        );
                    } else {
                        println!("{byte_number:>6} {left:03o} {right:03o}");
                    }
                } else {
                    print!(
                        "{} {} differ: byte {byte_number}, line {line_number}",
                        names[0], names[1]
                    );
                    if options.print_bytes {
                        print!(
                            " is {left:03o} {} {right:03o} {}",
                            printable(left),
                            printable(right)
                        );
                    }
                    println!();
                    return Ok(false);
                }
            }
            if left == b'\n' {
                line_number += 1;
            }
            byte_number += 1;
            remaining -= 1;
        }

        first.consume(available);
        second.consume(available);
    }

    Ok(equal)
}

fn run(invocation: Invocation) -> Result<i32, String> {
    let mut first = open(&invocation.files[0])
        .map_err(|error| format!("{}: {error}", invocation.files[0].display()))?;
    let mut second = open(&invocation.files[1])
        .map_err(|error| format!("{}: {error}", invocation.files[1].display()))?;

    skip(&mut first, invocation.skips[0])
        .map_err(|error| format!("{}: {error}", invocation.files[0].display()))?;
    skip(&mut second, invocation.skips[1])
        .map_err(|error| format!("{}: {error}", invocation.files[1].display()))?;

    let names = [
        invocation.files[0].to_string_lossy(),
        invocation.files[1].to_string_lossy(),
    ];
    let mut first = BufReader::with_capacity(BUFFER_SIZE, first);
    let mut second = BufReader::with_capacity(BUFFER_SIZE, second);
    compare(
        &mut first,
        &mut second,
        [&names[0], &names[1]],
        &invocation.options,
    )
    .map(|equal| if equal { 0 } else { 1 })
    .map_err(|error| error.to_string())
}

fn main() {
    let args = std::env::args_os().collect::<Vec<_>>();
    let result = parse_args(&args).and_then(|parsed| match parsed {
        ParseResult::Run(invocation) => run(invocation),
        ParseResult::Exit(code) => Ok(code),
    });
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("cmp: {error}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn options() -> Options {
        Options::default()
    }

    #[test]
    fn compares_equal_and_different_inputs() {
        let mut equal_left = BufReader::new(Cursor::new(b"alpha\nbeta"));
        let mut equal_right = BufReader::new(Cursor::new(b"alpha\nbeta"));
        assert!(compare(&mut equal_left, &mut equal_right, ["a", "b"], &options()).unwrap());

        let mut different_left = BufReader::new(Cursor::new(b"alpha"));
        let mut different_right = BufReader::new(Cursor::new(b"alpHa"));
        assert!(!compare(
            &mut different_left,
            &mut different_right,
            ["a", "b"],
            &options()
        )
        .unwrap());
    }

    #[test]
    fn byte_limit_can_ignore_a_later_difference() {
        let mut left = BufReader::new(Cursor::new(b"abcdef"));
        let mut right = BufReader::new(Cursor::new(b"abcxef"));
        let options = Options {
            limit: Some(3),
            ..Options::default()
        };
        assert!(compare(&mut left, &mut right, ["a", "b"], &options).unwrap());
    }

    #[test]
    fn parses_limits_and_positional_skips() {
        let args = ["cmp", "-s", "-n", "3K", "a", "b", "4", "5"].map(OsString::from);
        let ParseResult::Run(invocation) = parse_args(&args).unwrap() else {
            panic!("expected invocation");
        };
        assert!(invocation.options.silent);
        assert_eq!(invocation.options.limit, Some(3 * 1024));
        assert_eq!(invocation.skips, [4, 5]);
    }

    #[test]
    fn parses_verbose_print_bytes_and_stdin_default() {
        let args = ["cmp", "-bl", "file"].map(OsString::from);
        let ParseResult::Run(invocation) = parse_args(&args).unwrap() else {
            panic!("expected invocation");
        };
        assert!(invocation.options.verbose);
        assert!(invocation.options.print_bytes);
        assert_eq!(invocation.files[1], PathBuf::from("-"));
    }

    #[test]
    fn skip_discards_the_requested_prefix() {
        let mut input = Cursor::new(b"prefixpayload");
        skip(&mut input, 6).unwrap();
        let mut remainder = Vec::new();
        input.read_to_end(&mut remainder).unwrap();
        assert_eq!(remainder, b"payload");
    }

    #[test]
    fn skip_past_eof_matches_gnu_cmp() {
        let mut input = Cursor::new(b"short");
        skip(&mut input, 100).unwrap();
        assert_eq!(input.position(), 5);
    }
}
