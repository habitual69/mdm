use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("db: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("serialization: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("config: {0}")]
    Config(String),

    #[error("binary '{name}' not found ({hint})")]
    BinMissing { name: String, hint: String },

    #[error("engine: {0}")]
    Engine(String),
}

pub type Result<T> = std::result::Result<T, Error>;
