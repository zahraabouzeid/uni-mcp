use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

/// Messages are shown to the model, so they must never contain secrets.
#[derive(Debug, Error)]
pub enum Error {
    #[error("not logged in to {0}; run `unimcp login {0}`")]
    NotLoggedIn(String),
    #[error("{service} rejected the stored login; run `unimcp login {service}` again")]
    Auth { service: String },
    #[error("network error talking to {service}: {message}")]
    Network { service: String, message: String },
    #[error("unexpected response from {service}: {message}")]
    Protocol { service: String, message: String },
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("configuration error: {0}")]
    Config(String),
    #[error("secret storage error: {0}")]
    Secrets(String),
}
