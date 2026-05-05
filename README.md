# vortex-mod-1fichier

1fichier hoster WASM plugin for [Vortex](https://github.com/mpiton/vortex).
Resolves `https://1fichier.com/?<id>` URLs in two modes:

- **Premium** — when a 1fichier API key is configured in the host
  credential store, the plugin hits
  `https://api.1fichier.com/v1/download/get_token.cgi` with bearer auth
  and returns the one-shot direct CDN URL. Skips the wait + captcha.
- **Free** — falls back to scraping the public landing page when no
  credential is configured (or when the API rejects the configured
  key as invalid / expired). The parser surfaces `wait_seconds` and a
  `requires_captcha` flag as metadata; the host owns the wait
  scheduling (`WaitManager`, task 39) and the captcha solver pipeline
  (task 43+). `resolve_stream_url` for free mode therefore surfaces
  `PluginError::CaptchaRequired` until the captcha pipeline ships.

## Features

- File-id extraction from `1fichier.com/?<id>` query
- Free landing-page parser:
  - Filename + size (B/KB/MB/GB/TB) from the metadata `<th>/<td>` table
  - Wait countdown from `data-wait="…"`, `class="countdown"`, or `var c = …`
  - Captcha detection (`g-recaptcha`, `h-captcha`)
  - Offline-page detection (file removed / not found)
- Premium API parser:
  - `status: "OK"` → direct URL + optional `traffic_used` /
    `traffic_total`
  - `status: "KO"` → typed errors (`InvalidCredentials`,
    `AccountExpired`, `RateLimited`, `Offline`, `InvalidApiResponse`)
- Auto-fallback to free mode when the API key is rejected
- Resume support advertised on every link

## Build

```bash
# Native unit + integration tests (no WASM)
cargo test

# Lint
cargo clippy --all-targets -- -D warnings

# WASM artefact
rustup target add wasm32-wasip1   # one-time
cargo build --target wasm32-wasip1 --release
# target/wasm32-wasip1/release/vortex_mod_1fichier.wasm
```

## Install (development)

```bash
PLUGIN_NAME="vortex-mod-1fichier"
PLUGIN_DIR="$HOME/.local/share/dev.vortex.app/plugins/$PLUGIN_NAME"

mkdir -p "$PLUGIN_DIR"
cp target/wasm32-wasip1/release/vortex_mod_1fichier.wasm "$PLUGIN_DIR/plugin.wasm"
cp plugin.toml "$PLUGIN_DIR/plugin.toml"
```

Vortex hot-reloads the plugin via the file watcher.

## Configure premium

Store your 1fichier API key under the plugin's own service name:

- **Service**: `vortex-mod-1fichier`
- **Username**: anything (unused)
- **Password**: your API key

The plugin reads it through the host's `get_credential` host function
(scoped — only `vortex-mod-1fichier` can read this slot).

## License

GPL-3.0 — see [`LICENSE`](./LICENSE).
