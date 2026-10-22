use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

fn main() {
    let executable = env::args_os().next().unwrap_or_default();
    if Path::new(&executable).file_stem().and_then(|name| name.to_str()) == Some("dirname") {
        let argument = env::args_os().nth(2).unwrap_or_default();
        let path = PathBuf::from(argument);
        if let Some(parent) = path.parent() {
            println!("{}", parent.display());
            return;
        }

        fail("dirname received a path without a parent");
    }

    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let expected = ["--fail", "--silent", "--show-error", "--location", "--retry", "3", "--proto", "=https", "--proto-redir", "=https", "--connect-timeout", "15", "--max-time", "120"];
    if arguments.len() != 17 || arguments[..14] != expected || arguments[15] != "-o" {
        fail("unexpected curl arguments");
    }

    let url = &arguments[14];
    let expected_base = match env::var("RUNTIME_EXPECTED_BASE") {
        Ok(value) => value,
        Err(error) => fail(&format!("expected release URL is missing: {error}")),
    };

    let asset_name = Path::new(url).file_name().and_then(|name| name.to_str());
    if !url.starts_with(&format!("{expected_base}/"))
        || !matches!(asset_name, Some(name) if name.starts_with("ci-") && (name.ends_with(".tar.gz") || name.ends_with(".tar.gz.sha256")))
    {
        fail(&format!("unexpected release URL: {url}"));
    }

    let output = PathBuf::from(&arguments[16]);
    let expected_directory = match env::var("RUNTIME_EXPECTED_OUTPUT") {
        Ok(value) => PathBuf::from(value),
        Err(error) => fail(&format!("expected output directory is missing: {error}")),
    };

    if output.parent() != Some(expected_directory.as_path())
        || output.file_name().and_then(|name| name.to_str()) != asset_name
    {
        fail(&format!("unexpected curl output path: {}", output.display()));
    }

    if let Ok(log) = env::var("RUNTIME_CURL_LOG") {
        let mut contents = fs::read_to_string(&log).unwrap_or_default();
        contents.push_str(url);
        contents.push('\t');
        contents.push_str(&output.display().to_string());
        contents.push('\n');
        if let Err(error) = fs::write(log, contents) {
            fail(&format!("cannot write curl log: {error}"));
        }
    }

    let assets = match env::var("RUNTIME_ASSETS") {
        Ok(value) => PathBuf::from(value),
        Err(error) => fail(&format!("fixture assets are missing: {error}")),
    };

    let source = assets.join(asset_name.unwrap_or_default());
    if let Err(error) = fs::copy(&source, &output) {
        eprintln!("cannot serve fixture asset {}: {error}", source.display());
        process::exit(22);
    }
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    process::exit(2);
}
