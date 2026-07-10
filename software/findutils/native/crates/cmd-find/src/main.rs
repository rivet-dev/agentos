fn main() {
    uucore::panic::mute_sigpipe_panic();

    let args = std::env::args().collect::<Vec<String>>();
    let args = args
        .iter()
        .map(std::convert::AsRef::as_ref)
        .collect::<Vec<&str>>();
    let deps = findutils::find::StandardDependencies::new();
    std::process::exit(findutils::find::find_main(&args, &deps));
}
