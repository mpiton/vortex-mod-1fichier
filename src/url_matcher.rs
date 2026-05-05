//! 1fichier URL detection and parsing.
//!
//! Recognised shapes:
//! - File: `https://1fichier.com/?<id>` — id matches `[A-Za-z0-9]+`
//! - File: `https://www.1fichier.com/?<id>` — alias host
//!
//! Optional trailing `&` query parameters and fragments are tolerated
//! and stripped. Anything else falls through to [`UrlKind::Unknown`].

use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UrlKind {
    /// Single file link: `1fichier.com/?<id>`
    File,
    /// Anything else.
    Unknown,
}

pub fn classify_url(url: &str) -> UrlKind {
    if extract_file_id(url).is_some() {
        UrlKind::File
    } else {
        UrlKind::Unknown
    }
}

/// Extract the file id from a recognised file URL.
pub fn extract_file_id(url: &str) -> Option<String> {
    let (host, path_and_query) = validate_and_split(url)?;
    if !is_1fichier_host(host) {
        return None;
    }

    // 1fichier file URLs always carry an empty path and the id as the
    // first query token: `/?abcdef123456` or `/?abcdef123456&...`.
    let after_q = path_query_to_query(path_and_query)?;
    file_id_regex()
        .captures(after_q)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

fn is_1fichier_host(host: &str) -> bool {
    ["1fichier.com", "www.1fichier.com"]
        .iter()
        .any(|h| host.eq_ignore_ascii_case(h))
}

fn path_query_to_query(path_and_query: &str) -> Option<&str> {
    let no_frag = path_and_query.split('#').next().unwrap_or("");
    // The path must be `/` or empty; reject anything else (e.g. `/foo?...`)
    let (path, query) = no_frag.split_once('?')?;
    if !path.is_empty() && path != "/" {
        return None;
    }
    Some(query)
}

fn file_id_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // Capture the id token; it must be the first query token. Anything
        // after `&` is ignored. The id is alphanumeric and at least 6
        // chars long — short tokens are rejected so random fragments
        // can't masquerade as ids.
        Regex::new(r"^([A-Za-z0-9]{6,})(?:&|$)")
            .expect("file_id_regex: compile-time constant regex must compile")
    })
}

fn validate_and_split(url: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let (authority, path_and_query) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };
    let authority_no_user = authority.rsplit('@').next().unwrap_or(authority);
    let host = extract_host(authority_no_user)?;
    if host.is_empty() {
        return None;
    }
    Some((host, path_and_query))
}

fn extract_host(authority: &str) -> Option<&str> {
    if authority.is_empty() {
        return None;
    }
    if let Some(rest) = authority.strip_prefix('[') {
        let close = rest.find(']')?;
        return Some(&authority[..=close + 1]);
    }
    let host = authority.split(':').next().unwrap_or(authority);
    (!host.is_empty()).then_some(host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("https://1fichier.com/?abc123def456", UrlKind::File)]
    #[case("https://www.1fichier.com/?abc123def456", UrlKind::File)]
    #[case("http://1fichier.com/?abcdef", UrlKind::File)]
    #[case("https://1fichier.com/?abc123&af=1", UrlKind::File)]
    #[case("https://1fichier.com/?abc123#frag", UrlKind::File)]
    #[case("https://1fichier.com/?abc123/", UrlKind::Unknown)]
    #[case("https://1fichier.com/", UrlKind::Unknown)]
    #[case("https://1fichier.com/?short", UrlKind::Unknown)] // < 6 chars
    #[case("https://1fichier.com/path?abc123", UrlKind::Unknown)] // path not /
    #[case("https://example.com/?abc123def", UrlKind::Unknown)]
    #[case("ftp://1fichier.com/?abc123def", UrlKind::Unknown)]
    #[case("not a url", UrlKind::Unknown)]
    fn classify_url_recognises_shapes(#[case] url: &str, #[case] expected: UrlKind) {
        assert_eq!(classify_url(url), expected);
    }

    #[test]
    fn extract_file_id_returns_id_token() {
        assert_eq!(
            extract_file_id("https://1fichier.com/?abc123def456"),
            Some("abc123def456".into())
        );
    }

    #[test]
    fn extract_file_id_strips_extra_query_params() {
        assert_eq!(
            extract_file_id("https://1fichier.com/?abc123def456&af=99"),
            Some("abc123def456".into())
        );
    }

    #[test]
    fn extract_file_id_rejects_non_1fichier_host() {
        assert_eq!(extract_file_id("https://example.com/?abc123def"), None);
    }

    #[test]
    fn extract_file_id_rejects_path_segment() {
        assert_eq!(extract_file_id("https://1fichier.com/x?abc123def"), None);
    }

    #[test]
    fn classify_handles_uppercase_host() {
        assert_eq!(
            classify_url("https://1FICHIER.COM/?abc123def"),
            UrlKind::File
        );
    }
}
