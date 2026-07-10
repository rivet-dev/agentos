//! rev -- reverse characters in each line (UTF-8 aware)

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, ErrorKind, Write};

const MAX_INPUT_LINE_BYTES: usize = 16 * 1024 * 1024;

pub fn main(args: Vec<OsString>) -> i32 {
    let filenames: Vec<String> = args
        .iter()
        .skip(1)
        .map(|a| a.to_string_lossy().to_string())
        .collect();

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if filenames.is_empty() {
        // Read from stdin
        if let Err(e) = process_reader(io::stdin().lock(), &mut out) {
            eprintln!("rev: {}", e);
            return 1;
        }
    } else {
        for filename in &filenames {
            match File::open(filename) {
                Ok(f) => {
                    if let Err(e) = process_reader(BufReader::new(f), &mut out) {
                        eprintln!("rev: {}: {}", filename, e);
                        return 1;
                    }
                }
                Err(e) => {
                    eprintln!("rev: {}: {}", filename, e);
                    return 1;
                }
            }
        }
    }

    0
}

fn process_reader<R: BufRead, W: Write>(mut reader: R, out: &mut W) -> io::Result<()> {
    let mut line = Vec::new();

    while read_line_limited(&mut reader, &mut line)? != 0 {
        trim_line_ending(&mut line);
        let line =
            std::str::from_utf8(&line).map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?;
        let reversed: String = line.chars().rev().collect();
        writeln!(out, "{}", reversed)?;
    }
    Ok(())
}

fn read_line_limited<R: BufRead>(reader: &mut R, line: &mut Vec<u8>) -> io::Result<usize> {
    line.clear();
    let mut bytes_read = 0;

    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(bytes_read);
        }

        let newline = available.iter().position(|&b| b == b'\n');
        let chunk_len = newline.map_or(available.len(), |pos| pos + 1);
        let content_len = line.len() + chunk_len - usize::from(newline.is_some());
        if content_len > MAX_INPUT_LINE_BYTES {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "input line exceeds size limit",
            ));
        }

        line.extend_from_slice(&available[..chunk_len]);
        reader.consume(chunk_len);
        bytes_read += chunk_len;

        if newline.is_some() {
            return Ok(bytes_read);
        }
    }
}

fn trim_line_ending(line: &mut Vec<u8>) {
    if line.ends_with(b"\n") {
        line.pop();
        if line.ends_with(b"\r") {
            line.pop();
        }
    }
}
