//! Record child invocations without executing package managers or publishing tools.

use std::path::PathBuf;
use std::{env, fs, process};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory =
        PathBuf::from(env::var_os("WORKFLOW_RECORDER_DIR").ok_or("missing recorder directory")?);
    let count = fs::read_dir(&directory)?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "args")
        })
        .count();
    fs::write(
        directory.join(format!("{count:04}.args")),
        env::args().skip(1).collect::<Vec<_>>().join("\0"),
    )?;
    let keys = env::var("WORKFLOW_RECORDER_KEYS").unwrap_or_default();
    let values: Vec<_> = keys
        .split(',')
        .filter(|key| !key.is_empty())
        .map(|key| format!("{key}={}", env::var(key).unwrap_or_default()))
        .collect();
    fs::write(directory.join(format!("{count:04}.env")), values.join("\0"))?;
    if let Ok(output) = env::var("WORKFLOW_RECORDER_STDOUT") {
        print!("{output}");
    }

    process::exit(
        env::var("WORKFLOW_RECORDER_STATUS")
            .ok()
            .and_then(|status| status.parse().ok())
            .unwrap_or(0),
    );
}
