# Contributing to vortex-mod-1fichier

Thanks for taking the time to contribute! This crate is a WASM plugin for the
[Vortex download manager](https://github.com/mpiton/vortex). It targets
`wasm32-wasip1` via Extism PDK and is loaded by the Vortex host at runtime.

## How to Contribute

### Reporting Bugs

1. Check if the bug has already been reported in [Issues](https://github.com/mpiton/vortex-mod-1fichier/issues)
2. If not, create a new issue using the **Bug Report** template
3. Include the 1fichier URL shape (without sensitive parts), the mode (free /
   premium), the `vortex --version`, and the plugin version

### Suggesting Features

1. Check existing [Feature Requests](https://github.com/mpiton/vortex-mod-1fichier/issues?q=label%3Aenhancement)
2. Open a new issue using the **Feature Request** template
3. Describe the 1fichier URL shape or capability and the use case

### Pull Requests

1. Fork the repository
2. Create a feature branch (`git checkout -b feat/your-feature`)
3. Add a **failing test first** — see existing fixtures in
   `tests/fixtures/*.html` and `tests/fixtures/*.json`
4. Implement the change in `src/free_mode.rs` / `src/premium_mode.rs` /
   `src/url_matcher.rs` / `src/lib.rs`
5. Run `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`
6. Build the WASM artefact (`cargo build --target wasm32-wasip1 --release`)
   and check `wasm_smoke.rs` still passes
7. Commit using [Conventional Commits](https://www.conventionalcommits.org/)
8. Push to your fork and open a Pull Request

### Commit Message Format

```
<type>(<scope>): <description>

[optional body]
```

Types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `chore`, `ci`
Scopes: `free-mode`, `premium-mode`, `url-matcher`, `plugin-api`, `error`, `tests`, `build`

Example: `fix(premium-mode): classify 'token expired' as AccountExpired`

## Development Setup

```bash
# Prerequisites
rustup target add wasm32-wasip1

# Clone
git clone https://github.com/mpiton/vortex-mod-1fichier.git
cd vortex-mod-1fichier

# Native unit tests + parser fixtures + WASM smoke
cargo test

# Lint + format
cargo clippy --all-targets -- -D warnings
cargo fmt --check

# Build WASM release artefact
cargo build --target wasm32-wasip1 --release
# → target/wasm32-wasip1/release/vortex_mod_1fichier.wasm
```

## Adding a fixture

Real 1fichier pages and API responses drift over time. To add a new variant:

1. Save the relevant HTML snippet (free landing) to
   `tests/fixtures/free_<short_name>.html` — strip everything outside the
   `<table>` metadata block + form / countdown widget
2. Or save the relevant JSON response (premium API) to
   `tests/fixtures/premium_<short_name>.json` — keep only the documented
   fields (`status`, `url`, `message`, `traffic_*`)
3. Add a `#[rstest]` `#[case]` row to `parser_fixtures.rs`
4. Run `cargo test` — RED first, then make the parser pass without breaking
   any existing fixture

## Security

The premium-mode parser maps `{"status":"KO","message":...}` envelopes onto
typed `PluginError` variants. **Do not relax the classifier** — a downgrade
from `InvalidCredentials` to `InvalidApiResponse` would silently disable
the auto-fallback to free mode and leave users stuck with a dead key.

The free-mode parser caps the landing-page body at `MAX_BODY_BYTES`
(1 MiB) to bound the worst-case regex scan. **Do not raise this limit**
without measuring the impact on regex execution time.

For sensitive vulnerability reports, see [SECURITY.md](SECURITY.md).

## Code of Conduct

This project follows the upstream
[Vortex Code of Conduct](https://github.com/mpiton/vortex/blob/main/CODE_OF_CONDUCT.md).
By participating, you agree to uphold it.

## Questions

Open a [Discussion](https://github.com/mpiton/vortex-mod-1fichier/discussions)
or file an issue using the **Question** template.
