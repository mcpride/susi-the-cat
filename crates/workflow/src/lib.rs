//! Git workflow for translators ("Alternative A"): a developer prepares a git working copy and
//! `.susi/workflow.toml`; translators then use three plain-language actions:
//!
//! * **Get translation project** ([`Workflow::fetch`]): backs up local changes and resets the
//!   working copy to the latest version of the default branch.
//! * **Save changes** ([`Workflow::save`]): commits the project files (`.slp`, sources, localized
//!   files) to a personal branch (created from a pattern when on the default branch) and pushes it.
//! * **Prepare publication** ([`Workflow::publish`]): creates or updates a merge request with reviewers.
//!
//! Git runs as a command line tool (`git` must be installed); the server is accessed through the
//! [`Forge`] trait (GitLab now, GitHub later). Configuration files never contain secrets, only
//! references to them (see [`secret::TokenRef`]).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod config;
pub mod flow;
pub mod forge;
pub mod git;
pub mod gitlab;
pub mod paths;
pub mod secret;

pub use config::Config;
pub use flow::{FetchOptions, FetchReport, PublishOptions, PublishReport, SaveOptions, SaveReport, State, Status, StatusOptions, Workflow};
pub use forge::{Forge, MergeRequest, MergeRequestDraft, MergeRequestState, User};

use susi_core::i18n::t;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("git {command}: {message}")]
    Git { command: String, message: String },
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Forge(String),
    #[error("{0}")]
    Workflow(String),
    #[error("{prefix} {0}", prefix = t("The project is not in a git working copy configured for the translation workflow:"))]
    NotConfigured(String),
    #[error(transparent)]
    Core(#[from] susi_core::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
