use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("XML: {0}")]
    Xml(String),
    #[error("JSON: {0}")]
    Json(String),
    #[error("Excel: {0}")]
    Excel(String),
    #[error("{0}")]
    Format(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{prefix}: {0}", prefix = crate::i18n::t("not found"))]
    NotFound(String),
    #[error("{prefix}: {0}", prefix = crate::i18n::t("unsupported"))]
    Unsupported(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io { path: path.into(), source }
    }
}

/// Reads a file, attaching the path to the error.
pub fn read_file(path: &std::path::Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| Error::io(path, e))
}

/// Writes a file atomically-ish (temp file + rename), creating parent directories.
pub fn write_file(path: &std::path::Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".susi-tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, data).map_err(|e| Error::io(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::io(path, e)
    })
}
