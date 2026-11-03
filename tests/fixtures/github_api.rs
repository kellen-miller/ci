//! Isolated gh executable: unknown endpoints fail without contacting a network.

use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

fn main() {
    let arguments: Vec<_> = env::args().skip(1).collect();
    assert_eq!(arguments.first().map(String::as_str), Some("api"));
    let endpoint = &arguments[1];
    let method = arguments
        .windows(2)
        .find(|pair| pair[0] == "--method")
        .map_or("GET", |pair| pair[1].as_str());
    let mut body = String::new();
    if arguments.iter().any(|argument| argument == "--input") {
        io::stdin().read_to_string(&mut body).unwrap();
    }

    let directory = PathBuf::from(env::var_os("GH_FIXTURE_DIR").unwrap());
    let calls = directory.join("calls");
    let index = fs::read_dir(&calls).unwrap().count();
    fs::write(
        calls.join(format!("{index:04}")),
        format!("{method}\n{endpoint}\n{}\n{body}", arguments.join("\t")),
    )
    .unwrap();
    let key: String = endpoint.bytes().map(|byte| format!("{byte:02x}")).collect();
    let stem = directory.join(format!("{method}-{key}"));
    let count_path = stem.with_extension("count");
    let count: usize = fs::read_to_string(&count_path)
        .unwrap_or_else(|_| "0".into())
        .parse()
        .unwrap();
    fs::write(count_path, (count + 1).to_string()).unwrap();
    let response = stem.with_extension(count.to_string());
    let response = if response.exists() {
        response
    } else {
        stem.with_extension("0")
    };

    let bytes =
        fs::read(&response).unwrap_or_else(|_| panic!("unconfigured gh call: {method} {endpoint}"));
    println!("{}", String::from_utf8(bytes).unwrap());
    if let Ok(code) = fs::read_to_string(stem.with_extension("exit")) {
        std::process::exit(code.parse().unwrap());
    }

}
