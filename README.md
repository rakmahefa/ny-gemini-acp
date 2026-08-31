# Ny Gemini ACP
[![Ask DeepWiki](https://devin.ai/assets/askdeepwiki.png)](https://deepwiki.com/rakmahefa/ny-gemini-acp)

This repository provides a high-performance AI agent backend for the Agent Client Protocol (ACP), specifically leveraging the unofficial Google Gemini web API. It is designed as a standalone agent for integration with clients like the Zed editor.

The project is built as a modular Rust workspace and also includes a separate OpenAI-compatible web API server (`gemini-web2api`) to expose the Gemini backend to a wider range of tools.

## Core Features

-   **ACP Agent (`gemini-acp`)**: A complete implementation of the Agent Client Protocol (v1/v2) for seamless integration with editors like Zed.
-   **Gemini Web API Integration**: Connects to powerful Gemini models via the web interface, supporting streaming generation and image uploads.
-   **Rich Built-in Tools**: A sandboxed suite of tools including `shell_exec`, `file_read`/`write`/`edit`, `search`, `glob`, and `web_search`.
-   **Extensible Tooling**: Supports external tools via the Model Context Protocol (MCP), allowing for project-specific extensions.
-   **Durable Sessions**: Robust session management with persistent history, configuration, forking, and automatic cleanup of stale processes.
-   **OpenAI-Compatible API (`gemini-web2api`)**: A standalone binary that emulates the OpenAI Chat Completions API, allowing you to use the Gemini web backend with tools expecting an OpenAI-compatible endpoint.

## Architecture

The repository is a Rust workspace composed of four main crates:

-   **`acp-adaptor`**: The public-facing component and main binary (`gemini-acp`). It handles all ACP communication and integrates the agent runtime with an ACP client.
-   **`agent-runtime`**: The protocol-agnostic core engine. It manages the agent's execution loop (model calls, tool execution), session state, and a semantic event bus that connects all components.
-   **`llm-provider`**: An implementation of the `LlmProvider` trait that handles communication with the unofficial Gemini Web API. It manages cookie-based authentication, request streaming, and image uploads. This crate also contains the `gemini-web2api` server.
-   **`tools-provider`**: The tool execution backend. It provides a secure sandbox for built-in tools and a client for the Model Context Protocol (MCP).

## Usage: ACP Agent for Zed

### 1. Configuration

The agent authenticates using your browser's session cookies for Google.

1.  Copy the example cookie file:
    ```sh
    cp vendor/cookie.example.json vendor/cookie.json
    ```
2.  Open `gemini.google.com` in your web browser.
3.  Open your browser's developer tools (usually with `F12` or `Ctrl+Shift+I`).
4.  Go to the "Application" (or "Storage") tab, find the cookies for `.google.com`, and locate the `SAPISID` cookie.
5.  Copy the value of the `SAPISID` cookie.
6.  Paste the copied value into `vendor/cookie.json`, replacing `REPLACE_WITH_YOUR_SESSION_COOKIE`.

#### Environment Variables (Optional)

You can further configure the agent with these environment variables:

-   `GEMINI_ACP_COOKIES`: Path to the cookie file (defaults to `vendor/cookie.json`).
-   `GEMINI_ACP_MODEL`: Default model to use (e.g., `gemini-3.6-flash`).
-   `GEMINI_ACP_AUTH_USER`: Specify a Google account profile if you use multiple accounts (e.g., `0` for the default, `1` for the second, etc.). Corresponds to the `/u/1/` part of the URL.
-   `GEMINI_ACP_PROXY`: Set an HTTP/S proxy (e.g., `http://127.0.0.1:7890`).

### 2. Build the Agent

Build the `gemini-acp` binary in release mode:

```sh
cargo build --release --bin gemini-acp
```

The executable will be located at `target/release/gemini-acp`.

### 3. Integrate with Zed

Open Zed's settings file (`zed > settings > open settings`) and add a server configuration pointing to the compiled binary:

```json
"assistant": {
  "version": "2",
  "enabled": true,
  "servers": [
    {
      "name": "gemini-acp",
      "binary": {
        "path": "/full/path/to/your/ny-gemini-acp/target/release/gemini-acp"
      }
    }
  ]
}
```

Replace `/full/path/to/your/ny-gemini-acp` with the absolute path to the repository on your machine. You can now access the agent via the assistant panel in Zed.

## Usage: OpenAI-Compatible API Server

This repository also includes a standalone server that emulates the OpenAI API, allowing you to use the Gemini web backend with any tool that supports the OpenAI Chat Completions endpoint.

### 1. Configuration

The server uses the same cookie-based authentication as the ACP agent. Ensure `vendor/cookie.json` is configured correctly. Additional options can be set in a `config.json` file at the root of the repository or via `GEMINI_WEB2API_*` environment variables.

### 2. Run the Server

Start the server with Cargo:

```sh
cargo run --release --bin gemini-web2api
```

By default, the server will be available at `http://127.0.0.1:8081`. You can now point your tools to this address and use it as an OpenAI API proxy.

## License

This project is licensed under the MIT License.
