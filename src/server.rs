use rmcp::handler::server::wrapper::Parameters;
use rmcp::handler::server::ServerHandler;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_handler, tool_router};
use std::sync::Arc;

use crate::client::NineRouterClient;
use crate::config::Config;
use crate::error::AppError;
use crate::tools::fetch::{execute_web_fetch, WebFetchParams};
use crate::tools::search::{execute_web_search, WebSearchParams};

#[derive(Clone)]
pub struct NineRouterMcpServer {
    client: Arc<NineRouterClient>,
    search_combo: String,
    fetch_combo: String,
}

impl NineRouterMcpServer {
    pub fn new(config: &Config) -> std::result::Result<Self, AppError> {
        let search_combo = config.search_combo.trim().to_string();
        if search_combo.is_empty() {
            return Err(AppError::Config(
                "search_combo cannot be empty or whitespace-only".to_string(),
            ));
        }
        let fetch_combo = config.fetch_combo.trim().to_string();
        if fetch_combo.is_empty() {
            return Err(AppError::Config(
                "fetch_combo cannot be empty or whitespace-only".to_string(),
            ));
        }
        let client = NineRouterClient::new(config)?;
        Ok(Self {
            client: Arc::new(client),
            search_combo,
            fetch_combo,
        })
    }

    pub fn client(&self) -> &NineRouterClient {
        &self.client
    }

    pub fn search_combo(&self) -> &str {
        &self.search_combo
    }

    pub fn fetch_combo(&self) -> &str {
        &self.fetch_combo
    }
}

#[tool_router]
impl NineRouterMcpServer {
    #[tool(
        name = "web_search",
        description = "Search the web and return the raw upstream search payload as JSON. Use when the user needs current information, links, or news. Searches through the configured 9Router search combo.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    pub async fn web_search(
        &self,
        Parameters(params): Parameters<WebSearchParams>,
    ) -> CallToolResult {
        execute_web_search(&self.client, &self.search_combo, params).await
    }

    #[tool(
        name = "web_fetch",
        description = "Fetch a URL and return its content as markdown (default), plain text, or HTML. Automatically converts GitHub source file (/blob/) URLs to raw content. Fetches through the configured 9Router fetch combo.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    pub async fn web_fetch(
        &self,
        Parameters(params): Parameters<WebFetchParams>,
    ) -> CallToolResult {
        execute_web_fetch(&self.client, &self.fetch_combo, params).await
    }
}

#[tool_handler(name = "9router-mcp-web")]
impl ServerHandler for NineRouterMcpServer {}
