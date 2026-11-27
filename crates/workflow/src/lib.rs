//! Git workflow for translators ("Alternative A"): a developer prepares a git working copy and
//! `.susi/workflow.toml`; translators then use three plain-language actions:
//!
//! * **Get translation project** ([`Workflow::fetch`]): backs up local changes and resets the
//!   working copy to the latest version of the default branch.
//! * **Save changes** ([`Workflow::save`]): commits the project files (`.slp`, sources, localized
//!   files) to a personal branch (created from a pattern when on the default branch) and pushes it.
//! * **Prepare publication** ([`Workflow::publish`]): creates or updates a merge request (GitLab)
//!   or pull request (GitHub) with reviewers.
//!
//! Git runs as a command line tool (`git` must be installed); the server is accessed through the
//! [`Forge`] trait ([`gitlab::GitLab`], [`github::GitHub`]). Configuration files never contain
//! secrets, only references to them (see [`secret::TokenRef`]).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod config;
pub mod flow;
pub mod forge;
pub mod git;
pub mod github;
pub mod gitlab;
pub mod http;
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
    /// `git` could not be started: not installed or not on the `PATH`.
    #[error("{message}", message = git_not_found())]
    GitNotFound,
    #[error(transparent)]
    Core(#[from] susi_core::Error),
}

/// What to do without git, in terms of the translator's platform.
fn git_not_found() -> &'static str {
    if cfg!(windows) {
        t(
            "git was not found, so the git workflow is not available. Install Git for Windows with the option \"Git from the command line and also from 3rd-party software\" and restart Susi.",
        )
    } else {
        t("git was not found, so the git workflow is not available. Install git (the git command must be on the PATH) and restart Susi.")
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
