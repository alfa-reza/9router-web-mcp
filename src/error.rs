use thiserror::Error;

fn format_response_too_large(limit: usize, observed: Option<usize>) -> String {
    match observed {
        Some(bytes) => format!(
            "Upstream response exceeded safe limit of {} bytes (observed {} bytes)",
            limit, bytes
        ),
        None => format!("Upstream response exceeded safe limit of {} bytes", limit),
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Invalid parameter: {0}")]
    InvalidInput(String),

    #[error("URL validation error: {0}")]
    InvalidUrl(String),

    #[error("Network error: failed to connect to 9Router at {0}")]
    NetworkUnreachable(String),

    #[error("Request to 9Router timed out after {0} seconds")]
    Timeout(u64),

    #[error("9Router rejected request (400): {0}")]
    BadRequest(String),

    #[error("Authentication failed (401). Verify 9Router API key.")]
    AuthenticationFailed,

    #[error("9Router rate limit exceeded (429): {0}")]
    RateLimited(String),

    #[error("9Router service unavailable (503): {0}")]
    ServiceUnavailable(String),

    #[error("9Router upstream error ({status}): {message}")]
    UpstreamServerError { status: u16, message: String },

    #[error("Failed to decode 9Router JSON response: {0}")]
    InvalidResponseJson(String),

    #[error("{}", format_response_too_large(*limit, *observed))]
    ResponseTooLarge {
        limit: usize,
        observed: Option<usize>,
    },
}

pub type Result<T> = std::result::Result<T, AppError>;
