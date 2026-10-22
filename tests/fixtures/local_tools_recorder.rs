use std::env;
use std::fs;
use std::process;

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if let Ok(log) = env::var("RECORDER_LOG") {
        let line = args.join("\t");
        let mut content = fs::read_to_string(&log).unwrap_or_default();
        content.push_str(&line);
        content.push('\n');
        if let Err(error) = fs::write(log, content) {
            eprintln!("cannot write recorder log: {error}");
            process::exit(1);
        }
    }
    if let Ok(output) = env::var("RECORDER_STDOUT") {
        print!("{output}");
    }
    process::exit(env::var("RECORDER_STATUS").ok().and_then(|value| value.parse().ok()).unwrap_or(0));
}
