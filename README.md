# 9router-mcp-web

[![CI](https://github.com/alfa-reza/9router-web-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/alfa-reza/9router-web-mcp/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

A lightweight, production-grade Model Context Protocol (MCP) server written in Rust that provides **Web Search** and **Web Fetch** capabilities to AI agents by delegating to an existing [9Router](https://github.com/alfa-reza/9router) deployment.

---

## Key Features

- **Exactly Two MCP Tools:** Exposes `web_search` and `web_fetch` over standard I/O (`stdio`).
- **Thin 9Router Interface:** Provider selection, API credentials, rate limits, account rotation, and fallback chains remain entirely inside 9Router. One MCP tool call corresponds strictly to one 9Router API request.
- **Operator-Controlled Routing:** The operator configures search and fetch combo names (defaulting to `search-combo` and `fetch-combo`). AI agents do not dictate or override backend providers.
- **GitHub Raw URL Normalization:** Automatically converts `github.com/.../blob/...` file URLs to their raw equivalent (`raw.githubusercontent.com/...`) for clean text extraction.
- **Protocol Purity:** Diagnostics and logs are strictly directed to `stderr`. `stdout` is reserved 100% for JSON-RPC 2.0 messages.
- **Resource Efficient:** Standalone, statically compiled Linux binaries (MUSL) with minimal memory footprint (~5–10 MB RSS), making it ideal for small VPS deployments (~2 GB RAM).

---

## Supported Platforms

Version 1 release targets **Linux only**:
- Linux `x86_64` (Intel/AMD 64-bit)
- Linux `aarch64` (ARM 64-bit)

---

## Installation

### Prebuilt Binary Installation

Run the integrity-verified installer script:

```bash
curl -fsSL https://raw.githubusercontent.com/alfa-reza/9router-web-mcp/main/install.sh | sh
```

The installer detects your CPU architecture, downloads the release artifact, verifies its SHA-256 checksum against `SHA256SUMS.txt`, installs the binary to `~/.local/bin/9router-mcp-web` (or `/usr/local/bin` if root), and prompts for initial configuration.

---

## Configuration

`9router-mcp-web` persists configuration in `~/.config/9router-mcp-web/config.toml` (permissions `0600`).

### Interactive Configuration

Run the configuration wizard at any time:

```bash
9router-mcp-web configure
```

```text
=== 9router-mcp-web Configuration ===
Config file destination: /home/user/.config/9router-mcp-web/config.toml

9Router URL (with or without /v1) [http://localhost:20128]: <press Enter to accept>
9Router API key (optional, press Enter to skip): <masked input>
Search combo name [search-combo]: <press Enter to accept>
Fetch combo name [fetch-combo]: <press Enter to accept>
Request timeout in seconds [30]: <press Enter to accept>

Configuration saved successfully to: /home/user/.config/9router-mcp-web/config.toml
```

Pressing **Enter** without typing a value selects the displayed default.

### Configuration File Format (`config.toml`)

```toml
# 9Router base URL (required; both with or without /v1, e.g. "http://localhost:20128" or "http://localhost:20128/v1")
base_url = "http://localhost:20128"

# Optional API key (if 9Router requires authentication)
# api_key = "sk-..."

# 9Router combo names
search_combo = "search-combo"
fetch_combo = "fetch-combo"

# Request timeout in seconds
timeout_secs = 30
```

### Environment Overrides

Environment variables override persistent configuration:

| Variable | Description | Default |
| :--- | :--- | :--- |
| `NINEROUTER_URL` | 9Router base endpoint URL (with or without `/v1`) | `http://localhost:20128` |
| `NINEROUTER_KEY` | 9Router API bearer key | None (optional) |
| `NINEROUTER_SEARCH_COMBO` | Web search combo name | `search-combo` |
| `NINEROUTER_FETCH_COMBO` | Web fetch combo name | `fetch-combo` |
| `NINEROUTER_TIMEOUT_SECS` | Network request timeout | `30` |
| `NINEROUTER_CONFIG` | Custom config file path | `~/.config/9router-mcp-web/config.toml` |

---

## MCP Client Configuration

Add `9router-mcp-web` to your MCP client configuration:

### Claude Code

```bash
claude mcp add --scope user 9router-mcp-web -- 9router-mcp-web
```

### VS Code / Cursor / Windsurf

Add to your `mcpServers` configuration:

```json
{
  "mcpServers": {
    "9router-web": {
      "command": "9router-mcp-web",
      "args": []
    }
  }
}
```

### Codex CLI

```bash
codex mcp add 9router-web -- 9router-mcp-web
```

### Remote 9Router Example (Over Environment)

```json
{
  "mcpServers": {
    "9router-web": {
      "command": "9router-mcp-web",
      "args": [],
      "env": {
        "NINEROUTER_URL": "https://9router.mycompany.com",
        "NINEROUTER_KEY": "sk-secret-key"
      }
    }
  }
}
```

---

## Tool Reference

### 1. `web_search`

Performs web search through the configured 9Router search combo.

- **`query`** *(string, required)*: The search term or question.
- **`max_results`** *(integer, optional)*: Number of results (1–20, default `5`).
- **`search_type`** *(string, optional)*: `'web'`, `'news'`, or `'x'` (default `'web'`).
- **`country`** *(string, optional)*: Two-letter country code (e.g. `'US'`, `'ID'`).
- **`language`** *(string, optional)*: Two-letter language code (e.g. `'en'`, `'id'`).
- **`time_range`** *(string, optional)*: Time range filter (e.g. `'day'`, `'week'`, `'month'`).
- **`domain_filter`** *(string, optional)*: Restrict search to domain (e.g. `'github.com'`).
- **`provider_options`** *(object, optional)*: Pass-through options for backend provider.

### 2. `web_fetch`

Fetches and extracts text from a target web URL through the configured 9Router fetch combo.

- **`url`** *(string, required)*: Absolute `http` or `https` URL.
- **`format`** *(string, optional)*: Output format: `'markdown'`, `'text'`, or `'html'` (default `'markdown'`).
- **`max_characters`** *(integer, optional)*: Maximum characters to return (default `8000`, `0` for unlimited).

---

## Security & Privacy

- **Secret Safety:** The 9Router API key is never printed in logs, errors, or MCP tool output.
- **Storage Protection:** Configuration files are saved with permissions `0600` in a `0700` directory.
- **Plaintext Warning:** A warning is logged to `stderr` if an API key is configured over unencrypted HTTP to a non-local host.
- **No Direct Provider Keys:** Provider credentials reside only in 9Router.
- **No Local Filesystem Fetch:** Local file paths (`file://`) are strictly rejected before making network requests.
- **No Telemetry & No Caching:** Version 1 collects no telemetry, records no search history, and does not cache page content locally.

---

## License

Licensed under the [Apache License, Version 2.0](LICENSE).