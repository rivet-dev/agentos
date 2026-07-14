use std::collections::BTreeMap;

pub const DEFAULT_GUEST_PATH_ENV: &str =
    "/usr/local/sbin:/usr/local/bin:/opt/agentos/bin:/usr/sbin:/usr/bin:/sbin:/bin";

pub fn default_guest_environment() -> BTreeMap<String, String> {
    BTreeMap::from([
        (String::from("CHARSET"), String::from("UTF-8")),
        (String::from("HOME"), String::from("/home/agentos")),
        (String::from("HOSTNAME"), String::from("secure-exec")),
        (String::from("LANG"), String::from("C.UTF-8")),
        (String::from("LC_COLLATE"), String::from("C")),
        (String::from("LOGNAME"), String::from("agentos")),
        (String::from("PAGER"), String::from("less")),
        (String::from("PATH"), String::from(DEFAULT_GUEST_PATH_ENV)),
        (
            String::from("MANPATH"),
            String::from("/opt/agentos/share/man:/usr/local/share/man:/usr/share/man"),
        ),
        (String::from("SHELL"), String::from("/bin/sh")),
        (String::from("USER"), String::from("agentos")),
        (String::from("PS1"), String::from("\\h:\\w\\$ ")),
    ])
}

pub fn guest_environment_with_overrides(
    overrides: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut environment = default_guest_environment();
    environment.extend(overrides.clone());
    environment
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_environment_defaults_are_runtime_owned_and_explicit_values_win() {
        let environment = guest_environment_with_overrides(&BTreeMap::from([
            (String::from("HOME"), String::from("/custom-home")),
            (String::from("CUSTOM"), String::from("value")),
        ]));

        assert_eq!(
            environment.get("HOME").map(String::as_str),
            Some("/custom-home")
        );
        assert_eq!(environment.get("CUSTOM").map(String::as_str), Some("value"));
        assert_eq!(environment.get("USER").map(String::as_str), Some("agentos"));
        assert_eq!(
            environment.get("SHELL").map(String::as_str),
            Some("/bin/sh")
        );
        assert_eq!(
            environment.get("PATH").map(String::as_str),
            Some(DEFAULT_GUEST_PATH_ENV)
        );
        assert_eq!(environment.get("LANG").map(String::as_str), Some("C.UTF-8"));
    }
}
