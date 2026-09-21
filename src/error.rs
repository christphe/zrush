use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, ZrushError>;

#[derive(Debug, thiserror::Error)]
pub enum ZrushError {
    #[error("{0}")]
    Msg(String),
    #[error("not a git repository: {0}")]
    NotARepo(PathBuf),
    #[error("git {args} failed: {stderr}")]
    Git { args: String, stderr: String },
    #[error("missing dependency: {0}")]
    MissingDependency(&'static str),
    #[error("config: {0}")]
    Config(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl ZrushError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Msg(s.into())
    }
}
