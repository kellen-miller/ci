//! Custom CI operations shared by the command-line entry point and actions.

pub mod branches;
pub mod changed_paths;
pub mod command;
pub mod github;
pub mod github_output;
pub mod helm;
pub mod local_files;
pub mod node;
pub mod packaging;
pub mod release;
pub mod repository;
pub mod rust_check;
pub mod rust_lexer;
pub mod rust_sources;
mod source_identity;
pub mod tooling;
pub mod workflow_commands;
pub mod workflow_security;
