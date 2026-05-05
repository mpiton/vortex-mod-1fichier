//! 1fichier free-mode HTML page parsing + HTTP envelope.
//!
//! The free flow on 1fichier looks like:
//!  1. `GET https://1fichier.com/?<id>` → landing page with filename,
//!     human-readable size, a 60-second wait countdown and (sometimes)
//!     a captcha challenge.
//!  2. After the wait elapses, the page submits a form back to itself;
//!     1fichier returns the direct CDN URL — but only if the captcha
//!     was solved.
//!
//! The plugin host owns the wait scheduling (task 39 — `WaitManager`)
//! and will own the captcha solver pipeline (task 43+). The plugin only
//! produces *the metadata observed on the landing page* so the host can
//! drive the rest of the flow. Resolving the direct URL synchronously
//! from `resolve_stream_url` is therefore unsupported in free mode for
//! now and surfaces as [`PluginError::CaptchaRequired`].

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::PluginError;
use crate::USER_AGENT;

// ── HTTP envelope ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct HttpRequest {
    pub method: String,
    pub url: String,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: String,
}

/// Cap landing pages so a malicious server can't force megabyte-scale
/// regex scans. Real 1fichier landing pages weigh well under 100 KB.
pub(crate) const MAX_BODY_BYTES: usize = 1024 * 1024;

impl HttpResponse {
    pub fn into_success_body(self) -> Result<String, PluginError> {
        if (200..300).contains(&self.status) {
            if self.body.len() > MAX_BODY_BYTES {
                return Err(PluginError::HttpStatus {
                    status: self.status,
                    message: format!("body exceeds {MAX_BODY_BYTES} bytes"),
                });
            }
            Ok(self.body)
        } else if self.status == 404 || self.status == 410 {
            Err(PluginError::Offline(format!("status {}", self.status)))
        } else {
            Err(PluginError::HttpStatus {
                status: self.status,
                message: truncate(&self.body, 256),
            })
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut cut = max;
        while !s.is_char_boundary(cut) && cut > 0 {
            cut -= 1;
        }
        format!("{}…", &s[..cut])
    }
}

pub fn parse_http_response(raw: &str) -> Result<HttpResponse, PluginError> {
    serde_json::from_str(raw).map_err(|e| PluginError::HostResponse(e.to_string()))
}

// ── Page request ─────────────────────────────────────────────────────────────

pub fn build_landing_request(url: &str) -> Result<String, PluginError> {
    let mut headers = HashMap::new();
    headers.insert("User-Agent".to_string(), USER_AGENT.to_string());
    headers.insert(
        "Accept".to_string(),
        "text/html,application/xhtml+xml".to_string(),
    );
    let req = HttpRequest {
        method: "GET".into(),
        url: url.to_string(),
        headers,
        body: None,
    };
    serde_json::to_string(&req).map_err(PluginError::SerdeJson)
}

// ── Parsed landing page ──────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
pub struct ParsedLanding {
    pub filename: Option<String>,
    pub size_bytes: Option<u64>,
    /// Wait time advertised by the countdown form (seconds). 1fichier
    /// pegs free downloads at 60s but the value is read from the page so
    /// we follow whatever the host wants today (e.g. degraded slots).
    pub wait_seconds: Option<u32>,
    /// True when the page ships a captcha challenge — currently inferred
    /// from the presence of a Google `g-recaptcha` element.
    pub requires_captcha: bool,
}

pub fn parse_landing_page(html: &str) -> Result<ParsedLanding, PluginError> {
    if is_offline_page(html) {
        return Err(PluginError::Offline(
            "landing page reports the file is offline".into(),
        ));
    }
    let filename = locate_filename(html);
    let size_bytes = locate_size_text(html).and_then(|s| parse_size_bytes(&s));
    let wait_seconds = locate_wait_seconds(html);
    let requires_captcha = detect_captcha(html);
    if filename.is_none() && size_bytes.is_none() && wait_seconds.is_none() {
        return Err(PluginError::NoDirectLink);
    }
    Ok(ParsedLanding {
        filename,
        size_bytes,
        wait_seconds,
        requires_captcha,
    })
}

fn is_offline_page(html: &str) -> bool {
    offline_regex().is_match(html)
}

fn offline_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"(?i)the file you are trying to access is no longer available|the requested file could not be found|file not found|fichier introuvable",
        )
        .expect("offline_regex: compile-time constant must compile")
    })
}

fn locate_filename(html: &str) -> Option<String> {
    capture(html, filename_regex())
}

fn locate_size_text(html: &str) -> Option<String> {
    capture(html, size_text_regex())
}

fn locate_wait_seconds(html: &str) -> Option<u32> {
    wait_regex()
        .captures(html)?
        .iter()
        .skip(1)
        .flatten()
        .find_map(|m| m.as_str().parse().ok())
}

fn detect_captcha(html: &str) -> bool {
    captcha_regex().is_match(html)
}

fn capture(html: &str, re: &Regex) -> Option<String> {
    re.captures(html)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

fn filename_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?is)Filename\s*:\s*</th>\s*<td>([^<]+)</td>"#)
            .expect("filename_regex: compile-time constant must compile")
    })
}

fn size_text_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?is)Size\s*:\s*</th>\s*<td>([^<]+)</td>"#)
            .expect("size_text_regex: compile-time constant must compile")
    })
}

fn wait_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // Three alternations cover the form-data attribute, the visible
        // countdown span, and the JS `var c = …` fallback that 1fichier
        // ships on degraded slots.
        Regex::new(
            r#"(?is)(?:data-wait\s*=\s*"(\d+)"|class="countdown"[^>]*>(\d+)</span>|var\s+c\s*=\s*(\d+))"#,
        )
        .expect("wait_regex: compile-time constant must compile")
    })
}

fn captcha_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?i)g-recaptcha|h-captcha|class\s*=\s*"captcha""#)
            .expect("captcha_regex: compile-time constant must compile")
    })
}

// ── Size parsing ─────────────────────────────────────────────────────────────

pub fn parse_size_bytes(text: &str) -> Option<u64> {
    let re = size_value_regex();
    let caps = re.captures(text)?;
    let value: f64 = caps.get(1)?.as_str().parse().ok()?;
    let unit = caps.get(2)?.as_str();
    let multiplier: f64 = if unit.eq_ignore_ascii_case("B") {
        1.0
    } else if unit.eq_ignore_ascii_case("KB") {
        1024.0
    } else if unit.eq_ignore_ascii_case("MB") {
        1024.0 * 1024.0
    } else if unit.eq_ignore_ascii_case("GB") {
        1024.0 * 1024.0 * 1024.0
    } else if unit.eq_ignore_ascii_case("TB") {
        1024.0 * 1024.0 * 1024.0 * 1024.0
    } else {
        return None;
    };
    let bytes = (value * multiplier).round();
    if bytes.is_finite() && bytes >= 0.0 {
        Some(bytes as u64)
    } else {
        None
    }
}

fn size_value_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)([0-9]+(?:\.[0-9]+)?)\s*(B|KB|MB|GB|TB)\b")
            .expect("size_value_regex: compile-time constant must compile")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── HTTP envelope ───────────────────────────────────────────────────────

    #[test]
    fn parse_http_response_round_trips_success() {
        let raw = r#"{"status":200,"headers":{},"body":"ok"}"#;
        let resp = parse_http_response(raw).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, "ok");
    }

    #[test]
    fn into_success_body_passes_2xx() {
        let resp = HttpResponse {
            status: 200,
            headers: HashMap::new(),
            body: "<html>".into(),
        };
        assert_eq!(resp.into_success_body().unwrap(), "<html>");
    }

    #[test]
    fn into_success_body_maps_404_to_offline() {
        let resp = HttpResponse {
            status: 404,
            headers: HashMap::new(),
            body: String::new(),
        };
        let err = resp.into_success_body().unwrap_err();
        assert!(matches!(err, PluginError::Offline(_)));
    }

    #[test]
    fn into_success_body_rejects_oversized_2xx_payload() {
        let resp = HttpResponse {
            status: 200,
            headers: HashMap::new(),
            body: "x".repeat(MAX_BODY_BYTES + 1),
        };
        let err = resp.into_success_body().unwrap_err();
        assert!(matches!(err, PluginError::HttpStatus { status: 200, .. }));
    }

    // ── Page request ────────────────────────────────────────────────────────

    #[test]
    fn build_landing_request_emits_get_with_user_agent() {
        let json = build_landing_request("https://1fichier.com/?abc123def").unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["method"], "GET");
        assert_eq!(v["url"], "https://1fichier.com/?abc123def");
        assert!(
            v["headers"]
                .as_object()
                .and_then(|h| h.get("User-Agent"))
                .is_some(),
            "request must carry a User-Agent so 1fichier returns the full landing page"
        );
    }

    // ── Landing parser ──────────────────────────────────────────────────────

    #[test]
    fn parse_landing_page_extracts_filename_size_wait_no_captcha() {
        let html = r#"
            <html><body>
              <table><tr>
                <th class="normal">Filename :</th><td>archive.zip</td>
                <th class="normal">Size :</th><td>1.50 MB</td>
              </tr></table>
              <form data-wait="60"><span class="countdown">60</span></form>
            </body></html>
        "#;
        let parsed = parse_landing_page(html).unwrap();
        assert_eq!(parsed.filename.as_deref(), Some("archive.zip"));
        assert_eq!(parsed.size_bytes, Some(1_572_864));
        assert_eq!(parsed.wait_seconds, Some(60));
        assert!(!parsed.requires_captcha);
    }

    #[test]
    fn parse_landing_page_detects_recaptcha() {
        let html = r#"
            <html><body>
              <th class="normal">Filename :</th><td>doc.pdf</td>
              <th class="normal">Size :</th><td>50 KB</td>
              <div class="g-recaptcha" data-sitekey="abc"></div>
            </body></html>
        "#;
        let parsed = parse_landing_page(html).unwrap();
        assert!(parsed.requires_captcha);
    }

    #[test]
    fn parse_landing_page_offline_returns_offline_error() {
        let html = r#"<html><body>The requested file could not be found.</body></html>"#;
        let err = parse_landing_page(html).unwrap_err();
        assert!(matches!(err, PluginError::Offline(_)));
    }

    #[test]
    fn parse_landing_page_no_metadata_returns_no_direct_link() {
        let html = "<html><body>Welcome to 1fichier.</body></html>";
        let err = parse_landing_page(html).unwrap_err();
        assert!(matches!(err, PluginError::NoDirectLink));
    }

    #[test]
    fn parse_landing_page_size_can_be_missing() {
        let html = r#"
            <th class="normal">Filename :</th><td>z.bin</td>
            <span class="countdown">60</span>
        "#;
        let parsed = parse_landing_page(html).unwrap();
        assert_eq!(parsed.filename.as_deref(), Some("z.bin"));
        assert_eq!(parsed.size_bytes, None);
        assert_eq!(parsed.wait_seconds, Some(60));
    }

    #[test]
    fn parse_landing_page_wait_can_come_from_var_c() {
        let html = r#"
            <th class="normal">Filename :</th><td>data.bin</td>
            <th class="normal">Size :</th><td>10 KB</td>
            <script>var c = 30; setTimeout(go, c*1000);</script>
        "#;
        let parsed = parse_landing_page(html).unwrap();
        assert_eq!(parsed.wait_seconds, Some(30));
    }

    // ── Size parsing ────────────────────────────────────────────────────────

    #[test]
    fn parse_size_bytes_recognises_units() {
        assert_eq!(parse_size_bytes("1.50 MB"), Some(1_572_864));
        assert_eq!(parse_size_bytes("10.00 KB"), Some(10_240));
        assert_eq!(parse_size_bytes("1 GB"), Some(1_073_741_824));
        assert_eq!(parse_size_bytes("123 B"), Some(123));
    }

    #[test]
    fn parse_size_bytes_rejects_garbage() {
        assert_eq!(parse_size_bytes("nope"), None);
    }
}
