//! jq implementation using the jaq crate (pure Rust, jq-compatible).
//!
//! Wraps jaq-core/jaq-std/jaq-json to provide a standard jq CLI interface.

use std::ffi::OsString;
use std::fs::File as FsFile;
use std::io::{self, Read, Write};

use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Compiler, Ctx, RcIter};
use jaq_json::Val;

const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_INPUT_VALUES: usize = 100_000;

/// Entry point for jq command.
pub fn main(args: Vec<OsString>) -> i32 {
    let str_args: Vec<String> = args
        .iter()
        .skip(1) // skip argv[0]
        .map(|a| a.to_string_lossy().to_string())
        .collect();

    match run_jq(&str_args) {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("jq: {}", msg);
            2
        }
    }
}

struct JqOptions {
    filter: String,
    raw_output: bool,
    raw_input: bool,
    slurp: bool,
    compact: bool,
    null_input: bool,
    exit_status: bool,
    join_output: bool,
    args: Vec<(String, String)>,
    jsonargs: Vec<(String, Val)>,
    input_paths: Vec<String>,
}

fn parse_args(args: &[String]) -> Result<JqOptions, String> {
    let mut opts = JqOptions {
        filter: String::new(),
        raw_output: false,
        raw_input: false,
        slurp: false,
        compact: false,
        null_input: false,
        exit_status: false,
        join_output: false,
        args: Vec::new(),
        jsonargs: Vec::new(),
        input_paths: Vec::new(),
    };

    let mut filter_set = false;
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        if arg == "--" {
            let remaining = &args[i + 1..];
            if !filter_set {
                let Some(filter) = remaining.first() else {
                    return Err("no filter provided".to_string());
                };
                opts.filter = filter.clone();
                filter_set = true;
                opts.input_paths.extend(remaining.iter().skip(1).cloned());
            } else {
                opts.input_paths.extend(remaining.iter().cloned());
            }
            break;
        }

        if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            // Handle combined short flags like -rn, -crS, etc.
            let flags = &arg[1..];
            for c in flags.chars() {
                match c {
                    'r' => opts.raw_output = true,
                    'R' => opts.raw_input = true,
                    's' => opts.slurp = true,
                    'c' => opts.compact = true,
                    'n' => opts.null_input = true,
                    'e' => opts.exit_status = true,
                    'j' => opts.join_output = true,
                    'S' => {} // sort keys - jaq sorts by default
                    _ => return Err(format!("unknown option: -{}", c)),
                }
            }
        } else if arg == "--raw-output" || arg == "--raw-output0" {
            opts.raw_output = true;
        } else if arg == "--raw-input" {
            opts.raw_input = true;
        } else if arg == "--slurp" {
            opts.slurp = true;
        } else if arg == "--compact-output" {
            opts.compact = true;
        } else if arg == "--null-input" {
            opts.null_input = true;
        } else if arg == "--exit-status" {
            opts.exit_status = true;
        } else if arg == "--join-output" {
            opts.join_output = true;
        } else if arg == "--arg" {
            if i + 2 >= args.len() {
                return Err("--arg requires name and value".to_string());
            }
            let name = args[i + 1].clone();
            let value = args[i + 2].clone();
            opts.args.push((name, value));
            i += 2;
        } else if arg == "--argjson" {
            if i + 2 >= args.len() {
                return Err("--argjson requires name and value".to_string());
            }
            let name = args[i + 1].clone();
            let json_str = &args[i + 2];
            let value: serde_json::Value = serde_json::from_str(json_str)
                .map_err(|e| format!("invalid JSON for --argjson: {}", e))?;
            opts.jsonargs.push((name, Val::from(value)));
            i += 2;
        } else if !filter_set {
            opts.filter = arg.clone();
            filter_set = true;
        } else {
            opts.input_paths.push(arg.clone());
        }

        i += 1;
    }

    if !filter_set {
        return Err("no filter provided".to_string());
    }

    Ok(opts)
}

fn read_sources(opts: &JqOptions) -> Result<Vec<String>, String> {
    if opts.input_paths.is_empty() {
        let mut stdin_data = String::new();
        io::stdin()
            .take((MAX_INPUT_BYTES + 1) as u64)
            .read_to_string(&mut stdin_data)
            .map_err(|e| format!("failed to read stdin: {}", e))?;
        if stdin_data.len() > MAX_INPUT_BYTES {
            return Err("stdin exceeds size limit".to_string());
        }
        return Ok(vec![stdin_data]);
    }

    let mut total = 0usize;
    let mut sources = Vec::new();
    for path in &opts.input_paths {
        let mut data = String::new();
        if path == "-" {
            io::stdin()
                .take((MAX_INPUT_BYTES.saturating_sub(total) + 1) as u64)
                .read_to_string(&mut data)
                .map_err(|e| format!("failed to read stdin: {}", e))?;
        } else {
            FsFile::open(path)
                .map_err(|e| format!("failed to open {}: {}", path, e))?
                .take((MAX_INPUT_BYTES.saturating_sub(total) + 1) as u64)
                .read_to_string(&mut data)
                .map_err(|e| format!("failed to read {}: {}", path, e))?;
        }
        total = total
            .checked_add(data.len())
            .ok_or_else(|| "input exceeds size limit".to_string())?;
        if total > MAX_INPUT_BYTES {
            return Err("input exceeds size limit".to_string());
        }
        sources.push(data);
    }
    Ok(sources)
}

fn read_inputs(opts: &JqOptions) -> Result<Vec<Val>, String> {
    if opts.null_input {
        return Ok(vec![Val::from(serde_json::Value::Null)]);
    }

    let sources = read_sources(opts)?;

    if opts.raw_input {
        let raw_data = sources.concat();
        if opts.slurp {
            Ok(vec![Val::from(serde_json::Value::String(raw_data))])
        } else {
            let mut lines = Vec::new();
            for line in raw_data.lines() {
                push_input_value(
                    &mut lines,
                    Val::from(serde_json::Value::String(line.to_string())),
                )?;
            }
            Ok(lines)
        }
    } else {
        let mut values = Vec::new();
        for source in &sources {
            let trimmed = source.trim();
            if trimmed.is_empty() {
                continue;
            }

            let decoder =
                serde_json::Deserializer::from_str(trimmed).into_iter::<serde_json::Value>();
            for result in decoder {
                let value = result.map_err(|e| format!("parse error: {}", e))?;
                push_input_value(&mut values, value)?;
            }
        }

        if values.is_empty() && opts.input_paths.is_empty() {
            values.push(serde_json::Value::Null);
        }

        if opts.slurp {
            Ok(vec![Val::from(serde_json::Value::Array(values))])
        } else {
            Ok(values.into_iter().map(Val::from).collect())
        }
    }
}

fn push_input_value<T>(values: &mut Vec<T>, value: T) -> Result<(), String> {
    if values.len() >= MAX_INPUT_VALUES {
        return Err("too many input values".to_string());
    }
    values.push(value);
    Ok(())
}

/// Format a jaq Val as a string for output.
fn format_output(val: &Val, opts: &JqOptions) -> Result<String, String> {
    let compact_str = format!("{}", val);

    // For raw output, unquote strings
    if opts.raw_output {
        if compact_str.starts_with('"') && compact_str.ends_with('"') && compact_str.len() >= 2 {
            if let Ok(unescaped) = serde_json::from_str::<String>(&compact_str) {
                return Ok(unescaped);
            }
        }
    }

    if opts.compact {
        Ok(compact_str)
    } else {
        // Pretty print via serde_json
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&compact_str) {
            serde_json::to_string_pretty(&v).map_err(|e| format!("output format error: {}", e))
        } else {
            Ok(compact_str)
        }
    }
}

fn run_jq(args: &[String]) -> Result<i32, String> {
    if matches!(args, [arg] if arg == "--version" || arg == "-V") {
        println!("jq-jaq-{}", env!("CARGO_PKG_VERSION"));
        return Ok(0);
    }

    let opts = parse_args(args)?;
    let inputs = read_inputs(&opts)?;

    // Set up variable bindings for --arg and --argjson
    let mut var_vals: Vec<Val> = Vec::new();

    for (_name, value) in &opts.args {
        var_vals.push(Val::from(serde_json::Value::String(value.clone())));
    }
    for (_name, value) in &opts.jsonargs {
        var_vals.push(value.clone());
    }

    // Load and compile filter
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

    let empty_inputs = RcIter::new(core::iter::empty());
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut had_false_or_null = false;

    for input in inputs {
        let ctx = Ctx::new(var_vals.iter().cloned(), &empty_inputs);
        let results = filter.run((ctx, input));

        for result in results {
            match result {
                Ok(val) => {
                    let s = format_output(&val, &opts)?;

                    // Track for --exit-status
                    let compact = format!("{}", val);
                    if compact == "null" || compact == "false" {
                        had_false_or_null = true;
                    }

                    if opts.join_output {
                        write!(out, "{}", s)
                            .map_err(|e| format!("failed to write stdout: {}", e))?;
                    } else {
                        writeln!(out, "{}", s)
                            .map_err(|e| format!("failed to write stdout: {}", e))?;
                    }
                }
                Err(e) => {
                    eprintln!("jq: error: {}", e);
                    return Ok(5);
                }
            }
        }
    }

    if opts.exit_status && had_false_or_null {
        return Ok(1);
    }

    out.flush()
        .map_err(|e| format!("failed to flush stdout: {}", e))?;

    Ok(0)
}
