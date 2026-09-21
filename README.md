# 9router-mcp-web

[![CI](https://github.com/alfa-reza/9router-web-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/alfa-reza/9router-web-mcp/actions/workflows/ci.yml)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-000000?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License](https://img.shields.io/github/license/alfa-reza/9router-web-mcp)](LICENSE)

A lightweight [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server that exposes **Web Search** and **Web Fetch** through [9Router](https://github.com/decolua/9router).

Written in Rust and designed as a thin STDIO adapter: provider credentials, routing, combos, and fallback behavior stay in 9Router.

## Features

- `web_search` for web, news, and X search through a 9Router search combo.
- `web_fetch` for fetching URLs as Markdown, text, or HTML through a 9Router fetch combo.
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

The default combo names are:

```text
search-combo
fetch-combo
```

Both URL styles are supported:

```text
http://localhost:20128
http://localhost:20128/v1
```

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

For MCP clients that use the `mcpServers` configuration shape:

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

If 9Router is remote, configure it first with:

```sh
9router-mcp-web configure
```

or pass supported `NINEROUTER_*` environment variables from your MCP client.

## Tools

| Tool | Description |
| --- | --- |
| `web_search` | Search the web through the configured 9Router search combo. |
| `web_fetch` | Fetch and extract content from a URL through the configured 9Router fetch combo. |

9Router remains responsible for provider selection, credentials, fallback, and combo behavior.

## Requirements

- Linux `x86_64` or `aarch64`
- A reachable [9Router](https://github.com/decolua/9router) instance
- Search/fetch combos configured in 9Router

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
