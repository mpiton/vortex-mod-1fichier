//! Vortex 1fichier WASM plugin.
//!
//! Implements the plugin contract used by the Vortex plugin host:
//! - `can_handle(url)` → `"true"` / `"false"`
//! - `supports_playlist(url)` → always `"false"` (single-file hoster)
//! - `extract_links(url)` → JSON metadata for the resolved file
//! - `resolve_stream_url(input)` → direct CDN URL (premium only in v1)
//!
//! The plugin operates in two modes:
//!  - **Premium** — requires an API key stored in the host's credential
//!    store under the plugin's own service name. Hits the JSON API
//!    (`api.1fichier.com/v1/download/get_token.cgi`) and returns the
//!    one-shot direct CDN URL. Skips wait + captcha.
//!  - **Free** — falls back to the public landing page when no
//!    credential is present (or the credential is rejected by the API).
//!    The landing page parser surfaces `wait_seconds` and a captcha
//!    flag as metadata; the host owns the wait scheduling (task 39 —
//!    `WaitManager`) and the captcha solver pipeline (task 43+).
//!    `resolve_stream_url` for free mode therefore surfaces
//!    [`PluginError::CaptchaRequired`] until the captcha pipeline
//!    ships.
//!
//! Network access is delegated to the host via `http_request`. Parsing
//! is pure (`free_mode.rs` / `premium_mode.rs`) so it can be exercised
//! natively without WASM.

pub mod error;
pub mod free_mode;
pub mod premium_mode;
pub mod url_matcher;

#[cfg(target_family = "wasm")]
mod plugin_api;

use serde::Serialize;

use crate::error::PluginError;
use crate::free_mode::ParsedLanding;
use crate::premium_mode::PremiumToken;
use crate::url_matcher::UrlKind;

pub(crate) const USER_AGENT: &str =
    "Mozilla/5.0 (Vortex/1.0; +https://vortex-app.com) 1fichierPlugin/1.0";

// ── IPC DTOs ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ExtractLinksResponse {
    pub kind: &'static str,
    pub mode: &'static str,
    pub files: Vec<FileLink>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct FileLink {
    pub id: String,
    pub url: String,
    pub filename: Option<String>,
    pub size_bytes: Option<u64>,
    /// Direct CDN URL — populated only in premium mode. For free mode
    /// the host has to walk the wait + captcha pipeline first.
    pub direct_url: Option<String>,
    pub resumable: bool,
    /// Number of seconds the host has to park the download in the
    /// `Waiting` state. `None` for premium.
    pub wait_seconds: Option<u32>,
    /// Whether 1fichier rendered a CAPTCHA challenge alongside the
    /// countdown. Always `false` for premium.
    pub requires_captcha: bool,
    /// Optional traffic monitoring info, populated only when the
    /// premium API includes it in the response.
    pub traffic_used_bytes: Option<u64>,
    pub traffic_total_bytes: Option<u64>,
}

// ── Routing helpers ──────────────────────────────────────────────────────────

pub fn handle_can_handle(url: &str) -> String {
    matches!(url_matcher::classify_url(url), UrlKind::File).to_string()
}

pub fn handle_supports_playlist(_url: &str) -> String {
    false.to_string()
}

pub fn ensure_file_url(url: &str) -> Result<(), PluginError> {
    match url_matcher::classify_url(url) {
        UrlKind::File => Ok(()),
        UrlKind::Unknown => Err(PluginError::UnsupportedUrl(url.to_string())),
    }
}

// ── Response builders ────────────────────────────────────────────────────────

pub fn build_free_response(source_url: &str, parsed: ParsedLanding) -> ExtractLinksResponse {
    let id = url_matcher::extract_file_id(source_url).unwrap_or_default();
    let link = FileLink {
        id,
        url: source_url.to_string(),
        filename: parsed.filename,
        size_bytes: parsed.size_bytes,
        direct_url: None,
        resumable: true,
        wait_seconds: parsed.wait_seconds,
        requires_captcha: parsed.requires_captcha,
        traffic_used_bytes: None,
        traffic_total_bytes: None,
    };
    ExtractLinksResponse {
        kind: "file",
        mode: "free",
        files: vec![link],
    }
}

pub fn build_premium_response(
    source_url: &str,
    landing_hint: Option<ParsedLanding>,
    token: PremiumToken,
) -> ExtractLinksResponse {
    let id = url_matcher::extract_file_id(source_url).unwrap_or_default();
    let (filename, size_bytes) = match landing_hint {
        Some(p) => (p.filename, p.size_bytes),
        None => (None, None),
    };
    let link = FileLink {
        id,
        url: source_url.to_string(),
        filename,
        size_bytes,
        direct_url: Some(token.direct_url),
        resumable: true,
        wait_seconds: None,
        requires_captcha: false,
        traffic_used_bytes: token.traffic_used_bytes,
        traffic_total_bytes: token.traffic_total_bytes,
    };
    ExtractLinksResponse {
        kind: "file",
        mode: "premium",
        files: vec![link],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_landing() -> ParsedLanding {
        ParsedLanding {
            filename: Some("archive.zip".into()),
            size_bytes: Some(2048),
            wait_seconds: Some(60),
            requires_captcha: true,
        }
    }

    fn sample_token() -> PremiumToken {
        PremiumToken {
            direct_url: "https://download.1fichier.com/key/archive.zip".into(),
            traffic_used_bytes: Some(1024),
            traffic_total_bytes: Some(1_000_000),
        }
    }

    // ── Routing ─────────────────────────────────────────────────────────────

    #[test]
    fn can_handle_recognises_file_url() {
        assert_eq!(
            handle_can_handle("https://1fichier.com/?abc123def456"),
            "true"
        );
    }

    #[test]
    fn can_handle_rejects_unrelated_url() {
        assert_eq!(handle_can_handle("https://example.com/?abc"), "false");
    }

    #[test]
    fn supports_playlist_always_false() {
        assert_eq!(
            handle_supports_playlist("https://1fichier.com/?abc123def"),
            "false"
        );
    }

    #[test]
    fn ensure_file_url_accepts_file() {
        ensure_file_url("https://1fichier.com/?abc123def").unwrap();
    }

    #[test]
    fn ensure_file_url_rejects_unknown() {
        let err = ensure_file_url("https://example.com/").unwrap_err();
        assert!(matches!(err, PluginError::UnsupportedUrl(_)));
    }

    // ── Free response builder ───────────────────────────────────────────────

    #[test]
    fn build_free_response_propagates_landing_metadata() {
        let r = build_free_response("https://1fichier.com/?abc123def456", sample_landing());
        assert_eq!(r.kind, "file");
        assert_eq!(r.mode, "free");
        assert_eq!(r.files.len(), 1);
        let f = &r.files[0];
        assert_eq!(f.id, "abc123def456");
        assert_eq!(f.filename.as_deref(), Some("archive.zip"));
        assert_eq!(f.size_bytes, Some(2048));
        assert_eq!(f.direct_url, None, "free mode never produces direct URL");
        assert_eq!(f.wait_seconds, Some(60));
        assert!(f.requires_captcha);
        assert!(f.resumable);
    }

    #[test]
    fn free_response_serialises_camel_kind_and_mode() {
        let r = build_free_response("https://1fichier.com/?abc123def456", sample_landing());
        let json = serde_json::to_string(&r).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["kind"], "file");
        assert_eq!(v["mode"], "free");
        assert_eq!(v["files"][0]["wait_seconds"], 60);
        assert_eq!(v["files"][0]["requires_captcha"], true);
    }

    // ── Premium response builder ────────────────────────────────────────────

    #[test]
    fn build_premium_response_carries_direct_url_and_traffic() {
        let r = build_premium_response(
            "https://1fichier.com/?abc123def456",
            Some(sample_landing()),
            sample_token(),
        );
        assert_eq!(r.mode, "premium");
        let f = &r.files[0];
        assert_eq!(f.filename.as_deref(), Some("archive.zip"));
        assert_eq!(f.size_bytes, Some(2048));
        assert_eq!(
            f.direct_url.as_deref(),
            Some("https://download.1fichier.com/key/archive.zip")
        );
        assert_eq!(f.wait_seconds, None);
        assert!(!f.requires_captcha);
        assert_eq!(f.traffic_used_bytes, Some(1024));
        assert_eq!(f.traffic_total_bytes, Some(1_000_000));
    }

    #[test]
    fn build_premium_response_without_landing_hint_omits_filename_and_size() {
        let r = build_premium_response("https://1fichier.com/?abc123def456", None, sample_token());
        let f = &r.files[0];
        assert_eq!(f.filename, None);
        assert_eq!(f.size_bytes, None);
    }
}
