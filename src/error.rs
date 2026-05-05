//! Plugin error type.

use thiserror::Error;

/// Errors raised by the 1fichier plugin.
#[derive(Debug, Error)]
pub enum PluginError {
    #[error("JSON error: {0}")]
    SerdeJson(#[from] serde_json::Error),

    #[error("1fichier HTTP returned status {status}: {message}")]
    HttpStatus { status: u16, message: String },

    #[error("host function response invalid: {0}")]
    HostResponse(String),

    #[error("URL is not a recognised 1fichier resource: {0}")]
    UnsupportedUrl(String),

    #[error("1fichier file is offline or removed: {0}")]
    Offline(String),

    #[error("no direct download link found in 1fichier free landing page")]
    NoDirectLink,

    #[error("1fichier API rejected the configured key as invalid")]
    InvalidCredentials,

    #[error("1fichier API reports the configured account is expired")]
    AccountExpired,

    #[error("1fichier API rate-limit exceeded: {0}")]
    RateLimited(String),

    #[error("1fichier API returned an unexpected payload: {0}")]
    InvalidApiResponse(String),

    #[error(
        "1fichier free mode requires a CAPTCHA solution; the captcha solver pipeline is not wired in v1"
    )]
    CaptchaRequired,
}
