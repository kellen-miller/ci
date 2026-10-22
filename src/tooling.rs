//! Small CI argument resolvers and tool runners.

use std::env;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};

use crate::{command, github_output};

/// Run one tool check or resolve a tool's action outputs.
///
/// # Errors
/// Returns an error for unsupported operations, invalid inputs, or failed child processes.
pub fn run(operation: &str) -> Result<()> {
    match operation {
        "code-duplication" => code_duplication(),
        "docker-cache" => docker_cache(),
        "playwright-version" => playwright_version(),
        "go-test" => go_test(),
        "gradle-check" => gradle_check(),
        _ => bail!("unknown tooling operation: {operation}"),
    }
}

fn code_duplication() -> Result<()> {
    let config = env::var("CPD_CONFIG").unwrap_or_else(|_| ".jscpd.json".to_owned());
    ensure!(
        Path::new(&config).is_file(),
        "Config file does not exist: {config}"
    );
    let mut args = vec![
        "--config".to_owned(),
        config.clone(),
        "--no-tips".to_owned(),
        "--no-colors".to_owned(),
    ];
    if let Ok(value) = env::var("CPD_THRESHOLD")
        && !value.is_empty()
    {
        args.extend(["--threshold".to_owned(), value]);
    }

    if let Ok(reporters) = env::var("CPD_REPORTERS")
        && !reporters.is_empty()
    {
        return command::run(
            Command::new("cpd")
                .args(args)
                .args(["--reporters", &reporters]),
        );
    }

    match command::run(Command::new("cpd").args(&args)) {
        Ok(()) => Ok(()),
        Err(error) => {
            let failure = error
                .downcast_ref::<command::Failure>()
                .map(|failure| failure.0)
                .unwrap_or(1);
            if let Err(report_error) = command::run(Command::new("cpd").args([
                "--config",
                &config,
                "--no-tips",
                "--no-colors",
                "--reporters",
                "ai",
                "--threshold",
                "100",
            ])) {
                eprintln!("CPD AI report failed: {report_error}");
            }

            Err(command::Failure(failure).into())
        }
    }
}

fn docker_cache() -> Result<()> {
    let ref_name = sanitize(&env::var("GITHUB_REF_NAME").unwrap_or_else(|_| "unknown".to_owned()));
    let event = env::var("GITHUB_EVENT_NAME").unwrap_or_else(|_| "unknown".to_owned());
    let trust_scope = match event.as_str() {
        "pull_request" | "pull_request_target" => {
            let head = env::var("GITHUB_HEAD_REF").unwrap_or_else(|_| ref_name.clone());
            format!("pr/{}", sanitize(&head))
        }

        "release" => format!("release/{ref_name}"),
        _ if env::var("GITHUB_REF_TYPE").unwrap_or_default() == "tag" => {
            format!("release/{ref_name}")
        }

        _ => format!("branch/{ref_name}"),
    };

    let repo = env::var("REPO")?;
    let scope = sanitize(&format!("{repo}/{trust_scope}"));
    github_output::append(
        "from",
        &env::var("CACHE_FROM")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("type=gha,scope={scope}")),
    )?;
    github_output::append(
        "to",
        &env::var("CACHE_TO")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("type=gha,scope={scope},mode=max")),
    )
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || "_.-".contains(character) {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn playwright_version() -> Result<()> {
    let mut version = env::var("PLAYWRIGHT_VERSION").unwrap_or_default();
    if version.is_empty() {
        let binary = Path::new("./node_modules/.bin/playwright");
        ensure!(
            is_executable(binary),
            "::error::Playwright binary not found at ./node_modules/.bin/playwright. Install @playwright/test or playwright as a dependency, or pass 'playwright-version' explicitly."
        );
        let output = command::capture(Command::new(binary).arg("--version"))?;
        version = output
            .split_whitespace()
            .nth(1)
            .context("Playwright version output is missing a version")?
            .to_owned();
    }

    let browsers = env::var("BROWSERS")
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");
    let browsers = if browsers.is_empty() {
        "all"
    } else {
        &browsers
    };

    let runner_os = env::var("RUNNER_OS")?;
    let runner_arch = env::var("RUNNER_ARCH")?;
    let cache_key = format!("playwright-{runner_os}-{runner_arch}-{version}-{browsers}");
    let home = env::var("HOME").unwrap_or_default();
    let cache_path = match runner_os.as_str() {
        "macOS" => Path::new(&home)
            .join("Library/Caches/ms-playwright")
            .to_string_lossy()
            .into_owned(),
        "Windows" => Path::new(&env::var("LOCALAPPDATA")?)
            .join("ms-playwright")
            .to_string_lossy()
            .into_owned(),
        "Linux" => {
            Path::new(&env::var("XDG_CACHE_HOME").unwrap_or_else(|_| format!("{home}/.cache")))
                .join("ms-playwright")
                .to_string_lossy()
                .into_owned()
        }

        _ => bail!("::error::Unsupported Playwright runner OS: {runner_os}"),
    };

    github_output::append("version", &version)?;
    github_output::append("cache-key", &cache_key)?;
    github_output::append("cache-path", &cache_path)?;
    println!("::notice::Playwright {version} (cache key: {cache_key})");
    Ok(())
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn go_test() -> Result<()> {
    let race = env::var("RACE").unwrap_or_default() == "true";
    let packages = env::var("TEST_PACKAGES").unwrap_or_else(|_| "./...".to_owned());
    let packages = packages.split_whitespace().collect::<Vec<_>>();
    ensure!(
        !packages.is_empty(),
        "test-packages must contain at least one package path"
    );
    let mut test = Command::new("go");
    test.arg("test").arg("-v");
    if race {
        test.arg("-race");
    }

    command::run(test.args(&packages))?;
    if env::var("VET").unwrap_or_default() == "true" {
        command::run(Command::new("go").args(["vet", "./..."]))?;
    }

    if let Ok(target) = env::var("BUILD_TARGET")
        && !target.is_empty()
    {
        command::run(Command::new("go").args(["build", &target]))?;
    }

    Ok(())
}

fn gradle_check() -> Result<()> {
    let args = env::var("GRADLE_ARGUMENTS")
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    ensure!(!args.is_empty(), "At least one Gradle argument is required");
    command::run(Command::new("./gradlew").args(args))
}
