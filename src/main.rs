use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use personal_ci::{
    branches, changed_paths, command, github, helm, local_files, node, packaging, release,
    repository, rust_check, tooling, workflow_commands, workflow_security,
};

#[derive(Parser)]
#[command(name = "ci", version, about = "Reusable CI operations")]
struct Cli {
    #[command(subcommand)]
    command: Operation,
}

#[derive(Subcommand)]
enum Operation {
    Check,
    RustCheck,
    PinActions {
        commit: String,
    },
    Package {
        target: String,
        binary: PathBuf,
        output: PathBuf,
    },
    VerifyRuntime {
        target: String,
        assets: PathBuf,
    },
    ChangedPaths,
    WorkflowSecurity {
        #[arg(long, env = "SCAN_ROOT", default_value = ".")]
        root: PathBuf,
        #[arg(long, env = "ALLOW_TARGET", default_value = "false", action = clap::ArgAction::SetTrue)]
        allow_pull_request_target: bool,
        #[arg(long, env = "ALLOW_INHERIT", default_value = "false", action = clap::ArgAction::SetTrue)]
        allow_secret_inheritance: bool,
    },
    CleanGeneratedOutput,
    IsolateCredentials,
    GoConfig,
    GoConfigOutput,
    GoFormat,
    NodeDetect,
    NodeInstall,
    HelmLint,
    InstallKubeconform,
    CodeDuplication,
    DockerCache,
    PlaywrightVersion,
    PlaywrightInstall,
    PlaywrightDeps,
    GoTest,
    GradleCheck,
    NodeChecks,
    PagesBuild,
    RenovateChanges,
    RenovateValidate,
    PlatformMatrix,
    DigestArtifact,
    DockerManifest,
    HelmPackage,
    HelmPush,
    Provenance,
    SelectRunner,
    ResolveGithubRelease,
    TerraformPlanComment,
    DiscoverRepositories,
    ConfigureBot,
    StaleBranchCleanup,
    Release,
    #[command(hide = true)]
    ReleaseFilter,
}

fn execute(operation: Operation) -> Result<()> {
    if std::env::var("CI_VERIFY_SOURCE").is_ok_and(|value| value == "true") {
        repository::verify_runtime_source()?;
    }

    match operation {
        Operation::Check => repository::check(),
        Operation::RustCheck => rust_check::run(),
        Operation::PinActions { commit } => repository::pin_actions(&commit),
        Operation::Package {
            target,
            binary,
            output,
        } => packaging::package(&target, &binary, &output),
        Operation::VerifyRuntime { target, assets } => packaging::verify_runtime(&target, &assets),
        Operation::ChangedPaths => changed_paths::run(),
        Operation::WorkflowSecurity {
            root,
            allow_pull_request_target,
            allow_secret_inheritance,
        } => workflow_security::run(&root, allow_pull_request_target, allow_secret_inheritance),
        Operation::CleanGeneratedOutput => local_files::run("clean-generated-output"),
        Operation::IsolateCredentials => local_files::run("isolate-credentials"),
        Operation::GoConfig => local_files::run("go-config"),
        Operation::GoConfigOutput => local_files::run("go-config-output"),
        Operation::GoFormat => local_files::run("go-format"),
        Operation::NodeDetect => node::run("detect"),
        Operation::NodeInstall => node::run("install"),
        Operation::HelmLint => helm::run("lint"),
        Operation::InstallKubeconform => helm::run("install-kubeconform"),
        Operation::CodeDuplication => tooling::run("code-duplication"),
        Operation::DockerCache => tooling::run("docker-cache"),
        Operation::PlaywrightVersion => tooling::run("playwright-version"),
        Operation::PlaywrightInstall => workflow_commands::run("playwright-install"),
        Operation::PlaywrightDeps => workflow_commands::run("playwright-deps"),
        Operation::GoTest => tooling::run("go-test"),
        Operation::GradleCheck => tooling::run("gradle-check"),
        Operation::NodeChecks => workflow_commands::run("node-checks"),
        Operation::PagesBuild => workflow_commands::run("pages-build"),
        Operation::RenovateChanges => workflow_commands::run("renovate-changes"),
        Operation::RenovateValidate => workflow_commands::run("renovate-validate"),
        Operation::PlatformMatrix => workflow_commands::run("platform-matrix"),
        Operation::DigestArtifact => workflow_commands::run("digest-artifact"),
        Operation::DockerManifest => workflow_commands::run("docker-manifest"),
        Operation::HelmPackage => workflow_commands::run("helm-package"),
        Operation::HelmPush => workflow_commands::run("helm-push"),
        Operation::Provenance => workflow_commands::run("provenance"),
        Operation::SelectRunner => github::run("select-runner"),
        Operation::ResolveGithubRelease => github::run("resolve-github-release"),
        Operation::TerraformPlanComment => github::run("terraform-plan-comment"),
        Operation::DiscoverRepositories => github::run("discover-repositories"),
        Operation::ConfigureBot => github::run("configure-bot"),
        Operation::StaleBranchCleanup => branches::run(),
        Operation::Release => release::run(),
        Operation::ReleaseFilter => release::filter_commits(),
    }
}

fn main() {
    if let Err(error) = execute(Cli::parse().command) {
        eprintln!("{error:#}");
        std::process::exit(
            error
                .downcast_ref::<command::Failure>()
                .map_or(1, |failure| failure.0),
        );
    }
}
