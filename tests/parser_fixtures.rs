//! Fixture-driven integration tests for the 1fichier free + premium
//! parsers.
//!
//! Each fixture in `tests/fixtures/*.html` mirrors a shape we have
//! observed on real 1fichier landing pages, and each `*.json` fixture
//! mirrors an `api.1fichier.com` response shape — see
//! `vortex/.claude/output/sprints/prd-v2-roadmap/tasks/38-plugin-1fichier.md`.

use std::fs;
use std::path::Path;

use rstest::rstest;
use vortex_mod_1fichier::error::PluginError;
use vortex_mod_1fichier::free_mode::parse_landing_page;
use vortex_mod_1fichier::premium_mode::parse_get_token_response;

const FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn load_fixture(name: &str) -> String {
    let path = Path::new(FIXTURES_DIR).join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

// ── Free landing page ───────────────────────────────────────────────────────

#[rstest]
#[case(
    "free_landing_basic.html",
    Some("archive.zip"),
    Some(1_572_864),
    Some(60),
    false
)]
#[case(
    "free_landing_with_captcha.html",
    Some("secret-document.pdf"),
    Some(843_213),
    Some(60),
    true
)]
#[case(
    "free_landing_var_c.html",
    Some("video.mp4"),
    Some(4_509_715_661),
    Some(30),
    false
)]
fn parses_free_landing(
    #[case] fixture: &str,
    #[case] expected_filename: Option<&str>,
    #[case] expected_size: Option<u64>,
    #[case] expected_wait: Option<u32>,
    #[case] expected_captcha: bool,
) {
    let html = load_fixture(fixture);
    let parsed =
        parse_landing_page(&html).unwrap_or_else(|e| panic!("fixture {fixture} should parse: {e}"));
    assert_eq!(parsed.filename.as_deref(), expected_filename);
    assert_eq!(parsed.size_bytes, expected_size);
    assert_eq!(parsed.wait_seconds, expected_wait);
    assert_eq!(parsed.requires_captcha, expected_captcha);
}

#[test]
fn free_landing_offline_fixture_yields_offline_error() {
    let html = load_fixture("free_landing_offline.html");
    let err = parse_landing_page(&html).unwrap_err();
    assert!(
        matches!(err, PluginError::Offline(_)),
        "offline fixture must surface PluginError::Offline, got {err:?}"
    );
}

#[test]
fn free_landing_no_metadata_fixture_yields_no_direct_link() {
    let html = load_fixture("free_landing_no_metadata.html");
    let err = parse_landing_page(&html).unwrap_err();
    assert!(
        matches!(err, PluginError::NoDirectLink),
        "no-metadata fixture must surface PluginError::NoDirectLink, got {err:?}"
    );
}

// ── Premium API responses ───────────────────────────────────────────────────

#[test]
fn premium_success_fixture_returns_direct_url() {
    let body = load_fixture("premium_success.json");
    let token = parse_get_token_response(&body).unwrap();
    assert_eq!(
        token.direct_url,
        "https://download.1fichier.com/abc123/file.zip"
    );
    assert_eq!(token.traffic_used_bytes, None);
}

#[test]
fn premium_success_with_traffic_fixture_propagates_traffic_info() {
    let body = load_fixture("premium_success_with_traffic.json");
    let token = parse_get_token_response(&body).unwrap();
    assert_eq!(token.traffic_used_bytes, Some(52_428_800));
    assert_eq!(token.traffic_total_bytes, Some(1_073_741_824_000));
}

#[rstest]
#[case("premium_invalid_key.json", "InvalidCredentials")]
#[case("premium_account_expired.json", "AccountExpired")]
#[case("premium_rate_limited.json", "RateLimited")]
fn premium_error_fixtures_classify(#[case] fixture: &str, #[case] expected: &str) {
    let body = load_fixture(fixture);
    let err = parse_get_token_response(&body).unwrap_err();
    let actual = match err {
        PluginError::InvalidCredentials => "InvalidCredentials",
        PluginError::AccountExpired => "AccountExpired",
        PluginError::RateLimited(_) => "RateLimited",
        PluginError::Offline(_) => "Offline",
        PluginError::InvalidApiResponse(_) => "InvalidApiResponse",
        other => panic!("unexpected error variant for {fixture}: {other:?}"),
    };
    assert_eq!(actual, expected, "fixture {fixture}");
}
