//! Cargo process fixture: record literal arguments and return configured status.

use std::env;
use std::fs::OpenOptions;
use std::io::Write;

fn main() {
    let arguments: Vec<String> = env::args().skip(1).collect();
    if arguments.first().is_some_and(|argument| argument == "metadata") {
        println!("{}", env::var("CARGO_METADATA").expect("fixture metadata"));
    }

    let configuration = env::var("CLIPPY_CONF_DIR").ok().map_or_else(|| "null".to_string(), |value| format!("{value:?}"));
    let mut log = OpenOptions::new().create(true).append(true).open(env::var("CARGO_LOG").expect("fixture log path")).expect("open fixture log");
    writeln!(log, "{{\"argv\":{arguments:?},\"cwd\":{:?},\"clippy_conf_dir\":{configuration}}}", env::current_dir().expect("fixture cwd").to_string_lossy()).expect("write fixture log");
    let failure = env::var("FAIL_CHECK").unwrap_or_default();
    let fail_argument = env::var("FAIL_ARG").unwrap_or_default();
    if arguments.first() == Some(&failure) && (fail_argument.is_empty() || arguments.contains(&fail_argument)) {
        std::process::exit(env::var("FAIL_CODE").unwrap_or_else(|_| "37".to_string()).parse().expect("fixture exit status"));
    }
}
