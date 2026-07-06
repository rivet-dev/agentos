fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    std::process::exit(uu_pwd::uumain(args.into_iter()));
}
