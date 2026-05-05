//! WASM-only entry points: `#[plugin_fn]` exports + `#[host_fn]` imports.
//!
//! Mode selection rule:
//!  1. Try `get_credential("vortex-mod-1fichier")` — if a non-empty key
//!     is returned, attempt the premium API.
//!  2. If the API rejects the key with [`PluginError::InvalidCredentials`]
//!     or [`PluginError::AccountExpired`], fall back to free mode so
//!     the host can still surface the file.
//!  3. If no credential is configured, jump straight to free mode.
//!
//! `extract_links` always succeeds (free response is a valid metadata
//! payload). `resolve_stream_url` only succeeds in premium mode — free
//! mode surfaces [`PluginError::CaptchaRequired`] until the captcha
//! pipeline ships (task 43+).

use extism_pdk::*;

use crate::error::PluginError;
use crate::free_mode::{
    build_landing_request, parse_http_response, parse_landing_page, ParsedLanding,
};
use crate::premium_mode::{
    build_get_token_request, parse_credential_response, parse_get_token_response, PremiumToken,
};
use crate::{
    build_free_response, build_premium_response, ensure_file_url, handle_can_handle,
    handle_supports_playlist,
};

const SERVICE_NAME: &str = "vortex-mod-1fichier";

#[host_fn]
extern "ExtismHost" {
    fn http_request(req: String) -> String;
    fn get_credential(service: String) -> String;
}

#[plugin_fn]
pub fn can_handle(url: String) -> FnResult<String> {
    Ok(handle_can_handle(&url))
}

#[plugin_fn]
pub fn supports_playlist(url: String) -> FnResult<String> {
    Ok(handle_supports_playlist(&url))
}

#[plugin_fn]
pub fn extract_links(url: String) -> FnResult<String> {
    ensure_file_url(&url).map_err(error_to_fn_error)?;

    let response = match select_mode_and_resolve(&url)? {
        ResolvedMode::Premium {
            token,
            landing_hint,
        } => build_premium_response(&url, landing_hint, token),
        ResolvedMode::Free { parsed } => build_free_response(&url, parsed),
    };
    Ok(serde_json::to_string(&response)?)
}

/// Resolve the direct CDN URL for a 1fichier file.
///
/// Input JSON: `{ "url": "..." }` — extra fields are ignored. Premium
/// mode returns the API-provided one-shot URL; free mode is unsupported
/// in v1 and surfaces [`PluginError::CaptchaRequired`].
#[plugin_fn]
pub fn resolve_stream_url(input: String) -> FnResult<String> {
    #[derive(serde::Deserialize)]
    struct Input {
        url: String,
    }
    let params: Input =
        serde_json::from_str(&input).map_err(|e| error_to_fn_error(PluginError::SerdeJson(e)))?;
    ensure_file_url(&params.url).map_err(error_to_fn_error)?;

    match select_mode_and_resolve(&params.url)? {
        ResolvedMode::Premium { token, .. } => Ok(token.direct_url),
        ResolvedMode::Free { .. } => Err(error_to_fn_error(PluginError::CaptchaRequired)),
    }
}

// ── Mode selection ───────────────────────────────────────────────────────────

enum ResolvedMode {
    Premium {
        token: PremiumToken,
        landing_hint: Option<ParsedLanding>,
    },
    Free {
        parsed: ParsedLanding,
    },
}

fn select_mode_and_resolve(url: &str) -> FnResult<ResolvedMode> {
    match read_api_key() {
        Some(key) => match try_premium(url, &key) {
            Ok(token) => Ok(ResolvedMode::Premium {
                token,
                landing_hint: None,
            }),
            Err(e) if is_credential_failure(&e) => {
                let parsed = fetch_and_parse_landing(url)?;
                Ok(ResolvedMode::Free { parsed })
            }
            Err(other) => Err(error_to_fn_error(other)),
        },
        None => {
            let parsed = fetch_and_parse_landing(url)?;
            Ok(ResolvedMode::Free { parsed })
        }
    }
}

fn is_credential_failure(err: &PluginError) -> bool {
    matches!(
        err,
        PluginError::InvalidCredentials | PluginError::AccountExpired
    )
}

fn read_api_key() -> Option<String> {
    // SAFETY: host registers `get_credential` in the `ExtismHost` namespace before
    // any export is callable; ABI marshalled by `#[host_fn]`. See
    // src-tauri/src/adapters/driven/plugin/host_functions.rs.
    let raw = unsafe { get_credential(SERVICE_NAME.to_string()) }.ok()?;
    parse_credential_response(&raw).ok()
}

fn try_premium(url: &str, api_key: &str) -> Result<PremiumToken, PluginError> {
    let req = build_get_token_request(url, api_key)?;
    // SAFETY: see `fetch_and_parse_landing`.
    let raw = unsafe { http_request(req) }
        .map_err(|e| PluginError::HostResponse(format!("http_request: {e}")))?;
    let resp = parse_http_response(&raw)?;
    let body = resp.into_success_body()?;
    parse_get_token_response(&body)
}

fn fetch_and_parse_landing(url: &str) -> FnResult<ParsedLanding> {
    let req = build_landing_request(url).map_err(error_to_fn_error)?;
    // SAFETY: host registers `http_request` in the `ExtismHost` namespace before
    // any export is callable; capability is gated on the `http` declaration in
    // `plugin.toml`. See src-tauri/src/adapters/driven/plugin/host_functions.rs.
    let raw = unsafe { http_request(req)? };
    let resp = parse_http_response(&raw).map_err(error_to_fn_error)?;
    let body = resp.into_success_body().map_err(error_to_fn_error)?;
    parse_landing_page(&body).map_err(error_to_fn_error)
}

fn error_to_fn_error(err: PluginError) -> WithReturnCode<extism_pdk::Error> {
    extism_pdk::Error::msg(err.to_string()).into()
}
