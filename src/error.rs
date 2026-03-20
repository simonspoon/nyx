use std::path::PathBuf;

#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("JSON parse error in {path}: {source}")]
    JsonParse {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[error("Invalid duration format: {0} (expected e.g. 7d, 24h, 30d)")]
    InvalidDuration(String),

    #[error("No index found at {0} — run `nyx index` first")]
    NoIndex(PathBuf),

    #[error("Conversation not found: {0}")]
    ConversationNotFound(String),

    #[error(
        "Schema version mismatch: expected {expected}, found {found}. Run `nyx index --rebuild` to recreate the index."
    )]
    SchemaMismatch { expected: i32, found: i32 },

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
