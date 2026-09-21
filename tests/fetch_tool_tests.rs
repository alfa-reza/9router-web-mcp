use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ninerouter_mcp_web::client::NineRouterClient;
use ninerouter_mcp_web::config::Config;
use ninerouter_mcp_web::tools::fetch::{execute_web_fetch, FetchFormat, WebFetchParams};

#[tokio::test]
async fn test_fetch_tool_success_with_github_raw_conversion() {
    let mock_server = MockServer::start().await;

    // Upstream must receive the converted raw URL
    let expected_payload = json!({
        "model": "test-fetch-combo",
        "url": "https://raw.githubusercontent.com/owner/repo/main/src/lib.rs",
        "format": "markdown",
        "max_characters": 5000
    });

    let response_body = json!({
        "title": "lib.rs",
        "content": "pub fn hello() {}",
        "url": "https://raw.githubusercontent.com/owner/repo/main/src/lib.rs"
    });

    Mock::given(method("POST"))
        .and(path("/v1/web/fetch"))
        .and(body_json(&expected_payload))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = Config {
        base_url: mock_server.uri(),
        fetch_combo: "test-fetch-combo".to_string(),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).unwrap();

    let params = WebFetchParams {
        url: "https://github.com/owner/repo/blob/main/src/lib.rs?plain=1#L1-L10".to_string(),
        format: Some(FetchFormat::Markdown),
        max_characters: Some(5000),
    };

    let result = execute_web_fetch(&client, &config.fetch_combo, params).await;
    assert_eq!(result.is_error, Some(false));
    assert!(result.structured_content.is_some());
    assert_eq!(result.structured_content.unwrap()["title"], "lib.rs");
}

#[tokio::test]
async fn test_fetch_tool_invalid_url_error() {
    let config = Config::default();
    let client = NineRouterClient::new(&config).unwrap();

    // 1. file:// scheme rejected
    let file_url = WebFetchParams {
        url: "file:///etc/passwd".to_string(),
        format: None,
        max_characters: None,
    };
    let result = execute_web_fetch(&client, "fetch-combo", file_url).await;
    assert_eq!(result.is_error, Some(true));

    // 2. Relative URL rejected
    let rel_url = WebFetchParams {
        url: "not-a-valid-url".to_string(),
        format: None,
        max_characters: None,
    };
    let result = execute_web_fetch(&client, "fetch-combo", rel_url).await;
    assert_eq!(result.is_error, Some(true));
}

#[tokio::test]
async fn test_fetch_tool_default_max_characters() {
    let mock_server = MockServer::start().await;

    let expected_payload = json!({
        "model": "fetch-combo",
        "url": "https://example.com/page",
        "max_characters": 8000
    });

    let response_body = json!({
        "title": "Page",
        "content": "Page content",
        "url": "https://example.com/page"
    });

    Mock::given(method("POST"))
        .and(path("/v1/web/fetch"))
        .and(body_json(&expected_payload))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = Config {
        base_url: mock_server.uri(),
        fetch_combo: "fetch-combo".to_string(),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).unwrap();

    let params = WebFetchParams {
        url: "https://example.com/page".to_string(),
        format: None,
        max_characters: None,
    };

    let result = execute_web_fetch(&client, &config.fetch_combo, params).await;
    assert_eq!(result.is_error, Some(false));
}

#[tokio::test]
async fn test_fetch_tool_explicit_zero_max_characters() {
    let mock_server = MockServer::start().await;

    let expected_payload = json!({
        "model": "fetch-combo",
        "url": "https://example.com/unlimited",
        "max_characters": 0
    });

    let response_body = json!({
        "title": "Unlimited",
        "content": "Huge content",
        "url": "https://example.com/unlimited"
    });

    Mock::given(method("POST"))
        .and(path("/v1/web/fetch"))
        .and(body_json(&expected_payload))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = Config {
        base_url: mock_server.uri(),
        fetch_combo: "fetch-combo".to_string(),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).unwrap();

    let params = WebFetchParams {
        url: "https://example.com/unlimited".to_string(),
        format: None,
        max_characters: Some(0),
    };

    let result = execute_web_fetch(&client, &config.fetch_combo, params).await;
    assert_eq!(result.is_error, Some(false));
}
