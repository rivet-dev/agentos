use std::env;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Selection {
    User,
    Group,
    Groups,
}

#[derive(Debug, Default, Eq, PartialEq)]
struct Options {
    selection: Option<Selection>,
    name: bool,
    real: bool,
    zero: bool,
    operand: Option<String>,
}

#[derive(Debug)]
struct Passwd {
    name: String,
    uid: u32,
    gid: u32,
}

#[derive(Debug)]
struct Identity {
    real_uid: u32,
    effective_uid: u32,
    real_gid: u32,
    effective_gid: u32,
    groups: Vec<u32>,
    passwd: Passwd,
}

fn set_selection(options: &mut Options, selection: Selection) -> Result<(), String> {
    if options
        .selection
        .is_some_and(|current| current != selection)
    {
        return Err(String::from(
            "cannot print only names or real IDs in more than one mode",
        ));
    }
    options.selection = Some(selection);
    Ok(())
}

fn apply_short_options(options: &mut Options, value: &str) -> Result<(), String> {
    for flag in value.chars() {
        match flag {
            'u' => set_selection(options, Selection::User)?,
            'g' => set_selection(options, Selection::Group)?,
            'G' => set_selection(options, Selection::Groups)?,
            'n' => options.name = true,
            'r' => options.real = true,
            'z' => options.zero = true,
            _ => return Err(format!("unknown option -- {flag}")),
        }
    }
    Ok(())
}

fn parse_options(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut options = Options::default();
    let mut operands = false;
    for arg in args {
        if !operands && arg == "--" {
            operands = true;
        } else if !operands && arg.starts_with("--") {
            match arg.as_str() {
                "--user" => set_selection(&mut options, Selection::User)?,
                "--group" => set_selection(&mut options, Selection::Group)?,
                "--groups" => set_selection(&mut options, Selection::Groups)?,
                "--name" => options.name = true,
                "--real" => options.real = true,
                "--zero" => options.zero = true,
                "--help" => return Err(String::from("help")),
                _ => return Err(format!("unknown option {arg}")),
            }
        } else if !operands && arg.starts_with('-') && arg.len() > 1 {
            apply_short_options(&mut options, &arg[1..])?;
        } else if options.operand.replace(arg).is_some() {
            return Err(String::from("extra operand"));
        }
    }
    if (options.name || options.real) && options.selection.is_none() {
        return Err(String::from("-n and -r require -u, -g, or -G"));
    }
    Ok(options)
}

#[cfg(any(target_os = "wasi", test))]
fn parse_passwd(raw: &str) -> Result<Passwd, String> {
    let fields = raw.split(':').collect::<Vec<_>>();
    if fields.len() != 7 {
        return Err(String::from("invalid passwd record from AgentOS"));
    }
    Ok(Passwd {
        name: fields[0].to_owned(),
        uid: fields[2]
            .parse()
            .map_err(|_| String::from("invalid passwd uid from AgentOS"))?,
        gid: fields[3]
            .parse()
            .map_err(|_| String::from("invalid passwd gid from AgentOS"))?,
    })
}

#[cfg(target_os = "wasi")]
fn passwd_for(uid: u32) -> Result<Passwd, String> {
    let mut buffer = vec![0u8; 4096];
    let len = wasi_ext::get_pwuid(uid, &mut buffer)
        .map_err(|errno| format!("getpwuid({uid}) failed with WASI errno {errno}"))?;
    let raw = std::str::from_utf8(&buffer[..len as usize])
        .map_err(|_| String::from("passwd record from AgentOS is not UTF-8"))?;
    parse_passwd(raw)
}

#[cfg(target_os = "wasi")]
fn group_name_for(gid: u32) -> Result<String, String> {
    let mut buffer = vec![0u8; 4096];
    let len = wasi_ext::get_grgid(gid, &mut buffer)
        .map_err(|errno| format!("getgrgid({gid}) failed with WASI errno {errno}"))?;
    let raw = std::str::from_utf8(&buffer[..len as usize])
        .map_err(|_| String::from("group record from AgentOS is not UTF-8"))?;
    raw.split(':')
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| String::from("invalid group record from AgentOS"))
}

#[cfg(not(target_os = "wasi"))]
fn group_name_for(gid: u32) -> Result<String, String> {
    Ok(if gid == 0 {
        String::from("root")
    } else {
        format!("group{gid}")
    })
}

#[cfg(not(target_os = "wasi"))]
fn passwd_for(uid: u32) -> Result<Passwd, String> {
    Ok(Passwd {
        name: if uid == 0 {
            String::from("root")
        } else {
            format!("user{uid}")
        },
        uid,
        gid: uid,
    })
}

#[cfg(target_os = "wasi")]
fn current_identity() -> Result<Identity, String> {
    let real_uid = wasi_ext::get_uid().map_err(|errno| format!("getuid failed: {errno}"))?;
    let effective_uid = wasi_ext::get_euid().map_err(|errno| format!("geteuid failed: {errno}"))?;
    let real_gid = wasi_ext::get_gid().map_err(|errno| format!("getgid failed: {errno}"))?;
    let effective_gid = wasi_ext::get_egid().map_err(|errno| format!("getegid failed: {errno}"))?;
    let groups = wasi_ext::get_groups().map_err(|errno| format!("getgroups failed: {errno}"))?;
    Ok(Identity {
        real_uid,
        effective_uid,
        real_gid,
        effective_gid,
        groups,
        passwd: passwd_for(real_uid)?,
    })
}

#[cfg(not(target_os = "wasi"))]
fn current_identity() -> Result<Identity, String> {
    Ok(Identity {
        real_uid: 0,
        effective_uid: 0,
        real_gid: 0,
        effective_gid: 0,
        groups: vec![0],
        passwd: passwd_for(0)?,
    })
}

fn selected_identity(mut identity: Identity, operand: Option<&str>) -> Result<Identity, String> {
    let Some(operand) = operand else {
        return Ok(identity);
    };
    let passwd = match operand.parse::<u32>() {
        Ok(uid) => passwd_for(uid)?,
        Err(_) if operand == identity.passwd.name => passwd_for(identity.real_uid)?,
        Err(_) => return Err(format!("no such user: {operand}")),
    };
    identity.real_uid = passwd.uid;
    identity.effective_uid = passwd.uid;
    identity.real_gid = passwd.gid;
    identity.effective_gid = passwd.gid;
    identity.groups = vec![passwd.gid];
    identity.passwd = passwd;
    Ok(identity)
}

fn render(options: &Options, identity: &Identity) -> Result<String, String> {
    let separator = if options.zero { '\0' } else { '\n' };
    let value = match options.selection {
        Some(Selection::User) => {
            let uid = if options.real {
                identity.real_uid
            } else {
                identity.effective_uid
            };
            if options.name {
                identity.passwd.name.clone()
            } else {
                uid.to_string()
            }
        }
        Some(Selection::Group) => {
            let gid = if options.real {
                identity.real_gid
            } else {
                identity.effective_gid
            };
            if options.name {
                identity.passwd.name.clone()
            } else {
                gid.to_string()
            }
        }
        Some(Selection::Groups) => identity
            .groups
            .iter()
            .map(|gid| {
                if options.name {
                    group_name_for(*gid)
                } else {
                    Ok(gid.to_string())
                }
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(if options.zero { "\0" } else { " " }),
        None => format!(
            "uid={}({}) gid={}({}) groups={}",
            identity.real_uid,
            identity.passwd.name,
            identity.real_gid,
            identity.passwd.name,
            identity
                .groups
                .iter()
                .map(|gid| gid.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
    };
    Ok(format!("{value}{separator}"))
}

fn usage() {
    println!("Usage: id [-ugGnr] [USER]");
}

fn main() {
    let options = match parse_options(env::args().skip(1)) {
        Ok(options) => options,
        Err(error) if error == "help" => {
            usage();
            return;
        }
        Err(error) => {
            eprintln!("id: {error}");
            std::process::exit(1);
        }
    };
    let result = current_identity()
        .and_then(|identity| selected_identity(identity, options.operand.as_deref()))
        .and_then(|identity| render(&options, &identity));
    match result {
        Ok(output) => print!("{output}"),
        Err(error) => {
            eprintln!("id: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_compact_options() {
        assert_eq!(
            parse_options([String::from("-un")]).unwrap(),
            Options {
                selection: Some(Selection::User),
                name: true,
                ..Options::default()
            }
        );
    }

    #[test]
    fn renders_numeric_and_named_user() {
        let identity = current_identity().unwrap();
        assert_eq!(
            render(
                &Options {
                    selection: Some(Selection::User),
                    ..Options::default()
                },
                &identity
            )
            .unwrap(),
            "0\n"
        );
        assert_eq!(
            render(
                &Options {
                    selection: Some(Selection::User),
                    name: true,
                    ..Options::default()
                },
                &identity
            )
            .unwrap(),
            "root\n"
        );
    }

    #[test]
    fn rejects_name_without_a_selection() {
        assert!(parse_options([String::from("-n")]).is_err());
    }

    #[test]
    fn parses_agentos_passwd_record() {
        let passwd = parse_passwd("alice:x:1000:100::/home/alice:/bin/sh").unwrap();
        assert_eq!(passwd.name, "alice");
        assert_eq!(passwd.uid, 1000);
        assert_eq!(passwd.gid, 100);
    }
}
