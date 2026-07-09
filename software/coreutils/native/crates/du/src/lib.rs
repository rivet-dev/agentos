//! du -- estimate file space usage
//!
//! Recursive directory walk summing file sizes via std::fs::metadata.
//! Supports -s (summary), -h (human-readable), -a (all files), -c (grand total),
//! -d N (max depth).

#![cfg_attr(target_os = "wasi", feature(wasi_ext))]

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(target_os = "wasi")]
use std::os::wasi::fs::MetadataExt;
use std::path::{Path, PathBuf};

pub fn main(args: Vec<OsString>) -> i32 {
    let str_args: Vec<String> = args
        .iter()
        .skip(1)
        .map(|a| a.to_string_lossy().to_string())
        .collect();

    let mut summary = false;
    let mut human = false;
    let mut all_files = false;
    let mut grand_total = false;
    let mut max_depth: Option<usize> = None;
    let mut paths: Vec<String> = Vec::new();

    let mut i = 0;
    while i < str_args.len() {
        match str_args[i].as_str() {
            "-s" | "--summarize" => summary = true,
            "-h" | "--human-readable" => human = true,
            "-a" | "--all" => all_files = true,
            "-c" | "--total" => grand_total = true,
            "-d" | "--max-depth" => {
                i += 1;
                if i >= str_args.len() {
                    eprintln!("du: option '-d' requires an argument");
                    return 1;
                }
                match str_args[i].parse::<usize>() {
                    Ok(d) => max_depth = Some(d),
                    Err(_) => {
                        eprintln!("du: invalid maximum depth '{}'", str_args[i]);
                        return 1;
                    }
                }
            }
            s if s.starts_with("-d") && s.len() > 2 => match s[2..].parse::<usize>() {
                Ok(d) => max_depth = Some(d),
                Err(_) => {
                    eprintln!("du: invalid maximum depth '{}'", &s[2..]);
                    return 1;
                }
            },
            // Combined short flags like -sh, -shc
            s if s.starts_with('-') && s.len() > 1 && !s.starts_with("--") => {
                for ch in s[1..].chars() {
                    match ch {
                        'k' => {}
                        's' => summary = true,
                        'h' => human = true,
                        'a' => all_files = true,
                        'c' => grand_total = true,
                        _ => {
                            eprintln!("du: unknown option '{}'", s);
                            return 1;
                        }
                    }
                }
            }
            _ => paths.push(str_args[i].clone()),
        }
        i += 1;
    }

    if paths.is_empty() {
        paths.push(".".to_string());
    }

    // -s implies max_depth=0
    if summary {
        max_depth = Some(0);
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut total: u64 = 0;
    let mut exit_code = 0;

    for path in &paths {
        let mut visited_dirs = HashSet::new();
        match walk_du(
            Path::new(path),
            0,
            max_depth,
            all_files,
            human,
            &mut out,
            &mut visited_dirs,
        ) {
            Ok(size) => {
                total += size;
            }
            Err(e) => {
                eprintln!("du: {}: {}", path, e);
                exit_code = 1;
            }
        }
    }

    if grand_total {
        if let Err(error) = print_size(&mut out, total, human, "total") {
            eprintln!("du: failed to write output: {error}");
            return 1;
        }
    }

    if let Err(error) = out.flush() {
        eprintln!("du: failed to write output: {error}");
        return 1;
    }

    exit_code
}

fn walk_du<W: Write>(
    path: &Path,
    depth: usize,
    max_depth: Option<usize>,
    all_files: bool,
    human: bool,
    out: &mut W,
    visited_dirs: &mut HashSet<PathBuf>,
) -> io::Result<u64> {
    let meta = fs::metadata(path)?;

    if meta.is_file() {
        let blocks = allocated_kib(&meta);
        if all_files || depth == 0 {
            print_size(out, blocks, human, &path.to_string_lossy())?;
        }
        return Ok(blocks);
    }

    if meta.is_dir() {
        let canonical_path = fs::canonicalize(path)?;
        if !visited_dirs.insert(canonical_path) {
            return Ok(0);
        }

        let mut dir_total: u64 = 0;
        let entries = fs::read_dir(path)?;
        for entry in entries {
            let entry = entry?;
            let child_path = entry.path();
            let child_meta = fs::metadata(&child_path)?;

            if child_meta.is_dir() {
                let sub = walk_du(
                    &child_path,
                    depth + 1,
                    max_depth,
                    all_files,
                    human,
                    out,
                    visited_dirs,
                )?;
                dir_total += sub;
            } else {
                let blocks = allocated_kib(&child_meta);
                dir_total += blocks;
                if all_files {
                    if let Some(md) = max_depth {
                        if depth + 1 <= md {
                            print_size(out, blocks, human, &child_path.to_string_lossy())?;
                        }
                    } else {
                        print_size(out, blocks, human, &child_path.to_string_lossy())?;
                    }
                }
            }
        }

        // Print this directory's total if within depth limit
        if let Some(md) = max_depth {
            if depth <= md {
                print_size(out, dir_total, human, &path.to_string_lossy())?;
            }
        } else {
            print_size(out, dir_total, human, &path.to_string_lossy())?;
        }

        return Ok(dir_total);
    }

    // Symlinks, special files -- just report their metadata size
    let blocks = allocated_kib(&meta);
    if all_files || depth == 0 {
        print_size(out, blocks, human, &path.to_string_lossy())?;
    }
    Ok(blocks)
}

fn allocated_kib(metadata: &fs::Metadata) -> u64 {
    metadata.blocks().div_ceil(2)
}

fn print_size<W: Write>(out: &mut W, blocks: u64, human: bool, name: &str) -> io::Result<()> {
    if human {
        writeln!(out, "{}\t{}", format_human(blocks), name)
    } else {
        writeln!(out, "{}\t{}", blocks, name)
    }
}

fn format_human(blocks: u64) -> String {
    let kb = blocks as f64;
    if kb < 1024.0 {
        format!("{}K", blocks)
    } else if kb < 1024.0 * 1024.0 {
        let mb = kb / 1024.0;
        if mb >= 10.0 {
            format!("{:.0}M", mb)
        } else {
            format!("{:.1}M", mb)
        }
    } else {
        let gb = kb / (1024.0 * 1024.0);
        if gb >= 10.0 {
            format!("{:.0}G", gb)
        } else {
            format!("{:.1}G", gb)
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::allocated_kib;
    use std::fs::{self, OpenOptions};
    use std::io::{Seek, SeekFrom, Write};

    #[test]
    fn allocated_size_uses_sparse_block_count() {
        let path = std::env::temp_dir().join(format!(
            "agentos-du-sparse-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .expect("create sparse test file");
        file.seek(SeekFrom::Start(1_600 * 1024))
            .expect("seek sparse test file");
        file.write_all(&vec![b'x'; 50 * 1024])
            .expect("write sparse extent");
        file.sync_all().expect("sync sparse test file");
        let metadata = file.metadata().expect("stat sparse test file");
        assert!(allocated_kib(&metadata) < metadata.len().div_ceil(1024));
        drop(file);
        fs::remove_file(path).expect("remove sparse test file");
    }
}
