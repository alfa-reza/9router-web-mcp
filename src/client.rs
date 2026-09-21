use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::redirect::Policy;
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

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
    pub provider_options: Option<&'a serde_json::Map<String, Value>>,
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

        // Custom redirect policy: do NOT forward Authorization to different hosts (R-UP-05, S-10)
        let redirect_policy = Policy::custom(|attempt| {
            if attempt
                .previous()
                .iter()
                .any(|prev| prev.host() != attempt.url().host())
            {
                // Prevent following redirects to different host when credentials are involved
                attempt.stop()
            } else if attempt.previous().len() >= 10 {
                attempt.stop()
            } else {
                attempt.follow()
            }
        });

        let client = Client::builder()
            .default_headers(headers)
            .redirect(redirect_policy)
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

    /// Base URL helper to construct full endpoint URL
    fn endpoint_url(&self, path: &str) -> String {
        let clean_path = path.strip_prefix('/').unwrap_or(path);
        format!("{}/{}", self.base_url, clean_path)
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
