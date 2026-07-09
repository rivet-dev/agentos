use std::env;
use std::process::Command;

#[derive(Debug, Default, Eq, PartialEq)]
struct Options {
    uid: Option<u32>,
    gid: Option<u32>,
    supplementary_gids: Vec<u32>,
    command: Vec<String>,
}

fn numeric_id(flag: &str, value: Option<String>) -> Result<u32, String> {
    value
        .ok_or_else(|| format!("{flag} requires an ID"))?
        .parse()
        .map_err(|_| format!("{flag} requires a numeric ID"))
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut args = args.into_iter();
    let mut options = Options::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-u" => options.uid = Some(numeric_id("-u", args.next())?),
            "-g" => options.gid = Some(numeric_id("-g", args.next())?),
            "-s" => options
                .supplementary_gids
                .push(numeric_id("-s", args.next())?),
            "--" => {
                options.command.extend(args);
                break;
            }
            _ if arg.starts_with('-') => return Err(format!("unsupported option {arg}")),
            _ => {
                options.command.push(arg);
                options.command.extend(args);
                break;
            }
        }
    }
    if options.command.is_empty() {
        return Err(String::from("missing command"));
    }
    Ok(options)
}

fn run() -> Result<i32, String> {
    let options = parse_args(env::args().skip(1))?;
    if let Some(gid) = options.gid {
        wasi_ext::set_gid(gid).map_err(|errno| format!("setgid({gid}) failed: errno {errno}"))?;
    }
    if options.gid.is_some() || !options.supplementary_gids.is_empty() {
        wasi_ext::set_groups(&options.supplementary_gids)
            .map_err(|errno| format!("setgroups failed: errno {errno}"))?;
    }
    if let Some(uid) = options.uid {
        wasi_ext::set_uid(uid).map_err(|errno| format!("setuid({uid}) failed: errno {errno}"))?;
    }
    Command::new(&options.command[0])
        .args(&options.command[1..])
        .status()
        .map(|status| status.code().unwrap_or(1))
        .map_err(|error| format!("{}: {error}", options.command[0]))
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("runas: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xfstests_numeric_options() {
        assert_eq!(
            parse_args([
                String::from("-u"),
                String::from("100"),
                String::from("-g"),
                String::from("101"),
                String::from("-s"),
                String::from("102"),
                String::from("--"),
                String::from("mkdir"),
                String::from("target"),
            ])
            .unwrap(),
            Options {
                uid: Some(100),
                gid: Some(101),
                supplementary_gids: vec![102],
                command: vec![String::from("mkdir"), String::from("target")],
            }
        );
    }
}
