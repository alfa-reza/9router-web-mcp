# 9router-mcp-web

[![CI](https://github.com/alfa-reza/9router-web-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/alfa-reza/9router-web-mcp/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/alfa-reza/9router-web-mcp?display_name=tag&sort=semver)](https://github.com/alfa-reza/9router-web-mcp/releases/latest)
[![Rust](https://img.shields.io/badge/built%20with-Rust-000000?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License](https://img.shields.io/github/license/alfa-reza/9router-web-mcp)](LICENSE)

A lightweight [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server written in Rust that exposes **Web Search** and **Web Fetch** through [9Router](https://github.com/decolua/9router).

`9router-mcp-web` is a thin adapter supporting both STDIO and local Streamable HTTP transports. Provider credentials, routing, combos, and fallback behavior stay in 9Router.

## Features

- `web_search` for web, news, and X search through a 9Router search combo.
- `web_fetch` for fetching URLs as Markdown, text, or HTML through a 9Router fetch combo.
- Dual transport support: STDIO (default) and local MCP Streamable HTTP.
- Automatic local 9Router discovery for default and custom-port local instances.
- Works with local or remote 9Router deployments.
- Accepts 9Router URLs with or without `/v1`.
- Keeps provider selection and credentials out of MCP tool calls.
- Prebuilt Linux binaries for `x86_64` and `aarch64`.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/alfa-reza/9router-web-mcp/main/install.sh | sh
```

Then configure the connection to 9Router:

```sh
9router-mcp-web configure
```

### Local Discovery & Configuration

When no explicit Base URL is configured, `9router-mcp-web` discovers local instances automatically:
1. Validates the standard local 9Router address (`http://127.0.0.1:20128`) via `/api/health`.
2. If unreachable, checks whether the `9router` binary is on `PATH` and safely inspects active launcher processes for an explicit custom port (`-p` / `--port`).
3. Determines keyless vs auth-required state without performing provider requests.
4. If no local instance is found, exits with an actionable manual configuration error.

Configuration resolution precedence is strictly:
```text
Environment (NINEROUTER_URL / NINEROUTER_BASE_URL)
    ↓
Config File (~/.config/9router-mcp-web/config.toml)
    ↓
Local Discovery (runtime-derived)
```

The default combo names are:

```text
search-combo
fetch-combo
```

Both URL styles are supported for explicit configuration:

```text
http://localhost:20128
http://localhost:20128/v1
```

Prebuilt binaries and checksums are available on the [Releases](https://github.com/alfa-reza/9router-web-mcp/releases) page.

## Transports

`9router-mcp-web` supports both **STDIO** (default) and **MCP Streamable HTTP**.

### STDIO (default)

STDIO remains the default transport:

```sh
9router-mcp-web
```

or explicitly:

```sh
9router-mcp-web --transport stdio
```

### Streamable HTTP

Run with the local Streamable HTTP transport:

```sh
9router-mcp-web --transport http
```

Default local endpoint:

```text
http://127.0.0.1:20129/mcp
```

Override the listen port with `--port`:

```sh
9router-mcp-web --transport http --port 3000
```

> [!NOTE]
> This HTTP transport:
> - binds to loopback only (`127.0.0.1`);
> - does not provide HTTPS/TLS;
> - does not provide MCP-layer authentication;
> - is intended strictly for local connections.

## Add to an MCP client

### Claude Code

```sh
claude mcp add --scope user 9router-web -- 9router-mcp-web
```

### Codex

```sh
codex mcp add 9router-web -- 9router-mcp-web
```

### JSON configuration

For MCP clients using STDIO:

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

For MCP clients connecting over Streamable HTTP:

```json
{
  "mcpServers": {
    "9router-web-http": {
      "url": "http://127.0.0.1:20129/mcp"
    }
  }
}
```

For a remote 9Router deployment, configure the connection first:

```sh
9router-mcp-web configure
```

or pass the supported `NINEROUTER_*` environment variables from your MCP client.

## Tools

| Tool | Description |
| --- | --- |
| `web_search` | Search the web through the configured 9Router search combo. |
| `web_fetch` | Fetch and extract content from a URL through the configured 9Router fetch combo. |

9Router remains responsible for provider selection, credentials, routing, combos, and fallback behavior.

## Requirements

- Linux `x86_64` or `aarch64`
- A reachable [9Router](https://github.com/decolua/9router) instance
- Search and fetch combos configured in 9Router

## Build from source

```sh
git clone https://github.com/alfa-reza/9router-web-mcp.git
cd 9router-web-mcp
cargo build --release --locked
```

The binary will be available at:

```text
target/release/9router-mcp-web
```

## License

Licensed under the [Apache License 2.0](LICENSE).
