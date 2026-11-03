//! Core library of the Susi the CAT: Sisulizer-compatible project model (`.slp`),
//! file formats, scan/build, translation memory, exchange formats and validation.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod error;
pub mod exchange;
pub mod filter;
pub mod formats;
pub mod i18n;
pub mod lang;
pub mod model;
pub mod ops;
pub mod pseudo;
pub mod settings;
pub mod slp;
pub mod stats;
pub mod status;
pub mod tm;
pub mod validate;
pub mod xml;

pub use error::{Error, Result};
pub use model::{Item, Language, Node, NodeRef, Project, Row, RowRef, Source, SourceKind, Translation, WriteMethod};
pub use status::{RowStatus, TranslationStatus};

/// Project file version written into `<document version="...">` (last Sisulizer 4 build).
pub const SLP_VERSION: &str = "4.0.374";
