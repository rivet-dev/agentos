use std::env;
use std::process::ExitCode;

#[derive(Debug, PartialEq, Eq)]
struct RemountRequest {
    target: String,
    options: String,
}

fn main() -> ExitCode {
    let request = match parse_args(env::args().skip(1)) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("mount: {error}");
            return ExitCode::from(32);
        }
    };

    #[cfg(target_arch = "wasm32")]
    match wasi_ext::remount_path(&request.target, &request.options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(errno) => {
            eprintln!(
                "mount: cannot remount {}: WASI errno {errno}",
                request.target
            );
            ExitCode::from(32)
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = request;
        eprintln!("mount: AgentOS remount is only available inside a VM");
        ExitCode::from(32)
    }
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<RemountRequest, String> {
    let mut options = None;
    let mut fstype = None;
    let mut positional = Vec::new();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" => options = Some(next_value(&mut args, "-o")?),
            "-t" => fstype = Some(next_value(&mut args, "-t")?),
            "--" => {
                positional.extend(args.by_ref());
                break;
            }
            _ if arg.starts_with("-o") && arg.len() > 2 => options = Some(arg[2..].to_owned()),
            _ if arg.starts_with("-t") && arg.len() > 2 => fstype = Some(arg[2..].to_owned()),
            "--help" => {
                return Err(String::from(
                    "usage: mount -o remount,<options> [source] target",
                ));
            }
            _ if arg.starts_with('-') => return Err(format!("unsupported option: {arg}")),
            _ => positional.push(arg),
        }
    }
    if let Some(fstype) = fstype {
        if fstype != "agentos" {
            return Err(format!("unsupported filesystem type: {fstype}"));
        }
    }
    if !(1..=2).contains(&positional.len()) {
        return Err(String::from("expected [source] target"));
    }
    let options = options.ok_or_else(|| String::from("remount options are required"))?;
    if !options.split(',').any(|option| option.trim() == "remount") {
        return Err(String::from("only existing-mount remounts are supported"));
    }
    Ok(RemountRequest {
        target: positional.pop().expect("validated positional target"),
        options,
    })
}

fn next_value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value for {option}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xfstests_remount_shape() {
        assert_eq!(
            parse_args(
                [
                    "-t",
                    "agentos",
                    "-o",
                    "remount,ro,strictatime",
                    "/dev/agentos-test",
                    "/mnt/test",
                ]
                .into_iter()
                .map(str::to_owned),
            )
            .unwrap(),
            RemountRequest {
                target: String::from("/mnt/test"),
                options: String::from("remount,ro,strictatime"),
            }
        );
    }

    #[test]
    fn rejects_new_mounts_and_other_filesystem_types() {
        assert!(parse_args(
            ["-o", "strictatime", "/mnt/test"]
                .into_iter()
                .map(str::to_owned)
        )
        .unwrap_err()
        .contains("only existing-mount"));
        assert!(parse_args(
            ["-t", "ext4", "-o", "remount", "/dev/test", "/mnt/test"]
                .into_iter()
                .map(str::to_owned)
        )
        .unwrap_err()
        .contains("unsupported filesystem type"));
    }
}
