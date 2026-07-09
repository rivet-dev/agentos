use std::env;
use std::process::Command;

#[derive(Debug)]
struct Account {
    uid: u32,
    gid: u32,
    name: String,
    home: String,
    shell: String,
}

fn record(operation: &str, bytes: &[u8], len: u32) -> Result<String, String> {
    std::str::from_utf8(&bytes[..len as usize])
        .map(str::to_owned)
        .map_err(|_| format!("{operation} returned non-UTF-8 data"))
}

fn account_by_name(name: &str) -> Result<Account, String> {
    let mut buffer = vec![0; 4096];
    let len = wasi_ext::get_pwnam(name, &mut buffer)
        .map_err(|errno| format!("unknown user {name} (errno {errno})"))?;
    let raw = record("getpwnam", &buffer, len)?;
    let fields = raw.split(':').collect::<Vec<_>>();
    if fields.len() != 7 {
        return Err(String::from("invalid passwd record from AgentOS"));
    }
    Ok(Account {
        uid: fields[2]
            .parse()
            .map_err(|_| String::from("invalid passwd uid"))?,
        gid: fields[3]
            .parse()
            .map_err(|_| String::from("invalid passwd gid"))?,
        name: fields[0].to_owned(),
        home: fields[5].to_owned(),
        shell: fields[6].to_owned(),
    })
}

fn groups_for(account: &Account) -> Result<Vec<u32>, String> {
    let mut groups = vec![account.gid];
    for index in 0..128 {
        let mut buffer = vec![0; 4096];
        let len = match wasi_ext::get_grent(index, &mut buffer) {
            Ok(len) => len,
            Err(wasi_ext::ERRNO_NOENT) => break,
            Err(errno) => return Err(format!("getgrent failed with errno {errno}")),
        };
        let raw = record("getgrent", &buffer, len)?;
        let fields = raw.split(':').collect::<Vec<_>>();
        if fields.len() != 4 {
            return Err(String::from("invalid group record from AgentOS"));
        }
        let gid = fields[2]
            .parse::<u32>()
            .map_err(|_| String::from("invalid group gid"))?;
        if fields[3].split(',').any(|member| member == account.name) && !groups.contains(&gid) {
            groups.push(gid);
        }
    }
    Ok(groups)
}

fn parse_args() -> Result<(String, Option<String>, Option<String>), String> {
    parse_args_from(env::args().skip(1))
}

fn parse_args_from(
    args: impl IntoIterator<Item = String>,
) -> Result<(String, Option<String>, Option<String>), String> {
    let mut args = args.into_iter();
    let mut shell = None;
    let mut command = None;
    let mut user = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-s" | "--shell" => {
                shell = Some(args.next().ok_or_else(|| String::from("missing shell"))?)
            }
            _ if arg.starts_with("--shell=") => shell = Some(arg["--shell=".len()..].to_owned()),
            _ if arg.starts_with("-s") && arg.len() > 2 => shell = Some(arg[2..].to_owned()),
            "-c" | "--command" => {
                command = Some(args.next().ok_or_else(|| String::from("missing command"))?)
            }
            _ if arg.starts_with("--command=") => {
                command = Some(arg["--command=".len()..].to_owned())
            }
            "-" | "-l" | "--login" => {}
            _ if arg.starts_with('-') => return Err(format!("unsupported option {arg}")),
            _ => {
                if user.is_some() {
                    return Err(String::from("extra operand"));
                }
                user = Some(arg);
            }
        }
    }
    Ok((user.unwrap_or_else(|| String::from("root")), shell, command))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xfstests_compact_shell_form() {
        assert_eq!(
            parse_args_from(
                ["-s/bin/bash", "-", "fsgqa", "-c", "id -u"]
                    .into_iter()
                    .map(str::to_owned)
            ),
            Ok((
                String::from("fsgqa"),
                Some(String::from("/bin/bash")),
                Some(String::from("id -u"))
            ))
        );
    }

    #[test]
    fn parses_long_equals_forms() {
        assert_eq!(
            parse_args_from(
                ["--shell=/bin/sh", "--command=true", "fsgqa"]
                    .into_iter()
                    .map(str::to_owned)
            ),
            Ok((
                String::from("fsgqa"),
                Some(String::from("/bin/sh")),
                Some(String::from("true"))
            ))
        );
    }
}

fn run() -> Result<i32, String> {
    let (user, shell_override, command) = parse_args()?;
    let account = account_by_name(&user)?;
    let groups = groups_for(&account)?;
    wasi_ext::set_groups(&groups).map_err(|errno| format!("setgroups failed: errno {errno}"))?;
    wasi_ext::set_gid(account.gid).map_err(|errno| format!("setgid failed: errno {errno}"))?;
    wasi_ext::set_uid(account.uid).map_err(|errno| format!("setuid failed: errno {errno}"))?;
    env::set_var("HOME", &account.home);
    env::set_var("USER", &account.name);
    env::set_var("LOGNAME", &account.name);
    let shell = shell_override.unwrap_or(account.shell);
    let mut child = Command::new(shell);
    if let Some(command) = command {
        child.args(["-c", &command]);
    }
    child
        .status()
        .map(|status| status.code().unwrap_or(1))
        .map_err(|error| format!("failed to execute user shell: {error}"))
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("su: {error}");
            std::process::exit(1);
        }
    }
}
