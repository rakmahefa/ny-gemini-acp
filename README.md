# Ny Gemini ACP

Ny Gemini ACP connects editor agents to Gemini through the unofficial Gemini web interface. It is a Rust workspace with two entry points:

- **`gemini-acp`** — an Agent Client Protocol (ACP) agent for editors such as Zed.
- **`gemini-web2api`** — a local HTTP server that adapts Gemini to OpenAI-compatible and Google-style API routes.

This project is **not an official Google API client**. The web interface and its authentication requirements can change.

## Quick start

You need Rust and Cargo (the workspace declares Rust 1.82 or newer).

1. Create your local cookie file:

   ```sh
   cp vendor/cookie.example.json vendor/cookie.json
   ```

2. Replace `REPLACE_WITH_YOUR_SESSION_COOKIE` in `vendor/cookie.json` with the `SAPISID` value from your own signed-in Google browser session. Keep this file private; never commit it or share its contents.

3. Build both binaries:

   ```sh
   cargo build --release --bin gemini-acp --bin gemini-web2api
   ```

## Run

Start the HTTP API server:

```sh
cargo run --release --bin gemini-web2api
```

The default endpoint is `http://127.0.0.1:8081`. For the ACP agent, configure your editor to launch the built `target/release/gemini-acp` executable using ACP over stdio.

## Development

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

See [PROJECT.md](PROJECT.md) for the single source of truth on architecture, configuration, responsibilities, and limitations.

## License

MIT.
