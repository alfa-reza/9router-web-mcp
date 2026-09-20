use rmcp::model::CallToolResult;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::{FetchRequestBody, NineRouterClient};
use crate::normalize::validate_and_normalize_url;
use crate::tools::search::{tool_error, tool_success};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FetchFormat {
    Markdown,
    Text,
    Html,
}

impl FetchFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Text => "text",
            Self::Html => "html",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebFetchParams {
    /// Absolute HTTP or HTTPS URL to fetch and extract.
    pub url: String,

    /// Output format: 'markdown', 'text', or 'html'. Defaults to 'markdown'.
    pub format: Option<FetchFormat>,

    /// Maximum characters to return. Default is 8000. Set to 0 for unlimited.
    pub max_characters: Option<u64>,
}

pub async fn execute_web_fetch(
    client: &NineRouterClient,
    fetch_combo: &str,
    params: WebFetchParams,
) -> CallToolResult {
    let normalized_url = match validate_and_normalize_url(&params.url) {
        Ok(u) => u,
        Err(e) => return tool_error(e.to_string()),
    };

    let format_str = params.format.map(|f| f.as_str());

    let req = FetchRequestBody {
        model: fetch_combo,
        url: &normalized_url,
        format: format_str,
        max_characters: params.max_characters,
    };

    match client.fetch(&req).await {
        Ok(json_val) => tool_success(json_val),
        Err(e) => tool_error(e.to_string()),
    }
}
