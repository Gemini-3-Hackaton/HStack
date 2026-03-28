use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Network error: {0}")]
    Network(String),
    #[error("API error (status {status}): {body}")]
    Api { status: u16, body: String },
    #[error("Provider error: {0}")]
    Provider(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Header error: {0}")]
    Header(String),
    #[error("HStack world error: {0}")]
    World(String),
    #[error("Max iterations reached")]
    MaxIterations,
    #[error("Rate limit exceeded. Wait {wait_time}s")]
    RateLimitExceeded { wait_time: f64 },
    #[error("Redis error: {0}")]
    Redis(String),
    #[error("Control system denied action: {0}")]
    Denied(String),
}
