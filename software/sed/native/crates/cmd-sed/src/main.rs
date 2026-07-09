fn main() {
    use std::io::Write;

    let args = normalize_in_place_args(std::env::args_os());
    let mut code = sed::sed::uumain(args.into_iter());
    if let Err(error) = std::io::stdout().flush() {
        eprintln!("Error flushing stdout: {error}");
        if code == 0 {
            code = 1;
        }
    }
    std::process::exit(code);
}

fn normalize_in_place_args(
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> Vec<std::ffi::OsString> {
    args.into_iter()
        .enumerate()
        .map(|(index, arg)| {
            if index == 0 {
                return arg;
            }
            let value = arg.to_string_lossy();
            if value == "-i" || value == "--in-place" {
                std::ffi::OsString::from("--in-place=")
            } else if let Some(suffix) = value.strip_prefix("-i").filter(|_| value.len() > 2) {
                std::ffi::OsString::from(format!("--in-place={suffix}"))
            } else {
                arg
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::normalize_in_place_args;
    use std::ffi::OsString;

    fn normalized(args: &[&str]) -> Vec<String> {
        normalize_in_place_args(args.iter().map(OsString::from))
            .into_iter()
            .map(|arg| arg.into_string().expect("UTF-8 test argument"))
            .collect()
    }

    #[test]
    fn standalone_in_place_does_not_consume_the_script() {
        assert_eq!(
            normalized(&["sed", "-i", "s/a/b/", "file"]),
            ["sed", "--in-place=", "s/a/b/", "file"]
        );
    }

    #[test]
    fn attached_in_place_suffix_is_preserved() {
        assert_eq!(
            normalized(&["sed", "-i.bak", "s/a/b/", "file"]),
            ["sed", "--in-place=.bak", "s/a/b/", "file"]
        );
    }
}
