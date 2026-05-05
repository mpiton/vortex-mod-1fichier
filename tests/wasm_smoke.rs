//! Smoke test: load the compiled `.wasm` via Extism and call the pure
//! `can_handle` / `supports_playlist` exports.
//!
//! `extract_links` and `resolve_stream_url` need a real `http_request`
//! and `get_credential` round-trip — exercised by the host's own
//! integration tests, not here. The stub host functions return JSON
//! envelopes shaped like the real ones so the WASM module loads
//! without unresolved imports.
//!
//! Skipped unless the WASM artifact is present at
//! `target/wasm32-wasip1/release/vortex_mod_1fichier.wasm`. To produce
//! it:
//!
//! ```bash
//! cargo build --target wasm32-wasip1 --release
//! ```

use std::path::PathBuf;

use extism::{Function, UserData, Val, PTR};

const WASM_REL_PATH: &str = "target/wasm32-wasip1/release/vortex_mod_1fichier.wasm";

fn wasm_path() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(WASM_REL_PATH);
    p.exists().then_some(p)
}

fn stub_http_request() -> Function {
    Function::new(
        "http_request",
        [PTR],
        [PTR],
        UserData::<()>::default(),
        |plugin, _inputs, outputs, _user_data: UserData<()>| {
            let body = r#"{"status":200,"headers":{},"body":""}"#;
            let handle = plugin.memory_new(body)?;
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
        |_plugin, _inputs, _outputs, _user_data: UserData<()>| {
            // Mimic the real host: signal "no credential" by returning
            // an error. The plugin will treat this as "use free mode".
            Err(extism::Error::msg("no credential configured"))
        },
    )
}

fn load_plugin(path: &PathBuf) -> extism::Plugin {
    let manifest = extism::Manifest::new([extism::Wasm::file(path)]);
    extism::Plugin::new(
        &manifest,
        [stub_http_request(), stub_get_credential()],
        true,
    )
    .expect("load wasm")
}

macro_rules! require_wasm {
    () => {
        match wasm_path() {
            Some(p) => p,
            None => {
                eprintln!(
                    "skipping: build with `cargo build --target wasm32-wasip1 --release` first"
                );
                return;
            }
        }
    };
}

#[test]
fn wasm_can_handle_recognises_1fichier_url() {
    let path = require_wasm!();
    let mut plugin = load_plugin(&path);
    let result: String = plugin
        .call("can_handle", "https://1fichier.com/?abc123def456")
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
        .call("supports_playlist", "https://1fichier.com/?abc123def456")
        .expect("supports_playlist call");
    assert_eq!(result.trim(), "false");
}
