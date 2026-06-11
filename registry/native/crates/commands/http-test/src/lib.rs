pub fn parse_header(raw: &str) -> Result<(String, String), String> {
    let Some((name, value)) = raw.split_once(':') else {
        return Err("invalid header".to_string());
    };

    let value = trim_header_value_ows(value);
    if !is_valid_header_name(name) || !is_valid_header_value(value) {
        return Err("invalid header".to_string());
    }

    Ok((name.to_string(), value.to_string()))
}

fn is_valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| matches!(byte, b'!' | b'#'..=b'\'' | b'*' | b'+' | b'-' | b'.' | b'0'..=b'9' | b'A'..=b'Z' | b'^'..=b'z' | b'|' | b'~'))
}

fn is_valid_header_value(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| matches!(byte, b'\t' | b' '..=b'~') || byte >= 0x80)
}

fn trim_header_value_ows(value: &str) -> &str {
    value.trim_matches(|ch| matches!(ch, ' ' | '\t'))
}
