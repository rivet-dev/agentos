use std::env;
use std::fs;
use std::process::ExitCode;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Mount {
    source: String,
    target: String,
    fstype: String,
    options: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Column {
    Source,
    Target,
    Fstype,
    Options,
}

#[derive(Debug)]
struct Query {
    source: Option<String>,
    mountpoint: Option<String>,
    target_path: Option<String>,
    positional: Option<String>,
    fstypes: Vec<String>,
    columns: Vec<Column>,
    no_headings: bool,
    first_only: bool,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            source: None,
            mountpoint: None,
            target_path: None,
            positional: None,
            fstypes: Vec::new(),
            columns: vec![
                Column::Source,
                Column::Target,
                Column::Fstype,
                Column::Options,
            ],
            no_headings: false,
            first_only: false,
        }
    }
}

fn main() -> ExitCode {
    let query = match parse_args(env::args().skip(1)) {
        Ok(query) => query,
        Err(error) => {
            eprintln!("findmnt: {error}");
            return ExitCode::from(2);
        }
    };
    let contents = match fs::read_to_string("/proc/mounts") {
        Ok(contents) => contents,
        Err(error) => {
            eprintln!("findmnt: cannot read /proc/mounts: {error}");
            return ExitCode::from(2);
        }
    };
    let mounts = match parse_mounts(&contents) {
        Ok(mounts) => mounts,
        Err(error) => {
            eprintln!("findmnt: {error}");
            return ExitCode::from(2);
        }
    };
    let matches = select_mounts(&mounts, &query);
    if matches.is_empty() {
        return ExitCode::from(1);
    }

    if !query.no_headings {
        println!(
            "{}",
            query
                .columns
                .iter()
                .map(|column| column.heading())
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    for mount in matches {
        println!("{}", render_mount(mount, &query.columns));
    }
    ExitCode::SUCCESS
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Query, String> {
    let mut query = Query::default();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        if arg == "--" {
            for value in args {
                set_positional(&mut query, value)?;
            }
            break;
        }
        if let Some(value) = arg.strip_prefix("--target=") {
            query.target_path = Some(value.to_owned());
            continue;
        }
        if let Some(value) = arg.strip_prefix("--output=") {
            query.columns = parse_columns(value)?;
            continue;
        }
        match arg.as_str() {
            "--target" => query.target_path = Some(next_value(&mut args, "--target")?),
            "--source" => query.source = Some(next_value(&mut args, "--source")?),
            "--mountpoint" => query.mountpoint = Some(next_value(&mut args, "--mountpoint")?),
            "--types" => query.fstypes = parse_types(&next_value(&mut args, "--types")?),
            "--output" => query.columns = parse_columns(&next_value(&mut args, "--output")?)?,
            "--noheadings" => query.no_headings = true,
            "--first-only" => query.first_only = true,
            "--canonicalize" | "--raw" | "--nofsroot" => {}
            "--direction" => {
                let _ = next_value(&mut args, "--direction")?;
            }
            "--help" => return Err(String::from("usage: findmnt [options] [device|mountpoint]")),
            _ if arg.starts_with('-') => parse_short_options(&arg, &mut args, &mut query)?,
            _ => set_positional(&mut query, arg)?,
        }
    }
    Ok(query)
}

fn parse_short_options(
    arg: &str,
    args: &mut std::iter::Peekable<impl Iterator<Item = String>>,
    query: &mut Query,
) -> Result<(), String> {
    let mut chars = arg[1..].chars().peekable();
    while let Some(option) = chars.next() {
        match option {
            'n' => query.no_headings = true,
            'f' => query.first_only = true,
            'r' | 'c' | 'v' => {}
            'S' | 'M' | 'T' | 't' | 'o' | 'd' => {
                let inline = chars.collect::<String>();
                let value = if inline.is_empty() {
                    next_value(args, &format!("-{option}"))?
                } else {
                    inline
                };
                match option {
                    'S' => query.source = Some(value),
                    'M' => query.mountpoint = Some(value),
                    'T' => query.target_path = Some(value),
                    't' => query.fstypes = parse_types(&value),
                    'o' => query.columns = parse_columns(&value)?,
                    'd' => {}
                    _ => unreachable!(),
                }
                break;
            }
            unknown => return Err(format!("unknown option -- '{unknown}'")),
        }
    }
    Ok(())
}

fn next_value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("option {option} requires an argument"))
}

fn set_positional(query: &mut Query, value: String) -> Result<(), String> {
    if query.positional.replace(value).is_some() {
        return Err(String::from(
            "only one device or mountpoint may be specified",
        ));
    }
    Ok(())
}

fn parse_columns(value: &str) -> Result<Vec<Column>, String> {
    let columns = value
        .split(',')
        .map(|value| match value.to_ascii_uppercase().as_str() {
            "SOURCE" => Ok(Column::Source),
            "TARGET" => Ok(Column::Target),
            "FSTYPE" => Ok(Column::Fstype),
            "OPTIONS" => Ok(Column::Options),
            unknown => Err(format!("unknown column: {unknown}")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if columns.is_empty() {
        return Err(String::from("output column list cannot be empty"));
    }
    Ok(columns)
}

fn parse_types(value: &str) -> Vec<String> {
    value.split(',').map(str::to_owned).collect()
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
                options: unescape_mount_field(fields[3])?,
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

fn select_mounts<'a>(mounts: &'a [Mount], query: &Query) -> Vec<&'a Mount> {
    let mut selected = mounts
        .iter()
        .filter(|mount| {
            query
                .source
                .as_ref()
                .is_none_or(|source| mount.source == *source)
        })
        .filter(|mount| {
            query
                .mountpoint
                .as_ref()
                .is_none_or(|target| mount.target == *target)
        })
        .filter(|mount| {
            query.fstypes.is_empty() || query.fstypes.iter().any(|fstype| mount.fstype == *fstype)
        })
        .filter(|mount| {
            query
                .positional
                .as_ref()
                .is_none_or(|value| mount.source == *value || mount.target == normalize_path(value))
        })
        .collect::<Vec<_>>();

    if let Some(path) = &query.target_path {
        let path = normalize_path(path);
        selected.retain(|mount| path_is_within(&path, &mount.target));
        selected.sort_by_key(|mount| std::cmp::Reverse(mount.target.len()));
        selected.truncate(1);
    }
    if query.first_only {
        selected.truncate(1);
    }
    selected
}

fn normalize_path(path: &str) -> String {
    if path == "/" {
        return String::from("/");
    }
    path.trim_end_matches('/').to_owned()
}

fn path_is_within(path: &str, mountpoint: &str) -> bool {
    mountpoint == "/"
        || path == mountpoint
        || path
            .strip_prefix(mountpoint)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn render_mount(mount: &Mount, columns: &[Column]) -> String {
    columns
        .iter()
        .map(|column| column.value(mount))
        .collect::<Vec<_>>()
        .join(" ")
}

impl Column {
    fn heading(self) -> &'static str {
        match self {
            Self::Source => "SOURCE",
            Self::Target => "TARGET",
            Self::Fstype => "FSTYPE",
            Self::Options => "OPTIONS",
        }
    }

    fn value(self, mount: &Mount) -> &str {
        match self {
            Self::Source => &mount.source,
            Self::Target => &mount.target,
            Self::Fstype => &mount.fstype,
            Self::Options => &mount.options,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mounts() -> Vec<Mount> {
        parse_mounts(
            "root / root rw 0 0\n/dev/agentos-test /mnt/test agentos rw 0 0\n/dev/agentos-scratch /mnt/scratch agentos rw,noattr2 0 0\n",
        )
        .expect("valid fixtures")
    }

    #[test]
    fn combined_xfstests_options_select_source_and_columns() {
        let query = parse_args(
            ["-rncv", "-S", "/dev/agentos-test", "-o", "SOURCE,TARGET"]
                .into_iter()
                .map(str::to_owned),
        )
        .expect("valid query");
        let mounts = mounts();
        let selected = select_mounts(&mounts, &query);
        assert_eq!(selected.len(), 1);
        assert_eq!(
            render_mount(selected[0], &query.columns),
            "/dev/agentos-test /mnt/test"
        );
    }

    #[test]
    fn target_query_selects_longest_containing_mount() {
        let query = parse_args(
            ["-n", "-T", "/mnt/scratch/subdir", "-o", "FSTYPE"]
                .into_iter()
                .map(str::to_owned),
        )
        .expect("valid query");
        let mounts = mounts();
        let selected = select_mounts(&mounts, &query);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].target, "/mnt/scratch");
    }

    #[test]
    fn mountpoint_and_type_filters_match_xfstests_queries() {
        let query = parse_args(
            [
                "-rncv",
                "-M",
                "/mnt/scratch",
                "-t",
                "agentos",
                "-o",
                "OPTIONS",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .expect("valid query");
        let mounts = mounts();
        let selected = select_mounts(&mounts, &query);
        assert_eq!(render_mount(selected[0], &query.columns), "rw,noattr2");
    }

    #[test]
    fn parses_proc_mount_escapes() {
        let parsed = parse_mounts("dev\\040name /mnt/test agentos rw 0 0\n").expect("valid mount");
        assert_eq!(parsed[0].source, "dev name");
    }
}
