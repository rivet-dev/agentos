fn main() {
    let args = std::env::args().collect::<Vec<String>>();
    let args = args
        .iter()
        .map(std::convert::AsRef::as_ref)
        .collect::<Vec<&str>>();
    std::process::exit(findutils::xargs::xargs_main(&args));
}
