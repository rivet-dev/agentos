use std::env;
use std::fs;
use std::path::Path;

const RECORD_BYTES: usize = 4096;

#[derive(Debug, Eq, PartialEq)]
struct Options {
    recursive: bool,
    follow_symlinks: bool,
    uid: Option<u32>,
    gid: Option<u32>,
    paths: Vec<String>,
}

fn lookup_id(record: Result<u32, u32>, buffer: &[u8], field: usize) -> Result<u32, String> {
    let len = record.map_err(|errno| format!("account lookup failed: errno {errno}"))? as usize;
    let text = std::str::from_utf8(&buffer[..len])
        .map_err(|_| String::from("account database returned invalid UTF-8"))?;
    text.split(':')
        .nth(field)
        .ok_or_else(|| String::from("account database returned a malformed record"))?
        .parse()
        .map_err(|_| String::from("account database returned an invalid ID"))
}

fn user_id(name: &str) -> Result<u32, String> {
    if let Ok(uid) = name.parse() {
        return Ok(uid);
    }
    let mut buffer = vec![0; RECORD_BYTES];
    lookup_id(wasi_ext::get_pwnam(name, &mut buffer), &buffer, 2)
}

fn group_id(name: &str) -> Result<u32, String> {
    if let Ok(gid) = name.parse() {
        return Ok(gid);
    }
    let mut buffer = vec![0; RECORD_BYTES];
    lookup_id(wasi_ext::get_grnam(name, &mut buffer), &buffer, 2)
}

fn parse_owner(owner: &str, group_only: bool) -> Result<(Option<u32>, Option<u32>), String> {
    if group_only {
        return Ok((None, Some(group_id(owner)?)));
    }
    let (user, group) = owner
        .split_once(':')
        .or_else(|| owner.split_once('.'))
        .unwrap_or((owner, ""));
    let uid = (!user.is_empty()).then(|| user_id(user)).transpose()?;
    let gid = (!group.is_empty()).then(|| group_id(group)).transpose()?;
    if uid.is_none() && gid.is_none() {
        return Err(String::from("empty owner specification"));
    }
    Ok((uid, gid))
}

fn parse_args(args: impl IntoIterator<Item = String>, group_only: bool) -> Result<Options, String> {
    let mut recursive = false;
    let mut follow_symlinks = true;
    let mut operands = Vec::new();
    let mut options_done = false;
    for arg in args {
        if !options_done {
            match arg.as_str() {
                "--" => {
                    options_done = true;
                    continue;
                }
                "-R" | "--recursive" => {
                    recursive = true;
                    continue;
                }
                "-h" | "--no-dereference" | "-P" => {
                    follow_symlinks = false;
                    continue;
                }
                "-H" | "-L" | "--dereference" => {
                    follow_symlinks = true;
                    continue;
                }
                "-f" | "--silent" | "--quiet" => continue,
                _ if arg.starts_with('-') => return Err(format!("unsupported option {arg}")),
                _ => options_done = true,
            }
        }
        operands.push(arg);
    }
    if operands.len() < 2 {
        return Err(String::from("missing owner or file operand"));
    }
    let (uid, gid) = parse_owner(&operands.remove(0), group_only)?;
    Ok(Options {
        recursive,
        follow_symlinks,
        uid,
        gid,
        paths: operands,
    })
}

fn change_one(path: &Path, options: &Options) -> Result<(), String> {
    let path = path
        .to_str()
        .ok_or_else(|| String::from("path is not valid UTF-8"))?;
    let (current_uid, current_gid) = wasi_ext::path_ids(path, options.follow_symlinks)
        .map_err(|errno| format!("{path}: stat failed with errno {errno}"))?;
    wasi_ext::chown_path(
        path,
        options.uid.unwrap_or(current_uid),
        options.gid.unwrap_or(current_gid),
        options.follow_symlinks,
    )
    .map_err(|errno| format!("{path}: chown failed with errno {errno}"))
}

fn change_tree(path: &Path, options: &Options) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if options.recursive && metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|error| format!("{}: {error}", path.display()))? {
            let entry = entry.map_err(|error| format!("{}: {error}", path.display()))?;
            change_tree(&entry.path(), options)?;
        }
    }
    change_one(path, options)
}

fn run(group_only: bool) -> Result<(), String> {
    let options = parse_args(env::args().skip(1), group_only)?;
    for path in &options.paths {
        change_tree(Path::new(path), &options)?;
    }
    Ok(())
}

fn main() {
    let group_only = env::args()
        .next()
        .as_deref()
        .map(Path::new)
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        == Some("chgrp");
    if let Err(error) = run(group_only) {
        eprintln!("{}: {error}", if group_only { "chgrp" } else { "chown" });
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_recursive_numeric_owner_and_group() {
        assert_eq!(
            parse_args(
                ["-R", "1000:1001", "a", "b"].into_iter().map(String::from),
                false,
            )
            .unwrap(),
            Options {
                recursive: true,
                follow_symlinks: true,
                uid: Some(1000),
                gid: Some(1001),
                paths: vec![String::from("a"), String::from("b")],
            }
        );
    }

    #[test]
    fn parses_group_only_mode() {
        assert_eq!(
            parse_args(["-h", "7", "file"].into_iter().map(String::from), true).unwrap(),
            Options {
                recursive: false,
                follow_symlinks: false,
                uid: None,
                gid: Some(7),
                paths: vec![String::from("file")],
            }
        );
    }
}
