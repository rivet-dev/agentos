use std::env;
use std::fs;
use std::process::ExitCode;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Mount {
    source: String,
    target: String,
    fstype: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileSystemStats {
    total_bytes: u64,
    used_bytes: u64,
    available_bytes: u64,
    total_inodes: u64,
    free_inodes: u64,
}

#[cfg(target_os = "wasi")]
#[link(wasm_import_module = "host_fs")]
unsafe extern "C" {
    fn path_statfs(
        fd: u32,
        path_ptr: *const u8,
        path_len: u32,
        ret_total_bytes: *mut u64,
        ret_used_bytes: *mut u64,
        ret_available_bytes: *mut u64,
        ret_total_inodes: *mut u64,
        ret_free_inodes: *mut u64,
    ) -> u16;
}

#[derive(Debug, Default)]
struct Options {
    show_type: bool,
    inodes: bool,
    operands: Vec<String>,
}

fn main() -> ExitCode {
    let options = match parse_args(env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("df: {error}");
            return ExitCode::from(1);
        }
    };
    let mounts = match fs::read_to_string("/proc/mounts")
        .map_err(|error| format!("cannot read /proc/mounts: {error}"))
        .and_then(|contents| parse_mounts(&contents))
    {
        Ok(mounts) => mounts,
        Err(error) => {
            eprintln!("df: {error}");
            return ExitCode::from(1);
        }
    };
    let selected = select_mounts(&mounts, &options.operands);
    if selected.is_empty() {
        for operand in &options.operands {
            eprintln!("df: {operand}: no matching mount");
        }
        return ExitCode::from(1);
    }

    print_header(&options);
    for mount in selected {
        let stats = match filesystem_stats(&mount.target) {
            Ok(stats) => stats,
            Err(error) => {
                eprintln!("df: {}: {error}", mount.target);
                return ExitCode::from(1);
            }
        };
        println!("{}", format_mount(mount, &options, stats));
    }
    ExitCode::SUCCESS
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        if arg == "--" {
            options.operands.extend(args);
            break;
        }
        if arg == "--print-type" {
            options.show_type = true;
        } else if arg == "--inodes" {
            options.inodes = true;
        } else if arg == "--portability" || arg == "--human-readable" {
        } else if arg == "--block-size" {
            args.next()
                .ok_or_else(|| String::from("--block-size requires an argument"))?;
        } else if arg.starts_with("--block-size=") {
        } else if arg.starts_with('-') && arg != "-" {
            parse_short_options(&arg, &mut args, &mut options)?;
        } else {
            options.operands.push(arg);
        }
    }
    Ok(options)
}

fn parse_short_options(
    arg: &str,
    args: &mut impl Iterator<Item = String>,
    options: &mut Options,
) -> Result<(), String> {
    let mut chars = arg[1..].chars().peekable();
    while let Some(option) = chars.next() {
        match option {
            'T' => options.show_type = true,
            'i' => options.inodes = true,
            'P' | 'k' | 'h' => {}
            'B' => {
                if chars.peek().is_none() {
                    args.next()
                        .ok_or_else(|| String::from("-B requires an argument"))?;
                }
                break;
            }
            unknown => return Err(format!("unknown option -- '{unknown}'")),
        }
    }
    Ok(())
}

fn parse_mounts(contents: &str) -> Result<Vec<Mount>, String> {
    contents
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 4 {
                return Err(format!("malformed /proc/mounts line {}", index + 1));
            }
            Ok(Mount {
                source: unescape_mount_field(fields[0])?,
                target: unescape_mount_field(fields[1])?,
                fstype: unescape_mount_field(fields[2])?,
            })
        })
        .collect()
}

fn unescape_mount_field(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            let octal = &value[index + 1..index + 4];
            if octal.bytes().all(|byte| matches!(byte, b'0'..=b'7')) {
                output.push(
                    u8::from_str_radix(octal, 8)
                        .map_err(|_| format!("invalid mount escape: \\{octal}"))?,
                );
                index += 4;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(output).map_err(|_| String::from("mount field is not UTF-8"))
}

fn select_mounts<'a>(mounts: &'a [Mount], operands: &[String]) -> Vec<&'a Mount> {
    if operands.is_empty() {
        return mounts.iter().collect();
    }
    let mut selected = Vec::new();
    for operand in operands {
        let normalized = normalize_path(operand);
        let matching = mounts
            .iter()
            .filter(|mount| mount.source == *operand || path_is_within(&normalized, &mount.target))
            .max_by_key(|mount| mount.target.len());
        if let Some(mount) = matching {
            if !selected.contains(&mount) {
                selected.push(mount);
            }
        }
    }
    selected
}

fn normalize_path(path: &str) -> String {
    if path == "/" {
        String::from("/")
    } else {
        path.trim_end_matches('/').to_owned()
    }
}

fn path_is_within(path: &str, mountpoint: &str) -> bool {
    mountpoint == "/"
        || path == mountpoint
        || path
            .strip_prefix(mountpoint)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn print_header(options: &Options) {
    if options.inodes {
        println!("Filesystem Inodes IUsed IFree IUse% Mounted on");
    } else if options.show_type {
        println!("Filesystem Type 1024-blocks Used Available Capacity Mounted on");
    } else {
        println!("Filesystem 1024-blocks Used Available Capacity Mounted on");
    }
}

fn format_mount(mount: &Mount, options: &Options, stats: FileSystemStats) -> String {
    if options.inodes {
        let used = stats.total_inodes.saturating_sub(stats.free_inodes);
        format!(
            "{} {} {} {} {}% {}",
            mount.source,
            stats.total_inodes,
            used,
            stats.free_inodes,
            percentage(used, stats.total_inodes),
            mount.target
        )
    } else if options.show_type {
        format!(
            "{} {} {} {} {} {}% {}",
            mount.source,
            mount.fstype,
            bytes_to_kib(stats.total_bytes),
            bytes_to_kib(stats.used_bytes),
            bytes_to_kib(stats.available_bytes),
            percentage(stats.used_bytes, stats.total_bytes),
            mount.target
        )
    } else {
        format!(
            "{} {} {} {} {}% {}",
            mount.source,
            bytes_to_kib(stats.total_bytes),
            bytes_to_kib(stats.used_bytes),
            bytes_to_kib(stats.available_bytes),
            percentage(stats.used_bytes, stats.total_bytes),
            mount.target
        )
    }
}

fn bytes_to_kib(bytes: u64) -> u64 {
    bytes.div_ceil(1024)
}

fn percentage(used: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    ((u128::from(used) * 100).div_ceil(u128::from(total))).min(100) as u64
}

#[cfg(target_os = "wasi")]
fn filesystem_stats(path: &str) -> Result<FileSystemStats, String> {
    let path_len = u32::try_from(path.len()).map_err(|_| String::from("path is too long"))?;
    let mut stats = FileSystemStats {
        total_bytes: 0,
        used_bytes: 0,
        available_bytes: 0,
        total_inodes: 0,
        free_inodes: 0,
    };
    let errno = unsafe {
        path_statfs(
            u32::MAX,
            path.as_ptr(),
            path_len,
            &mut stats.total_bytes,
            &mut stats.used_bytes,
            &mut stats.available_bytes,
            &mut stats.total_inodes,
            &mut stats.free_inodes,
        )
    };
    if errno == 0 {
        Ok(stats)
    } else {
        Err(format!("statfs failed with errno {errno}"))
    }
}

#[cfg(not(target_os = "wasi"))]
fn filesystem_stats(_path: &str) -> Result<FileSystemStats, String> {
    Err(String::from("filesystem statistics require the AgentOS WASI host"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mounts() -> Vec<Mount> {
        parse_mounts(
            "root / root rw 0 0\n/dev/agentos-test /mnt/test agentos rw 0 0\n/dev/agentos-scratch /mnt/scratch agentos rw 0 0\n",
        )
        .expect("valid fixture")
    }

    #[test]
    fn parses_xfstests_type_and_portability_options() {
        let options = parse_args(
            ["-T", "-P", "/dev/agentos-test"]
                .into_iter()
                .map(str::to_owned),
        )
        .expect("valid options");
        assert!(options.show_type);
        let mounts = mounts();
        let selected = select_mounts(&mounts, &options.operands);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].fstype, "agentos");
    }

    #[test]
    fn path_operand_uses_longest_containing_mount() {
        let mounts = mounts();
        let selected = select_mounts(&mounts, &[String::from("/mnt/scratch/file")]);
        assert_eq!(selected[0].source, "/dev/agentos-scratch");
    }

    #[test]
    fn formats_authoritative_capacity_and_inode_rows() {
        let mounts = mounts();
        let mount = &mounts[1];
        let stats = FileSystemStats {
            total_bytes: 8 * 1024,
            used_bytes: 2 * 1024,
            available_bytes: 6 * 1024,
            total_inodes: 100,
            free_inodes: 75,
        };
        assert_eq!(
            format_mount(
                mount,
                &Options {
                    show_type: true,
                    ..Options::default()
                },
                stats,
            ),
            "/dev/agentos-test agentos 8 2 6 25% /mnt/test"
        );
        assert_eq!(
            format_mount(
                mount,
                &Options {
                    inodes: true,
                    ..Options::default()
                },
                stats,
            ),
            "/dev/agentos-test 100 25 75 25% /mnt/test"
        );
    }
}
