use thiserror::Error;

#[derive(Error, Debug)]
pub enum CcmError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Vector database error: {0}")]
    VectorStore(String),

    #[error("Parsing error: {0}")]
    Parsing(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Graph error: {0}")]
    Graph(String),

    #[error("Unknown error: {0}")]
    Unknown(#[from] anyhow::Error),
}
