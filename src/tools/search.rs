use rmcp::model::{CallToolResult, ContentBlock};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::{NineRouterClient, SearchRequestBody};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SearchType {
    Web,
    News,
    X,
}

impl SearchType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::News => "news",
            Self::X => "x",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebSearchParams {
    /// Search query to send to the backend. Must not be empty.
    pub query: String,

    /// Maximum number of search results to return (1 to 20). Defaults to 5.
    pub max_results: Option<u32>,

    /// Type of search: 'web', 'news', or 'x'. Defaults to 'web'.
    pub search_type: Option<SearchType>,

    /// Two-letter country code for localized search results (e.g. 'US', 'ID').
    pub country: Option<String>,

    /// Two-letter language code for localized search results (e.g. 'en', 'id').
    pub language: Option<String>,

    /// Time range for results (e.g. 'day', 'week', 'month', 'year').
    pub time_range: Option<String>,

    /// Specific domain to restrict search to (e.g. 'github.com').
    pub domain_filter: Option<String>,

    /// Optional pass-through JSON object for provider-specific parameters.
    pub provider_options: Option<serde_json::Value>,
}

pub fn tool_success(value: serde_json::Value) -> CallToolResult {
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    let mut res = CallToolResult::structured(value);
    res.content = vec![ContentBlock::text(text)];
    res
}

pub fn tool_error<S: Into<String>>(message: S) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message.into())])
}

pub async fn execute_web_search(
    client: &NineRouterClient,
    search_combo: &str,
    params: WebSearchParams,
) -> CallToolResult {
    let trimmed_query = params.query.trim();
    if trimmed_query.is_empty() {
        return tool_error("Search query must not be empty");
    }

    if let Some(max_res) = params.max_results {
        if !(1..=20).contains(&max_res) {
            return tool_error("max_results must be between 1 and 20");
        }
    }

    let search_type_str = params.search_type.map(|st| st.as_str());

    let req = SearchRequestBody {
        model: search_combo,
        query: trimmed_query,
        max_results: params.max_results,
        search_type: search_type_str,
        country: params.country.as_deref(),
        language: params.language.as_deref(),
        time_range: params.time_range.as_deref(),
        domain_filter: params.domain_filter.as_deref(),
        provider_options: params.provider_options.as_ref(),
    };

    match client.search(&req).await {
        Ok(json_val) => tool_success(json_val),
        Err(e) => tool_error(e.to_string()),
    }
}
