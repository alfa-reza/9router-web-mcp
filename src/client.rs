use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::redirect::Policy;
use reqwest::Client;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use url::Url;

use crate::config::{validate_and_normalize_base_url, Config};
use crate::error::{AppError, Result};

pub const MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024; // 10 MB safety limit
pub const MAX_ERROR_PREVIEW_BYTES: usize = 1024; // 1 KB error preview cap

#[derive(Clone)]
pub struct NineRouterClient {
    client: Client,
    base_url: String,
    api_key: Option<String>,
    timeout_secs: u64,
}

impl NineRouterClient {
    pub fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }
}

/// Provider-specific search options supported by 9Router.
///
/// Only documented and intentionally supported per-request option fields are accepted.
/// Arbitrary keys and endpoint overrides such as `baseUrl` are rejected.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
pub struct SearchProviderOptions {
    /// Google Custom Search Engine ID (required for google-pse).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cx: Option<String>,

    /// Search depth for providers supporting it (e.g. 'fast', 'standard', 'deep' for Linkup).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<String>,

    /// Pagination cursor for continuing search (e.g. for Xquik).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,

    /// Query type for providers supporting it (e.g. 'Latest', 'Top' for Xquik).
    #[serde(default, rename = "queryType", skip_serializing_if = "Option::is_none")]
    pub query_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SearchRequestBody<'a> {
    pub model: &'a str,
    pub query: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_type: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_range: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain_filter: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<&'a SearchProviderOptions>,
}

#[derive(Debug, Serialize)]
pub struct FetchRequestBody<'a> {
    pub model: &'a str,
    pub url: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_characters: Option<u64>,
}

/// Check if two URLs have the same origin (scheme, host, and effective port).
pub fn is_same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host().is_some()
        && a.host() == b.host()
        && a.port_or_known_default() == b.port_or_known_default()
}

/// Evaluates whether a redirect from previous URLs to the target URL should be followed.
///
/// Invariant: A credential-bearing request must not follow a redirect to a different origin.
/// Returns true if all previous hops share the exact same origin (scheme, host, effective port)
/// as the target URL, and redirect depth is within the hop limit (< 10).
pub fn should_follow_redirect(previous: &[Url], next: &Url) -> bool {
    if previous.is_empty() || previous.len() >= 10 {
        return false;
    }
    previous.iter().all(|prev| is_same_origin(prev, next))
}

/// Custom redirect policy enforcing the same-origin invariant across redirect hops.
pub fn same_origin_redirect_policy() -> Policy {
    Policy::custom(|attempt| {
        if should_follow_redirect(attempt.previous(), attempt.url()) {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

impl NineRouterClient {
    pub fn new(config: &Config) -> Result<Self> {
        let base_url = validate_and_normalize_base_url(&config.base_url)?;

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if let Some(key) = &config.api_key {
            let trimmed = key.trim();
            if !trimmed.is_empty() {
                let auth_val = format!("Bearer {}", trimmed);
                let mut val = HeaderValue::from_str(&auth_val)
                    .map_err(|e| AppError::Config(format!("Invalid API key characters: {}", e)))?;
                val.set_sensitive(true);
                headers.insert(AUTHORIZATION, val);
            }
        }

        // Custom redirect policy: do NOT follow redirects across origin boundaries (scheme, host, effective port)
        let client = Client::builder()
            .default_headers(headers)
            .redirect(same_origin_redirect_policy())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| AppError::Config(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self {
            client,
            base_url,
            api_key: config.api_key.clone(),
            timeout_secs: config.timeout_secs,
        })
    }

    /// Constructs the full endpoint URL for a given 9Router API path.
    ///
    /// Central Version 1 UX rule:
    /// - If the configured Base URL already ends in path segment "v1", append the endpoint below it.
    /// - Otherwise insert "v1" before the endpoint.
    ///
    /// This accepts both `http://host:port` and `http://host:port/v1` (and reverse-proxy prefixes
    /// like `https://example.com/9router` and `https://example.com/9router/v1`) without producing
    /// `/v1/v1/...`. Suffixes like `/api-v1` are not treated as `/v1`.
    pub fn endpoint_url(&self, path: &str) -> String {
        let clean_path = path.strip_prefix('/').unwrap_or(path);
        let subpath = clean_path.strip_prefix("v1/").unwrap_or(clean_path);

        if let Ok(mut url) = Url::parse(&self.base_url) {
            let has_v1_suffix = url
                .path_segments()
                .and_then(|mut segs| segs.rfind(|s| !s.is_empty()))
                == Some("v1");

            let segments: Vec<&str> = if has_v1_suffix {
                subpath.split('/').filter(|s| !s.is_empty()).collect()
            } else {
                std::iter::once("v1")
                    .chain(subpath.split('/').filter(|s| !s.is_empty()))
                    .collect()
            };

            let mutated = match url.path_segments_mut() {
                Ok(mut path_segs) => {
                    path_segs.pop_if_empty();
                    path_segs.extend(&segments);
                    true
                }
                Err(_) => false,
            };

            if mutated {
                return url.to_string();
            }
        }

        // Defensive fallback
        let clean_base = self.base_url.trim_end_matches('/');
        if clean_base.ends_with("/v1") {
            format!("{}/{}", clean_base, subpath)
        } else {
            format!("{}/v1/{}", clean_base, subpath)
        }
    }

    /// Send a request to 9Router with bounded response buffering and distinct error mapping
    async fn send_request<T: Serialize>(&self, path: &str, body: &T) -> Result<Value> {
        let url = self.endpoint_url(path);

        let response = self
            .client
            .post(&url)
            .json(body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    AppError::Timeout(self.timeout_secs)
                } else if e.is_connect() {
                    AppError::NetworkUnreachable(url.clone())
                } else {
                    AppError::NetworkUnreachable(format!("{}: {}", url, e))
                }
            })?;

        let status = response.status();

        // Check Content-Length header for early rejection if oversized
        if let Some(content_length) = response.content_length() {
            if content_length > MAX_RESPONSE_BYTES as u64 {
                return Err(AppError::ResponseTooLarge {
                    limit: MAX_RESPONSE_BYTES,
                    observed: Some(content_length as usize),
                });
            }
        }

        // Read response body with streaming byte counter to protect memory (F-10, AC-18)
        let mut response = response;
        let mut bytes = Vec::new();

        while let Some(chunk) = response.chunk().await.map_err(|e| {
            if e.is_timeout() {
                AppError::Timeout(self.timeout_secs)
            } else {
                AppError::NetworkUnreachable(format!("Error reading response stream: {}", e))
            }
        })? {
            if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(AppError::ResponseTooLarge {
                    limit: MAX_RESPONSE_BYTES,
                    observed: Some(bytes.len() + chunk.len()),
                });
            }
            bytes.extend_from_slice(&chunk);
        }

        if !status.is_success() {
            let error_msg = extract_error_message(&bytes, status.as_u16());
            let code = status.as_u16();

            return Err(match code {
                400 => AppError::BadRequest(error_msg),
                401 => AppError::AuthenticationFailed,
                403 => AppError::Forbidden(error_msg),
                429 => AppError::RateLimited(error_msg),
                503 => AppError::ServiceUnavailable(error_msg),
                _ => AppError::UpstreamServerError {
                    status: code,
                    message: error_msg,
                },
            });
        }

        serde_json::from_slice::<Value>(&bytes).map_err(|e| {
            let preview = format_error_preview(&bytes);
            AppError::InvalidResponseJson(format!("{}: {}", e, preview))
        })
    }

    /// Execute search through 9Router POST /v1/search
    pub async fn search(&self, body: &SearchRequestBody<'_>) -> Result<Value> {
        self.send_request("v1/search", body).await
    }

    /// Execute web fetch through 9Router POST /v1/web/fetch
    pub async fn fetch(&self, body: &FetchRequestBody<'_>) -> Result<Value> {
        self.send_request("v1/web/fetch", body).await
    }
}

fn truncate_preview(s: &str) -> String {
    if s.len() <= MAX_ERROR_PREVIEW_BYTES {
        s.to_string()
    } else {
        let mut boundary = MAX_ERROR_PREVIEW_BYTES;
        while !s.is_char_boundary(boundary) && boundary > 0 {
            boundary -= 1;
        }
        format!("{}... [truncated]", &s[..boundary])
    }
}

fn format_error_preview(bytes: &[u8]) -> String {
    let capped_len = bytes.len().min(MAX_ERROR_PREVIEW_BYTES);
    let s = String::from_utf8_lossy(&bytes[..capped_len]);
    if bytes.len() > MAX_ERROR_PREVIEW_BYTES {
        format!("{}... [truncated]", s)
    } else {
        s.to_string()
    }
}

fn extract_error_message(bytes: &[u8], status: u16) -> String {
    if bytes.is_empty() {
        return format!("HTTP {}", status);
    }

    if let Ok(parsed) = serde_json::from_slice::<Value>(bytes) {
        if let Some(err_obj) = parsed.get("error") {
            if let Some(msg) = err_obj.get("message").and_then(|m| m.as_str()) {
                return truncate_preview(msg);
            }
            if let Some(err_str) = err_obj.as_str() {
                return truncate_preview(err_str);
            }
        }
        if let Some(msg) = parsed.get("message").and_then(|m| m.as_str()) {
            return truncate_preview(msg);
        }
    }

    let preview = format_error_preview(bytes);
    let trimmed = preview.trim();
    if trimmed.is_empty() {
        format!("HTTP {}", status)
    } else {
        trimmed.to_string()
    }
}
