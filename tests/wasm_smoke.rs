//! Real ABI smoke tests for every runtime export of the release WASM artifact.
//! Premium credential and HTTP fixtures exercise extraction and resolution
//! through the same Extism host-function boundary as Vortex.
//!
//! Requires the WASM artifact at
//! `target/wasm32-wasip1/release/vortex_mod_1fichier.wasm`. To produce
//! it:
//!
//! ```bash
//! cargo build --target wasm32-wasip1 --release
//! ```

use std::path::PathBuf;

use extism::{Function, UserData, Val, PTR};
use serde_json::{json, Value};

const WASM_REL_PATH: &str = "target/wasm32-wasip1/release/vortex_mod_1fichier.wasm";
const FILE_URL: &str = "https://1fichier.com/?abc123def456";
const DIRECT_URL: &str = "https://download.1fichier.com/archive.zip";

fn wasm_path() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(WASM_REL_PATH);
    assert!(
        path.is_file(),
        "missing release WASM artifact at {}; run `cargo build --target wasm32-wasip1 --release` first",
        path.display()
    );
    path
}

fn stub_http_request() -> Function {
    Function::new(
        "http_request",
        [PTR],
        [PTR],
        UserData::<()>::default(),
        |plugin, _inputs, outputs, _user_data: UserData<()>| {
            let api_body = json!({ "status": "OK", "url": DIRECT_URL }).to_string();
            let response = json!({ "status": 200, "headers": {}, "body": api_body }).to_string();
            let handle = plugin.memory_new(&response)?;
            outputs[0] = Val::I64(handle.offset() as i64);
            Ok(())
        },
    )
}

fn stub_http_invalid_credential() -> Function {
    Function::new(
        "http_request",
        [PTR],
        [PTR],
        UserData::<()>::default(),
        |plugin, _inputs, outputs, _user_data: UserData<()>| {
            let api_body = json!({ "status": "KO", "message": "Invalid key" }).to_string();
            let response = json!({ "status": 200, "headers": {}, "body": api_body }).to_string();
            let handle = plugin.memory_new(&response)?;
            outputs[0] = Val::I64(handle.offset() as i64);
            Ok(())
        },
    )
}

fn stub_get_credential() -> Function {
    Function::new(
        "get_credential",
        [PTR],
        [PTR],
        UserData::<()>::default(),
        |plugin, _inputs, outputs, _user_data: UserData<()>| {
            let credential = json!({ "username": "", "password": "test-api-key" }).to_string();
            let handle = plugin.memory_new(&credential)?;
            outputs[0] = Val::I64(handle.offset() as i64);
            Ok(())
        },
    )
}

fn load_plugin(path: &PathBuf) -> extism::Plugin {
    load_plugin_with_http(path, stub_http_request())
}

fn load_plugin_with_http(path: &PathBuf, http_request: Function) -> extism::Plugin {
    let manifest = extism::Manifest::new([extism::Wasm::file(path)]);
    extism::Plugin::new(&manifest, [http_request, stub_get_credential()], true).expect("load wasm")
}

macro_rules! require_wasm {
    () => {
        wasm_path()
    };
}

#[test]
fn wasm_can_handle_recognises_1fichier_url() {
    let path = require_wasm!();
    let mut plugin = load_plugin(&path);
    let result: String = plugin
        .call("can_handle", FILE_URL)
        .expect("can_handle call");
    assert_eq!(result.trim(), "true");
}

#[test]
fn wasm_can_handle_rejects_unrelated_url() {
    let path = require_wasm!();
    let mut plugin = load_plugin(&path);
    let result: String = plugin
        .call("can_handle", "https://example.com/file/abc")
        .expect("can_handle call");
    assert_eq!(result.trim(), "false");
}

#[test]
fn wasm_supports_playlist_always_false() {
    let path = require_wasm!();
    let mut plugin = load_plugin(&path);
    let result: String = plugin
        .call("supports_playlist", FILE_URL)
        .expect("supports_playlist call");
    assert_eq!(result.trim(), "false");
}

#[test]
fn wasm_extraction_and_resolution_exports_are_callable() {
    let path = require_wasm!();
    let mut plugin = load_plugin(&path);

    let links: String = plugin
        .call("extract_links", FILE_URL)
        .expect("extract_links call");
    let links: Value = serde_json::from_str(&links).expect("extract_links JSON");
    assert_eq!(links["kind"], "file");
    assert_eq!(links["mode"], "premium");
    assert_eq!(links["files"][0]["direct_url"], DIRECT_URL);

    let direct_url: String = plugin
        .call("resolve_stream_url", json!({ "url": FILE_URL }).to_string())
        .expect("resolve_stream_url call");
    assert_eq!(direct_url, DIRECT_URL);
}

#[test]
fn wasm_validate_account_reads_host_credential_and_calls_api() {
    let path = require_wasm!();
    let mut plugin = load_plugin(&path);

    let outcome: String = plugin
        .call("validate_account", "")
        .expect("validate_account call");
    let outcome: Value = serde_json::from_str(&outcome).expect("validation JSON");
    assert_eq!(outcome["valid"], true);
}

#[test]
fn wasm_premium_resolution_surfaces_invalid_credential_code() {
    let path = require_wasm!();
    let mut plugin = load_plugin_with_http(&path, stub_http_invalid_credential());

    let error = plugin
        .call::<_, String>("extract_links", FILE_URL)
        .expect_err("invalid selected credential must not silently fall back to free mode");
    assert!(error.to_string().contains("ACCOUNT_INVALID_CREDENTIALS"));
}
