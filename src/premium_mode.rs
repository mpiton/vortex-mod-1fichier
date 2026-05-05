//! 1fichier premium-mode JSON API helpers.
//!
//! Premium accounts hit `https://api.1fichier.com/v1/download/get_token.cgi`
//! with a Bearer API key. The endpoint returns:
//!  - `{"status": "OK", "url": "https://...", ...}` on success — the
//!    `url` is a one-shot direct CDN link.
//!  - `{"status": "KO", "message": "..."}` on error. We classify
//!    common messages to surface dedicated [`PluginError`] variants
//!    so the host can drive the auto-fallback to free mode.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::PluginError;

const ENDPOINT: &str = "https://api.1fichier.com/v1/download/get_token.cgi";
const USER_AGENT: &str = "Mozilla/5.0 (Vortex/1.0; +https://vortex-app.com) 1fichierPlugin/1.0";

/// Build the host `http_request` envelope for `get_token.cgi`.
///
/// The envelope is the same JSON shape used by the host's
/// `http_request` host function (see free_mode::HttpRequest).
pub fn build_get_token_request(file_url: &str, api_key: &str) -> Result<String, PluginError> {
    let mut headers = HashMap::new();
    headers.insert("Authorization".to_string(), format!("Bearer {api_key}"));
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    headers.insert("User-Agent".to_string(), USER_AGENT.to_string());

    #[derive(Serialize)]
    struct Body<'a> {
        url: &'a str,
    }
    let body_json = serde_json::to_string(&Body { url: file_url })?;

    // We re-use the free-mode envelope shape — same field names so the
    // host parses both with one schema.
    #[derive(Serialize)]
    struct Envelope {
        method: &'static str,
        url: &'static str,
        #[serde(skip_serializing_if = "HashMap::is_empty")]
        headers: HashMap<String, String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    }

    let env = Envelope {
        method: "POST",
        url: ENDPOINT,
        headers,
        body: Some(body_json),
    };
    serde_json::to_string(&env).map_err(PluginError::SerdeJson)
}

/// Parsed payload from a successful `get_token.cgi` response.
#[derive(Debug, PartialEq, Eq)]
pub struct PremiumToken {
    pub direct_url: String,
    /// Optional traffic monitoring info — not always present.
    pub traffic_used_bytes: Option<u64>,
    pub traffic_total_bytes: Option<u64>,
}

/// Parse the `body` returned by `get_token.cgi`.
///
/// The body is the raw HTTP body string (not the host envelope — the
/// caller is expected to have unwrapped that already via
/// `free_mode::HttpResponse::into_success_body`).
pub fn parse_get_token_response(body: &str) -> Result<PremiumToken, PluginError> {
    #[derive(Deserialize)]
    struct ApiResponse {
        #[serde(default)]
        status: String,
        #[serde(default)]
        url: String,
        #[serde(default)]
        message: String,
        // Traffic info — present on most premium tiers, missing on Pro.
        #[serde(default, alias = "traffic_used")]
        traffic_used: Option<u64>,
        #[serde(default, alias = "traffic_total")]
        traffic_total: Option<u64>,
    }

    let parsed: ApiResponse = serde_json::from_str(body)
        .map_err(|e| PluginError::InvalidApiResponse(format!("body is not valid JSON: {e}")))?;

    if parsed.status.eq_ignore_ascii_case("OK") {
        if parsed.url.is_empty() {
            return Err(PluginError::InvalidApiResponse(
                "OK response missing `url` field".into(),
            ));
        }
        return Ok(PremiumToken {
            direct_url: parsed.url,
            traffic_used_bytes: parsed.traffic_used,
            traffic_total_bytes: parsed.traffic_total,
        });
    }

    Err(classify_ko_message(&parsed.message))
}

/// Classify a KO message into a typed error.
fn classify_ko_message(msg: &str) -> PluginError {
    let lower = msg.to_ascii_lowercase();
    if lower.contains("invalid") && lower.contains("key") {
        return PluginError::InvalidCredentials;
    }
    if lower.contains("subscription") || lower.contains("expired") || lower.contains("not premium")
    {
        return PluginError::AccountExpired;
    }
    if lower.contains("flood") || lower.contains("rate") || lower.contains("too many") {
        return PluginError::RateLimited(msg.to_string());
    }
    if lower.contains("not found") || lower.contains("offline") {
        return PluginError::Offline(msg.to_string());
    }
    PluginError::InvalidApiResponse(msg.to_string())
}

/// Decoded credential payload returned by the host's `get_credential`
/// host function.
#[derive(Debug, Deserialize)]
pub struct CredentialResponse {
    #[serde(default)]
    pub username: String,
    pub password: String,
}

/// Parse the `get_credential` host-function output.
///
/// 1fichier identifies users by API key only — the host stores it in
/// the `password` slot. `username` is unused but accepted for forward
/// compatibility (e.g. multi-user accounts).
pub fn parse_credential_response(raw: &str) -> Result<String, PluginError> {
    let resp: CredentialResponse = serde_json::from_str(raw).map_err(|e| {
        PluginError::HostResponse(format!("get_credential returned malformed JSON: {e}"))
    })?;
    let key = resp.password.trim().to_string();
    if key.is_empty() {
        return Err(PluginError::HostResponse(
            "get_credential returned an empty API key".into(),
        ));
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Request builder ─────────────────────────────────────────────────────

    #[test]
    fn build_get_token_request_uses_post_with_bearer_auth() {
        let json = build_get_token_request("https://1fichier.com/?abc123def", "SECRETKEY").unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["method"], "POST");
        assert_eq!(v["url"], ENDPOINT);
        assert_eq!(
            v["headers"]["Authorization"].as_str(),
            Some("Bearer SECRETKEY"),
            "Bearer auth header is mandatory for the 1fichier API"
        );
        let body_str = v["body"].as_str().unwrap();
        let body: serde_json::Value = serde_json::from_str(body_str).unwrap();
        assert_eq!(body["url"], "https://1fichier.com/?abc123def");
    }

    // ── Response parser ─────────────────────────────────────────────────────

    #[test]
    fn parse_get_token_response_success_returns_direct_url() {
        let body = r#"{"status":"OK","url":"https://download.1fichier.com/file.bin"}"#;
        let token = parse_get_token_response(body).unwrap();
        assert_eq!(token.direct_url, "https://download.1fichier.com/file.bin");
        assert_eq!(token.traffic_used_bytes, None);
        assert_eq!(token.traffic_total_bytes, None);
    }

    #[test]
    fn parse_get_token_response_with_traffic_info() {
        let body =
            r#"{"status":"OK","url":"https://x","traffic_used":12345,"traffic_total":1000000000}"#;
        let token = parse_get_token_response(body).unwrap();
        assert_eq!(token.traffic_used_bytes, Some(12_345));
        assert_eq!(token.traffic_total_bytes, Some(1_000_000_000));
    }

    #[test]
    fn parse_get_token_response_invalid_key_classifies_credentials() {
        let body = r#"{"status":"KO","message":"Invalid key"}"#;
        let err = parse_get_token_response(body).unwrap_err();
        assert!(matches!(err, PluginError::InvalidCredentials));
    }

    #[test]
    fn parse_get_token_response_expired_classifies_account_expired() {
        let body = r#"{"status":"KO","message":"Subscription expired"}"#;
        let err = parse_get_token_response(body).unwrap_err();
        assert!(matches!(err, PluginError::AccountExpired));
    }

    #[test]
    fn parse_get_token_response_not_premium_classifies_account_expired() {
        let body = r#"{"status":"KO","message":"Account is not premium"}"#;
        let err = parse_get_token_response(body).unwrap_err();
        assert!(matches!(err, PluginError::AccountExpired));
    }

    #[test]
    fn parse_get_token_response_rate_limit_classifies_rate_limited() {
        let body = r#"{"status":"KO","message":"Flood detected: please wait"}"#;
        let err = parse_get_token_response(body).unwrap_err();
        assert!(matches!(err, PluginError::RateLimited(_)));
    }

    #[test]
    fn parse_get_token_response_offline_classifies_offline() {
        let body = r#"{"status":"KO","message":"Resource not found"}"#;
        let err = parse_get_token_response(body).unwrap_err();
        assert!(matches!(err, PluginError::Offline(_)));
    }

    #[test]
    fn parse_get_token_response_unknown_ko_falls_back_to_invalid_api() {
        let body = r#"{"status":"KO","message":"Something else"}"#;
        let err = parse_get_token_response(body).unwrap_err();
        assert!(matches!(err, PluginError::InvalidApiResponse(_)));
    }

    #[test]
    fn parse_get_token_response_missing_url_on_ok_is_invalid() {
        let body = r#"{"status":"OK"}"#;
        let err = parse_get_token_response(body).unwrap_err();
        assert!(matches!(err, PluginError::InvalidApiResponse(_)));
    }

    #[test]
    fn parse_get_token_response_garbage_is_invalid_api() {
        let body = "not json at all";
        let err = parse_get_token_response(body).unwrap_err();
        assert!(matches!(err, PluginError::InvalidApiResponse(_)));
    }

    // ── Credential parser ───────────────────────────────────────────────────

    #[test]
    fn parse_credential_response_extracts_password_as_api_key() {
        let raw = r#"{"username":"ignored","password":"mykey"}"#;
        assert_eq!(parse_credential_response(raw).unwrap(), "mykey");
    }

    #[test]
    fn parse_credential_response_trims_whitespace() {
        let raw = r#"{"username":"","password":"  trimmed  "}"#;
        assert_eq!(parse_credential_response(raw).unwrap(), "trimmed");
    }

    #[test]
    fn parse_credential_response_empty_key_errors() {
        let raw = r#"{"username":"x","password":""}"#;
        let err = parse_credential_response(raw).unwrap_err();
        assert!(matches!(err, PluginError::HostResponse(_)));
    }

    #[test]
    fn parse_credential_response_malformed_json_errors() {
        let err = parse_credential_response("not json").unwrap_err();
        assert!(matches!(err, PluginError::HostResponse(_)));
    }
}
