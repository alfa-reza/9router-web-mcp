use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::redirect::Policy;
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

use crate::config::Config;
use crate::error::{AppError, Result};

pub const MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024; // 10 MB safety limit

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
    pub provider_options: Option<&'a Value>,
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
            base_url: config.base_url.clone(),
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
                return Err(AppError::ResponseTooLarge(content_length as usize));
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
                return Err(AppError::ResponseTooLarge(bytes.len() + chunk.len()));
            }
            bytes.extend_from_slice(&chunk);
        }

        let body_str = String::from_utf8_lossy(&bytes).to_string();

        if !status.is_success() {
            let error_msg = extract_error_message(&body_str, status.as_u16());
            let code = status.as_u16();

            return Err(match code {
                400 => AppError::BadRequest(error_msg),
                401 | 403 => AppError::AuthenticationFailed,
                429 => AppError::RateLimited(error_msg),
                503 => AppError::ServiceUnavailable(error_msg),
                _ => AppError::UpstreamServerError {
                    status: code,
                    message: error_msg,
                },
            });
        }

        serde_json::from_slice::<Value>(&bytes)
            .map_err(|e| AppError::InvalidResponseJson(format!("{}: {}", e, body_str)))
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

fn extract_error_message(body: &str, status: u16) -> String {
    if body.trim().is_empty() {
        return format!("HTTP {}", status);
    }

    if let Ok(parsed) = serde_json::from_str::<Value>(body) {
        if let Some(err_obj) = parsed.get("error") {
            if let Some(msg) = err_obj.get("message").and_then(|m| m.as_str()) {
                return msg.to_string();
            }
            if let Some(err_str) = err_obj.as_str() {
                return err_str.to_string();
            }
        }
        if let Some(msg) = parsed.get("message").and_then(|m| m.as_str()) {
            return msg.to_string();
        }
    }

    body.to_string()
}
