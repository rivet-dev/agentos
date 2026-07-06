//! yq — YAML/XML/TOML/JSON processor using jaq filter engine.
//!
//! Converts input to JSON, runs jaq filter, converts output to requested format.
//! Reuses jaq-core/jaq-std/jaq-json (same engine as jq command).

use std::ffi::OsString;
use std::fmt;
use std::io::{self, Read, Write};

use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Compiler, Ctx, RcIter};
use jaq_json::Val;

const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_FORMATTED_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_OUTPUT_VALUES: usize = 100_000;
const MAX_XML_DEPTH: usize = 256;
const MAX_XML_NODES: usize = 100_000;
const MAX_XML_ATTRIBUTES_PER_ELEMENT: usize = 4096;
const MAX_XML_TEXT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq)]
enum Format {
    Yaml,
    Json,
    Toml,
    Xml,
}

struct YqOptions {
    filter: String,
    input_format: Option<Format>,
    output_format: Option<Format>,
    raw_output: bool,
    compact: bool,
    null_input: bool,
    slurp: bool,
}

/// Entry point for yq command.
pub fn main(args: Vec<OsString>) -> i32 {
    let str_args: Vec<String> = args
        .iter()
        .skip(1)
        .map(|a| a.to_string_lossy().to_string())
        .collect();

    match run_yq(&str_args) {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("yq: {}", msg);
            2
        }
    }
}

fn parse_format(s: &str) -> Result<Format, String> {
    match s {
        "yaml" | "y" => Ok(Format::Yaml),
        "json" | "j" => Ok(Format::Json),
        "toml" | "t" => Ok(Format::Toml),
        "xml" | "x" => Ok(Format::Xml),
        _ => Err(format!(
            "unknown format: {} (expected yaml, json, toml, xml)",
            s
        )),
    }
}

fn parse_args(args: &[String]) -> Result<YqOptions, String> {
    let mut opts = YqOptions {
        filter: String::new(),
        input_format: None,
        output_format: None,
        raw_output: false,
        compact: false,
        null_input: false,
        slurp: false,
    };

    let mut filter_set = false;
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        if arg == "--" {
            break;
        }

        if arg == "-p" || arg == "--input-format" {
            i += 1;
            if i >= args.len() {
                return Err("-p requires a format argument".to_string());
            }
            opts.input_format = Some(parse_format(&args[i])?);
        } else if arg == "-o" || arg == "--output-format" {
            i += 1;
            if i >= args.len() {
                return Err("-o requires a format argument".to_string());
            }
            opts.output_format = Some(parse_format(&args[i])?);
        } else if arg == "-r" || arg == "--raw-output" {
            opts.raw_output = true;
        } else if arg == "-c" || arg == "--compact-output" {
            opts.compact = true;
        } else if arg == "-n" || arg == "--null-input" {
            opts.null_input = true;
        } else if arg == "-s" || arg == "--slurp" {
            opts.slurp = true;
        } else if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            // Combined short flags like -rc
            let flags = &arg[1..];
            let mut chars = flags.chars().peekable();
            while let Some(c) = chars.next() {
                match c {
                    'r' => opts.raw_output = true,
                    'c' => opts.compact = true,
                    'n' => opts.null_input = true,
                    's' => opts.slurp = true,
                    'p' => {
                        let rest: String = chars.collect();
                        if !rest.is_empty() {
                            opts.input_format = Some(parse_format(&rest)?);
                        } else {
                            i += 1;
                            if i >= args.len() {
                                return Err("-p requires a format argument".to_string());
                            }
                            opts.input_format = Some(parse_format(&args[i])?);
                        }
                        break;
                    }
                    'o' => {
                        let rest: String = chars.collect();
                        if !rest.is_empty() {
                            opts.output_format = Some(parse_format(&rest)?);
                        } else {
                            i += 1;
                            if i >= args.len() {
                                return Err("-o requires a format argument".to_string());
                            }
                            opts.output_format = Some(parse_format(&args[i])?);
                        }
                        break;
                    }
                    _ => return Err(format!("unknown option: -{}", c)),
                }
            }
        } else if !filter_set {
            opts.filter = arg.clone();
            filter_set = true;
        } else {
            return Err(format!("unexpected argument: {}", arg));
        }

        i += 1;
    }

    if !filter_set {
        opts.filter = ".".to_string();
    }

    Ok(opts)
}

fn detect_format(input: &str) -> Format {
    let trimmed = input.trim_start();

    // JSON: starts with { or [
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
            return Format::Json;
        }
    }

    // XML: starts with < (including <?xml declaration)
    if trimmed.starts_with('<') {
        return Format::Xml;
    }

    // TOML: has key = value syntax or [section] headers, but not YAML's key: value
    if !trimmed.contains(": ") && !trimmed.starts_with("---") {
        if (trimmed.contains(" = ") || trimmed.contains("= \"") || trimmed.starts_with('['))
            && toml::from_str::<toml::Value>(trimmed).is_ok()
        {
            return Format::Toml;
        }
    }

    // Default: YAML
    Format::Yaml
}

fn parse_input(input: &str, format: Format) -> Result<serde_json::Value, String> {
    match format {
        Format::Json => serde_json::from_str(input).map_err(|e| format!("invalid JSON: {}", e)),
        Format::Yaml => serde_yaml::from_str(input).map_err(|e| format!("invalid YAML: {}", e)),
        Format::Toml => {
            let toml_val: toml::Value =
                toml::from_str(input).map_err(|e| format!("invalid TOML: {}", e))?;
            toml_to_json(toml_val)
        }
        Format::Xml => xml_to_json(input),
    }
}

fn toml_to_json(val: toml::Value) -> Result<serde_json::Value, String> {
    Ok(match val {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(i) => serde_json::Value::Number(serde_json::Number::from(i)),
        toml::Value::Float(f) => serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
        toml::Value::Array(arr) => {
            let items: Result<Vec<_>, _> = arr.into_iter().map(toml_to_json).collect();
            serde_json::Value::Array(items?)
        }
        toml::Value::Table(table) => {
            let mut map = serde_json::Map::new();
            for (k, v) in table {
                map.insert(k, toml_to_json(v)?);
            }
            serde_json::Value::Object(map)
        }
    })
}

fn json_to_toml(val: &serde_json::Value) -> Result<toml::Value, String> {
    Ok(match val {
        serde_json::Value::Null => return Err("TOML does not support null values".to_string()),
        serde_json::Value::Bool(b) => toml::Value::Boolean(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                toml::Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                toml::Value::Float(f)
            } else {
                return Err("unsupported number for TOML".to_string());
            }
        }
        serde_json::Value::String(s) => toml::Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            let items: Result<Vec<_>, _> = arr.iter().map(json_to_toml).collect();
            toml::Value::Array(items?)
        }
        serde_json::Value::Object(map) => {
            let mut table = toml::map::Map::new();
            for (k, v) in map {
                table.insert(k.clone(), json_to_toml(v)?);
            }
            toml::Value::Table(table)
        }
    })
}

// --- XML parsing ---

fn xml_to_json(input: &str) -> Result<serde_json::Value, String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    struct StackEntry {
        name: String,
        children: serde_json::Map<String, serde_json::Value>,
        text: String,
    }

    let mut reader = Reader::from_str(input);
    let mut stack: Vec<StackEntry> = Vec::new();
    let mut root = serde_json::Map::new();
    let mut nodes = 0usize;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                count_xml_node(&mut nodes)?;
                if stack.len() >= MAX_XML_DEPTH {
                    return Err("XML exceeds maximum nesting depth".to_string());
                }
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut children = serde_json::Map::new();
                let mut attr_count = 0usize;
                for attr in e.attributes() {
                    let attr = attr.map_err(|e| format!("invalid XML attribute: {}", e))?;
                    attr_count += 1;
                    if attr_count > MAX_XML_ATTRIBUTES_PER_ELEMENT {
                        return Err("XML element has too many attributes".to_string());
                    }
                    let key = format!("@{}", String::from_utf8_lossy(attr.key.as_ref()));
                    let val = String::from_utf8_lossy(&attr.value).to_string();
                    children.insert(key, serde_json::Value::String(val));
                }
                stack.push(StackEntry {
                    name,
                    children,
                    text: String::new(),
                });
            }
            Ok(Event::End(_)) => {
                let entry = stack.pop().ok_or("unexpected closing tag")?;
                let text = entry.text.trim().to_string();

                let value = if entry.children.is_empty() && text.is_empty() {
                    serde_json::Value::Null
                } else if entry.children.is_empty() {
                    serde_json::Value::String(text)
                } else {
                    let mut obj = entry.children;
                    if !text.is_empty() {
                        obj.insert("#text".to_string(), serde_json::Value::String(text));
                    }
                    serde_json::Value::Object(obj)
                };

                let target = if let Some(parent) = stack.last_mut() {
                    &mut parent.children
                } else {
                    &mut root
                };

                insert_or_array(target, entry.name, value);
            }
            Ok(Event::Empty(ref e)) => {
                count_xml_node(&mut nodes)?;
                if stack.len() >= MAX_XML_DEPTH {
                    return Err("XML exceeds maximum nesting depth".to_string());
                }
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut attrs = serde_json::Map::new();
                let mut attr_count = 0usize;
                for attr in e.attributes() {
                    let attr = attr.map_err(|e| format!("invalid XML attribute: {}", e))?;
                    attr_count += 1;
                    if attr_count > MAX_XML_ATTRIBUTES_PER_ELEMENT {
                        return Err("XML element has too many attributes".to_string());
                    }
                    let key = format!("@{}", String::from_utf8_lossy(attr.key.as_ref()));
                    let val = String::from_utf8_lossy(&attr.value).to_string();
                    attrs.insert(key, serde_json::Value::String(val));
                }

                let value = if attrs.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::Object(attrs)
                };

                let target = if let Some(parent) = stack.last_mut() {
                    &mut parent.children
                } else {
                    &mut root
                };

                insert_or_array(target, name, value);
            }
            Ok(Event::Text(ref e)) => {
                if let Some(entry) = stack.last_mut() {
                    let text = e
                        .unescape()
                        .map_err(|e| format!("invalid XML text: {}", e))?;
                    let next_len = entry
                        .text
                        .len()
                        .checked_add(text.len())
                        .ok_or("XML text length overflowed")?;
                    if next_len > MAX_XML_TEXT_BYTES {
                        return Err("XML text exceeds size limit".to_string());
                    }
                    entry.text.push_str(&text);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {} // Skip PI, Comment, Decl, DocType, CData
            Err(e) => return Err(format!("invalid XML: {}", e)),
        }
    }

    if !stack.is_empty() {
        return Err("unexpected end of XML input".to_string());
    }

    Ok(serde_json::Value::Object(root))
}

fn count_xml_node(nodes: &mut usize) -> Result<(), String> {
    *nodes = nodes.checked_add(1).ok_or("XML node count overflowed")?;
    if *nodes > MAX_XML_NODES {
        return Err("XML contains too many nodes".to_string());
    }
    Ok(())
}

fn record_output_value(output_count: &mut usize) -> Result<(), String> {
    *output_count = output_count
        .checked_add(1)
        .ok_or("output count overflowed")?;
    if *output_count > MAX_OUTPUT_VALUES {
        return Err("too many output values".to_string());
    }
    Ok(())
}

fn read_limited_string<R: Read>(reader: R) -> Result<String, String> {
    let mut input = String::new();
    reader
        .take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_string(&mut input)
        .map_err(|e| format!("failed to read stdin: {}", e))?;
    if input.len() > MAX_INPUT_BYTES {
        return Err("stdin exceeds size limit".to_string());
    }
    Ok(input)
}

fn insert_or_array(
    map: &mut serde_json::Map<String, serde_json::Value>,
    key: String,
    value: serde_json::Value,
) {
    if let Some(existing) = map.get_mut(&key) {
        match existing {
            serde_json::Value::Array(arr) => arr.push(value),
            _ => {
                let prev = existing.clone();
                *existing = serde_json::Value::Array(vec![prev, value]);
            }
        }
    } else {
        map.insert(key, value);
    }
}

// --- XML output ---

fn json_to_xml(val: &serde_json::Value) -> Result<String, String> {
    use quick_xml::Writer;

    let mut writer = Writer::new(LimitedBytes::new(MAX_FORMATTED_OUTPUT_BYTES));

    match val {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                write_xml_element(&mut writer, key, value)
                    .map_err(|e| format!("XML write error: {}", e))?;
            }
        }
        _ => {
            write_xml_element(&mut writer, "root", val)
                .map_err(|e| format!("XML write error: {}", e))?;
        }
    }

    writer
        .into_inner()
        .into_string()
        .map_err(|e| format!("XML encoding error: {}", e))
}

fn write_xml_element<W: io::Write>(
    writer: &mut quick_xml::Writer<W>,
    name: &str,
    val: &serde_json::Value,
) -> Result<(), quick_xml::Error> {
    use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};

    match val {
        serde_json::Value::Object(map) => {
            let mut elem = BytesStart::new(name);
            for (k, v) in map {
                if let Some(attr_name) = k.strip_prefix('@') {
                    if let serde_json::Value::String(s) = v {
                        elem.push_attribute((attr_name, s.as_str()));
                    }
                }
            }
            writer.write_event(Event::Start(elem))?;

            if let Some(serde_json::Value::String(text)) = map.get("#text") {
                writer.write_event(Event::Text(BytesText::new(text)))?;
            }

            for (k, v) in map {
                if k.starts_with('@') || k == "#text" {
                    continue;
                }
                match v {
                    serde_json::Value::Array(arr) => {
                        for item in arr {
                            write_xml_element(writer, k, item)?;
                        }
                    }
                    _ => write_xml_element(writer, k, v)?,
                }
            }

            writer.write_event(Event::End(BytesEnd::new(name)))?;
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                write_xml_element(writer, name, item)?;
            }
        }
        serde_json::Value::String(s) => {
            writer.write_event(Event::Start(BytesStart::new(name)))?;
            writer.write_event(Event::Text(BytesText::new(s)))?;
            writer.write_event(Event::End(BytesEnd::new(name)))?;
        }
        serde_json::Value::Number(n) => {
            let s = n.to_string();
            writer.write_event(Event::Start(BytesStart::new(name)))?;
            writer.write_event(Event::Text(BytesText::new(&s)))?;
            writer.write_event(Event::End(BytesEnd::new(name)))?;
        }
        serde_json::Value::Bool(b) => {
            let s = if *b { "true" } else { "false" };
            writer.write_event(Event::Start(BytesStart::new(name)))?;
            writer.write_event(Event::Text(BytesText::new(s)))?;
            writer.write_event(Event::End(BytesEnd::new(name)))?;
        }
        serde_json::Value::Null => {
            writer.write_event(Event::Empty(BytesStart::new(name)))?;
        }
    }
    Ok(())
}

// --- Output formatting ---

fn format_val_output(val: &Val, opts: &YqOptions, out_format: Format) -> Result<String, String> {
    let mut compact = LimitedString::new(MAX_FORMATTED_OUTPUT_BYTES);
    fmt::write(&mut compact, format_args!("{}", val))
        .map_err(|_| "formatted output exceeds size limit".to_string())?;
    let compact_str = compact.into_string();

    // Raw output: unquote strings
    if opts.raw_output {
        if compact_str.starts_with('"') && compact_str.ends_with('"') && compact_str.len() >= 2 {
            if let Ok(unescaped) = serde_json::from_str::<String>(&compact_str) {
                ensure_formatted_output_limit(unescaped.len())?;
                return Ok(unescaped);
            }
        }
    }

    let json_val: serde_json::Value =
        serde_json::from_str(&compact_str).unwrap_or(serde_json::Value::String(compact_str));

    let output = format_json_as(out_format, &json_val, opts.compact)?;
    ensure_formatted_output_limit(output.len())?;
    Ok(output)
}

fn ensure_formatted_output_limit(len: usize) -> Result<(), String> {
    if len > MAX_FORMATTED_OUTPUT_BYTES {
        return Err("formatted output exceeds size limit".to_string());
    }
    Ok(())
}

struct LimitedString {
    inner: String,
    limit: usize,
}

impl LimitedString {
    fn new(limit: usize) -> Self {
        Self {
            inner: String::new(),
            limit,
        }
    }

    fn into_string(self) -> String {
        self.inner
    }

    fn write_str(&mut self, s: &str) -> Result<(), String> {
        let next_len = self
            .inner
            .len()
            .checked_add(s.len())
            .ok_or("formatted output length overflowed")?;
        if next_len > self.limit {
            return Err("formatted output exceeds size limit".to_string());
        }
        self.inner.push_str(s);
        Ok(())
    }

    fn write_char(&mut self, ch: char) -> Result<(), String> {
        let mut buf = [0u8; 4];
        self.write_str(ch.encode_utf8(&mut buf))
    }
}

impl fmt::Write for LimitedString {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        LimitedString::write_str(self, s).map_err(|_| fmt::Error)
    }
}

struct LimitedBytes {
    inner: Vec<u8>,
    limit: usize,
}

impl LimitedBytes {
    fn new(limit: usize) -> Self {
        Self {
            inner: Vec::new(),
            limit,
        }
    }

    fn into_string(self) -> Result<String, std::string::FromUtf8Error> {
        String::from_utf8(self.inner)
    }
}

impl io::Write for LimitedBytes {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let next_len = self
            .inner
            .len()
            .checked_add(buf.len())
            .ok_or_else(|| io::Error::other("formatted output length overflowed"))?;
        if next_len > self.limit {
            return Err(io::Error::other("formatted output exceeds size limit"));
        }
        self.inner.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn format_json_as(
    format: Format,
    val: &serde_json::Value,
    compact: bool,
) -> Result<String, String> {
    match format {
        Format::Json => {
            let mut out = LimitedBytes::new(MAX_FORMATTED_OUTPUT_BYTES);
            if compact {
                serde_json::to_writer(&mut out, val)
                    .map_err(|e| format!("JSON output error: {}", e))?;
            } else {
                serde_json::to_writer_pretty(&mut out, val)
                    .map_err(|e| format!("JSON output error: {}", e))?;
            }
            out.into_string()
                .map_err(|e| format!("JSON encoding error: {}", e))
        }
        Format::Yaml => {
            let mut out = LimitedBytes::new(MAX_FORMATTED_OUTPUT_BYTES);
            serde_yaml::to_writer(&mut out, val)
                .map_err(|e| format!("YAML output error: {}", e))?;
            let s = out
                .into_string()
                .map_err(|e| format!("YAML encoding error: {}", e))?;
            // Strip leading "---\n" and trailing newline for cleaner output
            let s = s.strip_prefix("---\n").unwrap_or(&s);
            let s = s.strip_suffix('\n').unwrap_or(s);
            Ok(s.to_string())
        }
        Format::Toml => json_to_toml_bounded(val),
        Format::Xml => json_to_xml(val),
    }
}

fn json_to_toml_bounded(val: &serde_json::Value) -> Result<String, String> {
    let toml_val = json_to_toml(val)?;
    let mut out = LimitedString::new(MAX_FORMATTED_OUTPUT_BYTES);
    write_toml_document(&mut out, &toml_val)?;
    let s = out.into_string();
    Ok(s.strip_suffix('\n').unwrap_or(&s).to_string())
}

fn write_toml_document(out: &mut LimitedString, val: &toml::Value) -> Result<(), String> {
    match val {
        toml::Value::Table(table) => write_toml_table(out, &mut Vec::new(), table),
        other => write_toml_inline(out, other),
    }
}

fn write_toml_table(
    out: &mut LimitedString,
    path: &mut Vec<String>,
    table: &toml::map::Map<String, toml::Value>,
) -> Result<(), String> {
    for (key, value) in table {
        if matches!(value, toml::Value::Table(_)) {
            continue;
        }
        write_toml_key(out, key)?;
        out.write_str(" = ")?;
        write_toml_inline(out, value)?;
        out.write_char('\n')?;
    }

    for (key, value) in table {
        let toml::Value::Table(child) = value else {
            continue;
        };
        if !path.is_empty() || table_has_scalar_entries(child) {
            out.write_char('\n')?;
            path.push(key.clone());
            out.write_char('[')?;
            write_toml_path(out, path)?;
            out.write_str("]\n")?;
            write_toml_table(out, path, child)?;
            path.pop();
        } else {
            path.push(key.clone());
            write_toml_table(out, path, child)?;
            path.pop();
        }
    }

    Ok(())
}

fn table_has_scalar_entries(table: &toml::map::Map<String, toml::Value>) -> bool {
    table
        .values()
        .any(|value| !matches!(value, toml::Value::Table(_)))
}

fn write_toml_path(out: &mut LimitedString, path: &[String]) -> Result<(), String> {
    for (i, key) in path.iter().enumerate() {
        if i > 0 {
            out.write_char('.')?;
        }
        write_toml_key(out, key)?;
    }
    Ok(())
}

fn write_toml_key(out: &mut LimitedString, key: &str) -> Result<(), String> {
    if !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        out.write_str(key)?;
    } else {
        write_toml_string(out, key)?;
    }
    Ok(())
}

fn write_toml_inline(out: &mut LimitedString, val: &toml::Value) -> Result<(), String> {
    match val {
        toml::Value::String(s) => write_toml_string(out, s),
        toml::Value::Integer(i) => out.write_str(&i.to_string()),
        toml::Value::Float(f) => out.write_str(&f.to_string()),
        toml::Value::Boolean(b) => out.write_str(if *b { "true" } else { "false" }),
        toml::Value::Datetime(dt) => out.write_str(&dt.to_string()),
        toml::Value::Array(arr) => {
            out.write_char('[')?;
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.write_str(", ")?;
                }
                write_toml_inline(out, item)?;
            }
            out.write_char(']')
        }
        toml::Value::Table(table) => {
            out.write_str("{ ")?;
            for (i, (key, value)) in table.iter().enumerate() {
                if i > 0 {
                    out.write_str(", ")?;
                }
                write_toml_key(out, key)?;
                out.write_str(" = ")?;
                write_toml_inline(out, value)?;
            }
            out.write_str(" }")
        }
    }
}

fn write_toml_string(out: &mut LimitedString, s: &str) -> Result<(), String> {
    out.write_char('"')?;
    for ch in s.chars() {
        match ch {
            '"' => out.write_str("\\\"")?,
            '\\' => out.write_str("\\\\")?,
            '\n' => out.write_str("\\n")?,
            '\r' => out.write_str("\\r")?,
            '\t' => out.write_str("\\t")?,
            '\u{08}' => out.write_str("\\b")?,
            '\u{0c}' => out.write_str("\\f")?,
            ch if ch.is_control() => out.write_str(&format!("\\u{:04X}", ch as u32))?,
            ch => out.write_char(ch)?,
        }
    }
    out.write_char('"')
}

// --- Main logic ---

fn run_yq(args: &[String]) -> Result<i32, String> {
    let opts = parse_args(args)?;

    // Read input
    let stdin_data = if opts.null_input {
        String::new()
    } else {
        read_limited_string(io::stdin())?
    };

    // Determine input format
    let in_format = opts.input_format.unwrap_or_else(|| {
        if opts.null_input {
            Format::Yaml
        } else {
            detect_format(&stdin_data)
        }
    });

    // Default output format: YAML for YAML input, otherwise matches input
    let out_format = opts.output_format.unwrap_or(in_format);

    // Parse input to JSON, then convert to jaq Val
    let inputs = if opts.null_input {
        vec![Val::from(serde_json::Value::Null)]
    } else {
        let json_val = parse_input(&stdin_data, in_format)?;
        if opts.slurp {
            match json_val {
                serde_json::Value::Array(_) => vec![Val::from(json_val)],
                _ => vec![Val::from(serde_json::Value::Array(vec![json_val]))],
            }
        } else {
            vec![Val::from(json_val)]
        }
    };

    // Compile jaq filter (same engine as jq)
    let loader = Loader::new(jaq_std::defs().chain(jaq_json::defs()));
    let arena = Arena::default();
    let program = File {
        code: opts.filter.as_str(),
        path: (),
    };
    let modules = loader
        .load(&arena, program)
        .map_err(|errs| format!("parse error: {:?}", errs))?;

    let filter = Compiler::default()
        .with_funs(jaq_std::funs().chain(jaq_json::funs()))
        .compile(modules)
        .map_err(|errs| format!("compile error: {:?}", errs))?;

    // Execute filter and output
    let empty_inputs = RcIter::new(core::iter::empty());
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut output_count = 0usize;

    for input in inputs {
        let ctx = Ctx::new(core::iter::empty(), &empty_inputs);
        let results = filter.run((ctx, input));

        for result in results {
            match result {
                Ok(val) => {
                    record_output_value(&mut output_count)?;
                    let s = format_val_output(&val, &opts, out_format)?;
                    writeln!(out, "{}", s).map_err(|e| format!("failed to write stdout: {}", e))?;
                }
                Err(e) => {
                    eprintln!("yq: error: {}", e);
                    return Ok(5);
                }
            }
        }
    }

    out.flush()
        .map_err(|e| format!("failed to flush stdout: {}", e))?;

    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_depth_limit_rejects_deep_input() {
        let mut input = String::new();
        for i in 0..=MAX_XML_DEPTH {
            input.push_str(&format!("<n{i}>"));
        }
        for i in (0..=MAX_XML_DEPTH).rev() {
            input.push_str(&format!("</n{i}>"));
        }

        let err = xml_to_json(&input).unwrap_err();

        assert!(err.contains("nesting depth"));
    }

    #[test]
    fn xml_depth_limit_rejects_deep_empty_input() {
        let mut input = String::new();
        for i in 0..MAX_XML_DEPTH {
            input.push_str(&format!("<n{i}>"));
        }
        input.push_str("<leaf />");
        for i in (0..MAX_XML_DEPTH).rev() {
            input.push_str(&format!("</n{i}>"));
        }

        let err = xml_to_json(&input).unwrap_err();

        assert!(err.contains("nesting depth"));
    }

    #[test]
    fn xml_node_limit_rejects_many_elements() {
        let mut nodes = MAX_XML_NODES;

        let err = count_xml_node(&mut nodes).unwrap_err();

        assert!(err.contains("too many nodes"));
    }

    #[test]
    fn xml_text_limit_rejects_large_text() {
        let input = format!("<root>{}</root>", "x".repeat(MAX_XML_TEXT_BYTES + 1));

        let err = xml_to_json(&input).unwrap_err();

        assert!(err.contains("text exceeds"));
    }

    #[test]
    fn output_limit_rejects_too_many_values() {
        let mut count = MAX_OUTPUT_VALUES;

        let err = record_output_value(&mut count).unwrap_err();

        assert!(err.contains("too many output values"));
    }

    #[test]
    fn formatted_output_limit_rejects_large_output() {
        let err = ensure_formatted_output_limit(MAX_FORMATTED_OUTPUT_BYTES + 1).unwrap_err();

        assert!(err.contains("formatted output"));
    }

    #[test]
    fn limited_bytes_rejects_large_serializer_write() {
        let mut output = LimitedBytes::new(4);

        let err = output.write_all(b"hello").unwrap_err();

        assert!(err.to_string().contains("formatted output"));
    }

    #[test]
    fn limited_toml_writer_rejects_large_value() {
        let mut output = LimitedString::new(4);
        let value = toml::Value::String("hello".to_string());

        let err = write_toml_inline(&mut output, &value).unwrap_err();

        assert!(err.contains("formatted output"));
    }

    #[test]
    fn input_limit_rejects_oversized_reader() {
        let input = vec![b'x'; MAX_INPUT_BYTES + 1];

        let err = read_limited_string(&input[..]).unwrap_err();

        assert!(err.contains("stdin exceeds"));
    }

    #[test]
    fn xml_rejects_unclosed_elements() {
        let err = xml_to_json("<root>").unwrap_err();

        assert!(err.contains("unexpected end"));
    }

    #[test]
    fn xml_rejects_invalid_text_escape() {
        let err = xml_to_json("<root>&bogus;</root>").unwrap_err();

        assert!(err.contains("invalid XML text"));
    }
}
