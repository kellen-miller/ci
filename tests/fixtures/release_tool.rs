//! Tripwire executable: validation tests must never launch npm or Node.

use std::env;
use std::fs::OpenOptions;
use std::io::Write;

fn main() {
    if let Some(path) = env::var_os("CI_RELEASE_TEST_PATHS_FILE") {
        std::fs::write(path, env::var("CI_RELEASE_PATHS").expect("normalized scope paths")).expect("write scope paths");
    }

    let mut log = OpenOptions::new().create(true).append(true).open(env::var_os("CI_RELEASE_TEST_TOOL_LOG").expect("tripwire log")).expect("open tripwire log");
    writeln!(log, "{:?}", env::args().collect::<Vec<_>>()).expect("write tripwire log");
    std::process::exit(91);
}
