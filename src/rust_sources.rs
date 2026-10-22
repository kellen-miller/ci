//! Discover handwritten package sources and enforce their shared code-line limit.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use walkdir::WalkDir;

use crate::command::capture_bytes;
use crate::rust_check::Metadata;
use crate::rust_lexer::code_lines;

const MAX_LINES: usize = 750;

// Cargo metadata may name a target directory or dependency not created yet.
fn resolve(path: &Path, strict: bool) -> Result<PathBuf> {
    if strict || path.exists() || path.is_symlink() {
        return dunce::canonicalize(path).with_context(|| format!("resolve {}", path.display()));
    }

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }

            Component::CurDir => {}
            other => normalized.push(other),
        }
    }

    let mut existing = normalized.as_path();
    let mut missing = Vec::new();
    while !existing.exists() {
        missing.push(
            existing
                .file_name()
                .context("missing path has no filename")?,
        );
        existing = existing.parent().context("missing path has no parent")?;
    }

    let mut resolved = dunce::canonicalize(existing)?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }

    Ok(resolved)
}

pub(crate) fn check(metadata: &Metadata, directory: &Path) -> Result<()> {
    let target = resolve(metadata.target_directory.as_deref().context("Invalid Cargo workspace metadata for Rust file-length check: target_directory is missing")?, false)?;
    let mut members = BTreeMap::new();
    let mut dependencies = BTreeSet::new();
    for package in &metadata.packages {
        let member = metadata.workspace_members.contains(&package.id);
        let manifest = resolve(package.manifest_path.as_deref().context("Invalid Cargo package metadata for Rust file-length check: manifest_path is missing")?, member)?;
        let root = manifest
            .parent()
            .context("Cargo manifest has no source directory")?
            .to_path_buf();
        if !member {
            dependencies.insert(root);
            continue;
        }

        ensure!(
            manifest.is_file()
                && manifest
                    .file_name()
                    .is_some_and(|name| name == "Cargo.toml"),
            "Invalid Cargo manifest path in workspace metadata: {}",
            manifest.display()
        );
        members.insert(root, package.generated());
    }

    for (root, generated) in &members {
        ensure!(
            *generated || root.is_dir(),
            "Workspace package source directory is missing: {}",
            root.display()
        );
    }

    let is_dependency = |path: &Path, owner: &Path| {
        dependencies
            .iter()
            .any(|root| root != owner && root.starts_with(owner) && path.starts_with(root))
    };

    let owner = |path: &Path| {
        members
            .keys()
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count())
    };

    let candidates = discover_sources(directory, &members, &dependencies, &target)?;

    let mut violations = Vec::new();
    for path in candidates {
        let Some(package_root) = owner(&path) else {
            continue;
        };

        if members[package_root] || path.starts_with(&target) || is_dependency(&path, package_root)
        {
            continue;
        }

        let rust_source = path.extension().is_some_and(|extension| extension == "rs");
        if !rust_source && !path.is_dir() {
            continue;
        }

        if path.is_symlink() {
            let resolved = resolve(&path, true).with_context(|| {
                format!("Cannot resolve Rust source symlink: {}", path.display())
            })?;
            if resolved.starts_with(&target) {
                continue;
            }

            ensure!(
                !is_dependency(&resolved, package_root),
                "Rust source symlink points into a dependency package: {} -> {}",
                path.display(),
                resolved.display()
            );
            let resolved_owner = owner(&resolved);
            ensure!(
                !resolved_owner.is_some_and(|root| members[root]),
                "Rust source symlink points into a generated workspace package: {} -> {}",
                path.display(),
                resolved.display()
            );
            if path.is_dir() {
                if resolved.starts_with(package_root) || resolved_owner.is_some() {
                    continue;
                }

                for entry in WalkDir::new(&resolved).follow_links(false) {
                    let context = format!(
                        "Cannot inspect Rust source symlink target: {}",
                        resolved.display()
                    );
                    let entry = entry.context(context)?;
                    ensure!(
                        !(!entry.file_type().is_dir()
                            && entry
                                .path()
                                .extension()
                                .is_some_and(|extension| extension == "rs")),
                        "Rust source directory symlink targets Rust files outside package ownership: {} -> {}",
                        path.display(),
                        resolved.display()
                    );
                }

                continue;
            }

            ensure!(
                resolved.starts_with(package_root) || resolved_owner.is_some(),
                "Rust source symlink escapes workspace package ownership: {} -> {}",
                path.display(),
                resolved.display()
            );
        }

        if !rust_source || (!path.is_file() && !path.is_symlink()) {
            continue;
        }

        let bytes = fs::read(&path).with_context(|| {
            format!(
                "Cannot read Rust source for file-length check: {}",
                path.display()
            )
        })?;
        let source = std::str::from_utf8(&bytes).with_context(|| {
            format!(
                "Cannot read Rust source for file-length check: {}",
                path.display()
            )
        })?;
        let lines = code_lines(source);
        if lines > MAX_LINES {
            violations.push((path, lines));
        }
    }

    for (path, lines) in &violations {
        eprintln!(
            "Rust file exceeds {MAX_LINES} code lines: {} ({lines} lines)",
            path.display()
        );
    }

    ensure!(
        violations.is_empty(),
        "Rust source file-length check failed"
    );
    Ok(())
}

fn discover_sources(
    directory: &Path,
    members: &BTreeMap<PathBuf, bool>,
    dependencies: &BTreeSet<PathBuf>,
    target: &Path,
) -> Result<BTreeSet<PathBuf>> {
    let git = Command::new("git")
        .current_dir(directory)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("locate Git checkout for Rust source discovery")?;
    let mut candidates = BTreeSet::new();
    if git.status.success() {
        let checkout = dunce::canonicalize(
            String::from_utf8(git.stdout)
                .context("Git checkout path is not UTF-8")?
                .trim_end_matches('\n'),
        )?;
        for (root, generated) in members {
            ensure!(
                *generated || root.starts_with(&checkout),
                "Workspace package source directory is outside the Git checkout: {}",
                root.display()
            );
        }

        let listed = capture_bytes(Command::new("git").arg("-C").arg(&checkout).args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ]))
        .context("Cannot list Rust source candidates from Git")?;
        for name in listed
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
        {
            candidates.insert(checkout.join(filename(name)?));
        }
    } else if git.status.code() == Some(128)
        && String::from_utf8_lossy(&git.stderr)
            .to_lowercase()
            .contains("not a git repository")
    {
        for (root, generated) in members {
            if *generated {
                continue;
            }

            let mut entries = WalkDir::new(root)
                .follow_links(false)
                .sort_by_file_name()
                .into_iter();
            while let Some(entry) = entries.next() {
                let entry = entry.with_context(|| {
                    format!("Cannot scan Rust source directory: {}", root.display())
                })?;
                let path = entry.path();
                if entry.file_type().is_symlink() {
                    candidates.insert(path.to_path_buf());
                    continue;
                }

                if !entry.file_type().is_dir() {
                    candidates.extend(
                        path.extension()
                            .filter(|extension| *extension == "rs")
                            .map(|_| path.to_path_buf()),
                    );
                    continue;
                }

                let resolved = resolve(path, false)?;
                if entry.depth() > 0
                    && (resolved.starts_with(target)
                        || dependencies.contains(&resolved)
                        || members.get(&resolved) == Some(&true))
                {
                    entries.skip_current_dir();
                }
            }
        }
    } else {
        bail!(
            "Cannot locate Git checkout for Rust source discovery: {}",
            String::from_utf8_lossy(&git.stderr)
        );
    }

    Ok(candidates)
}

#[cfg(unix)]
fn filename(bytes: &[u8]) -> Result<std::ffi::OsString> {
    use std::os::unix::ffi::OsStringExt;
    Ok(std::ffi::OsString::from_vec(bytes.to_vec()))
}

#[cfg(not(unix))]
fn filename(bytes: &[u8]) -> Result<std::ffi::OsString> {
    Ok(std::str::from_utf8(bytes)?.into())
}
