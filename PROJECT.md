# Project guide

## Purpose

Ny Gemini ACP provides two ways to use Gemini through its **unofficial web interface**: an ACP agent for editor integrations and a local HTTP API adapter. Both depend on a browser-session cookie rather than an official Google API credential, so upstream changes can break authentication or protocol behavior.

## Entry points

- **`gemini-acp`** — speaks ACP over standard input/output and connects the editor to the agent runtime.
- **`gemini-web2api`** — exposes local HTTP endpoints such as `/v1/models`, `/v1/chat/completions`, `/v1/responses`, and Google-style `/v1beta/models` routes.

These are two user-facing adapters, not two separate implementations of the Gemini client.

## Architecture: four crates

| Crate | Owns | Must not own |
|---|---|---|
| `acp-adaptor` | ACP transport, protocol mapping, editor-facing prompts and notifications | Gemini HTTP details or the canonical session store |
| `agent-runtime` | Provider-neutral execution loop, session state/persistence, event flow, turn lifecycle and cancellation | ACP wire-format handling or Gemini-specific requests |
| `llm-provider` | Gemini web client, authentication, model requests, streaming/images, and the `gemini-web2api` binary | Editor protocol state or built-in tool execution |
| `tools-provider` | Built-in tools, MCP integration, tool permissions and tool execution | Session ownership or model request implementation |

Keep these boundaries unless a demonstrated dependency problem justifies a change. A small number of explicit layers is preferable to either duplicated implementations or a broad rewrite.

## Main request flows

**ACP:** editor ⇄ `acp-adaptor` ⇄ `agent-runtime` ⇄ `llm-provider` and `tools-provider`. The runtime owns session state and turn orchestration; the adaptor translates protocol events and requests.

**HTTP:** API client ⇄ `gemini-web2api` ⇄ the shared Gemini client in `llm-provider`. Do not build a second Gemini transport for this server.

## Configuration and credentials

For the ACP agent, copy `vendor/cookie.example.json` to `vendor/cookie.json`, then set the `SAPISID` value locally. The default cookie path can be overridden with `GEMINI_ACP_COOKIES`. ACP-specific options include `GEMINI_ACP_MODEL`, `GEMINI_ACP_AUTH_USER`, and `GEMINI_ACP_PROXY`.

The HTTP server uses the same authentication cookie and supports configuration through its `config.json` and `GEMINI_WEB2API_*` options. Its default local endpoint is `127.0.0.1:8081`. If `GEMINI_WEB2API_API_KEYS` is configured, send a key through `Authorization: Bearer …`, `x-api-key`, or `x-goog-api-key`; query-string keys are rejected. Browser origins are denied by default; configure `GEMINI_WEB2API_CORS_ORIGINS` as a comma-separated list of exact allowed origins when browser access is required. Keep the service bound to loopback unless API authentication and network exposure are configured deliberately.

**Never commit a real cookie, token, or generated local configuration containing credentials.** The cookie is equivalent to a browser-session credential and must be treated as a secret.

## Important limitation: shell-tool isolation

The shell tool uses a heuristic command/argument filter. It is **not an operating-system sandbox** and does not confine a process running with the user's privileges. Treat it as a risk-reduction layer, not a security boundary; do not expose it as safe execution for untrusted input. A true OS-level confinement mechanism remains a separate security improvement.

## Rules for keeping the project coherent

1. Put each responsibility in its owning crate; do not duplicate provider clients, session stores, permission decisions, or error classification.
2. Keep errors typed across crate boundaries. Convert them to protocol-specific messages only at the outer adapter.
3. Preserve cancellation from the runtime through in-flight tool/model work. Do not add a second lifecycle or permission state machine without proving the existing one insufficient.
4. Add regression tests with fixes, especially for persistence, cancellation, permissions, shell-filter bypasses, streaming errors and HTTP error handling.
5. Keep documentation intentionally small: `README.md` is the user entry point; this file is the architecture/configuration reference. Keep implementation details next to the code and use tests as executable specifications.

## Validation

Run from the repository root before merging:

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The CI workflow runs these same checks on pushes to `main`/ `dev` and on pull requests. Passing them is a regression baseline, not proof that every external Gemini behavior works; live integration still depends on a valid private cookie and the upstream web interface.
