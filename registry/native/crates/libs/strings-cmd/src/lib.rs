//! strings -- find printable ASCII strings in binary data

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};

const READ_BUFFER_BYTES: usize = 8 * 1024;
const MAX_MIN_LENGTH: usize = 1024 * 1024;

pub fn main(args: Vec<OsString>) -> i32 {
    let str_args: Vec<String> = args
        .iter()
        .skip(1)
        .map(|a| a.to_string_lossy().to_string())
        .collect();

    let mut min_len: usize = 4;
    let mut offset_format: Option<char> = None; // 'd', 'o', or 'x'
    let mut filenames: Vec<String> = Vec::new();

    let mut i = 0;
    while i < str_args.len() {
        match str_args[i].as_str() {
            "-n" => {
                i += 1;
                if i >= str_args.len() {
                    eprintln!("strings: option '-n' requires an argument");
                    return 1;
                }
                match str_args[i].parse::<usize>() {
                    Ok(n) if (1..=MAX_MIN_LENGTH).contains(&n) => min_len = n,
                    _ => {
                        eprintln!("strings: invalid minimum string length '{}'", str_args[i]);
                        return 1;
                    }
                }
            }
            s if s.starts_with("-n") => {
                let val = &s[2..];
                match val.parse::<usize>() {
                    Ok(n) if (1..=MAX_MIN_LENGTH).contains(&n) => min_len = n,
                    _ => {
                        eprintln!("strings: invalid minimum string length '{}'", val);
                        return 1;
                    }
                }
            }
            "-t" => {
                i += 1;
                if i >= str_args.len() {
                    eprintln!("strings: option '-t' requires an argument");
                    return 1;
                }
                match str_args[i].as_str() {
                    "d" | "o" | "x" => offset_format = Some(str_args[i].chars().next().unwrap()),
                    _ => {
                        eprintln!("strings: invalid radix for -t: '{}'", str_args[i]);
                        return 1;
                    }
                }
            }
            s if s.starts_with('-') && s.len() > 1 => {
                // Try parsing as -N (numeric min length, GNU extension)
                if let Ok(n) = s[1..].parse::<usize>() {
                    if (1..=MAX_MIN_LENGTH).contains(&n) {
                        min_len = n;
                    } else {
                        eprintln!("strings: invalid minimum string length '{}'", &s[1..]);
                        return 1;
                    }
                } else {
                    eprintln!("strings: unknown option '{}'", s);
                    return 1;
                }
            }
            _ => filenames.push(str_args[i].clone()),
        }
        i += 1;
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if filenames.is_empty() {
        if let Err(e) = extract_strings(io::stdin().lock(), min_len, offset_format, &mut out) {
            eprintln!("strings: stdin: {}", e);
            return 1;
        }
    } else {
        for filename in &filenames {
            match File::open(filename)
                .and_then(|f| extract_strings(f, min_len, offset_format, &mut out))
            {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("strings: {}: {}", filename, e);
                    return 1;
                }
            };
        }
    }

    match out.flush() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("strings: stdout: {}", e);
            1
        }
    }
}

fn extract_strings<R: Read, W: Write>(
    mut reader: R,
    min_len: usize,
    offset_fmt: Option<char>,
    out: &mut W,
) -> io::Result<()> {
    let mut run_start: Option<usize> = None;
    let mut run = Vec::new();
    let mut emitted = false;
    let mut offset = 0;
    let mut buffer = [0; READ_BUFFER_BYTES];

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }

        for &b in &buffer[..bytes_read] {
            if is_printable_ascii(b) {
                if run_start.is_none() {
                    run_start = Some(offset);
                }
                if emitted {
                    out.write_all(&[b])?;
                } else {
                    run.push(b);
                    if run.len() == min_len {
                        emit_prefix(out, run_start.unwrap_or(0), offset_fmt)?;
                        out.write_all(&run)?;
                        run.clear();
                        emitted = true;
                    }
                }
            } else {
                if emitted {
                    writeln!(out)?;
                }
                run.clear();
                run_start = None;
                emitted = false;
            }
            offset += 1;
        }
    }

    if emitted {
        writeln!(out)?;
    }
    Ok(())
}

fn emit_prefix<W: Write>(out: &mut W, offset: usize, offset_fmt: Option<char>) -> io::Result<()> {
    if let Some(fmt) = offset_fmt {
        match fmt {
            'd' => {
                write!(out, "{:7} ", offset)?;
            }
            'o' => {
                write!(out, "{:7o} ", offset)?;
            }
            'x' => {
                write!(out, "{:7x} ", offset)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn is_printable_ascii(b: u8) -> bool {
    // Printable ASCII: space (0x20) through tilde (0x7E), plus tab (0x09)
    b == b'\t' || (b >= 0x20 && b <= 0x7E)
}
