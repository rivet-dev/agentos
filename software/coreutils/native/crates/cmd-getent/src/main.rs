use std::env;

const RECORD_BYTES: usize = 4096;
const MAX_RECORDS: u32 = 256;

fn record(call: impl FnOnce(&mut [u8]) -> Result<u32, u32>) -> Result<String, u32> {
    let mut buffer = vec![0; RECORD_BYTES];
    let len = call(&mut buffer)?;
    std::str::from_utf8(&buffer[..len as usize])
        .map(str::to_owned)
        .map_err(|_| wasi_ext::ERRNO_INVAL)
}

fn passwd(key: Option<&str>) -> Result<Vec<String>, u32> {
    if let Some(key) = key {
        return record(|buffer| match key.parse::<u32>() {
            Ok(uid) => wasi_ext::get_pwuid(uid, buffer),
            Err(_) => wasi_ext::get_pwnam(key, buffer),
        })
        .map(|entry| vec![entry]);
    }
    enumerate(|index, buffer| wasi_ext::get_pwent(index, buffer))
}

fn group(key: Option<&str>) -> Result<Vec<String>, u32> {
    if let Some(key) = key {
        return record(|buffer| match key.parse::<u32>() {
            Ok(gid) => wasi_ext::get_grgid(gid, buffer),
            Err(_) => wasi_ext::get_grnam(key, buffer),
        })
        .map(|entry| vec![entry]);
    }
    enumerate(|index, buffer| wasi_ext::get_grent(index, buffer))
}

fn enumerate(
    mut call: impl FnMut(u32, &mut [u8]) -> Result<u32, u32>,
) -> Result<Vec<String>, u32> {
    let mut entries = Vec::new();
    for index in 0..MAX_RECORDS {
        match record(|buffer| call(index, buffer)) {
            Ok(entry) => entries.push(entry),
            Err(wasi_ext::ERRNO_NOENT) => return Ok(entries),
            Err(errno) => return Err(errno),
        }
    }
    Err(wasi_ext::ERRNO_INVAL)
}

fn run() -> Result<Vec<String>, String> {
    let mut args = env::args().skip(1);
    let database = args
        .next()
        .ok_or_else(|| String::from("missing database"))?;
    let key = args.next();
    if args.next().is_some() {
        return Err(String::from("extra operand"));
    }
    let result = match database.as_str() {
        "passwd" => passwd(key.as_deref()),
        "group" => group(key.as_deref()),
        _ => return Err(format!("unsupported database {database}")),
    };
    result.map_err(|errno| format!("lookup failed with errno {errno}"))
}

fn main() {
    match run() {
        Ok(entries) => {
            for entry in entries {
                println!("{entry}");
            }
        }
        Err(error) => {
            eprintln!("getent: {error}");
            std::process::exit(2);
        }
    }
}
