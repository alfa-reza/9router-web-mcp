use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ninerouter_mcp_web::client::NineRouterClient;
use ninerouter_mcp_web::config::Config;
use ninerouter_mcp_web::tools::search::{execute_web_search, SearchType, WebSearchParams};

#[tokio::test]
async fn test_search_tool_success() {
    let mock_server = MockServer::start().await;

    let response_body = json!({
        "results": [
            { "title": "Result 1", "url": "https://example.com/1" }
        ],
        "query": "rust"
    });

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = Config {
        base_url: mock_server.uri(),
        search_combo: "search-combo".to_string(),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).unwrap();

    let params = WebSearchParams {
        query: "rust".to_string(),
        max_results: Some(5),
        search_type: Some(SearchType::Web),
        country: None,
        language: None,
        time_range: None,
        domain_filter: None,
        provider_options: None,
    };

    let result = execute_web_search(&client, &config.search_combo, params).await;
    assert_eq!(result.is_error, Some(false));
    assert!(result.structured_content.is_some());
    assert!(!result.content.is_empty());
}

#[tokio::test]
async fn test_search_tool_validation_errors() {
    let config = Config::default();
    let client = NineRouterClient::new(&config).unwrap();

    // 1. Empty query
    let empty_query = WebSearchParams {
        query: "   ".to_string(),
        max_results: Some(5),
        search_type: None,
        country: None,
        language: None,
        time_range: None,
        domain_filter: None,
        provider_options: None,
    };
    let res = execute_web_search(&client, "search-combo", empty_query).await;
    assert_eq!(res.is_error, Some(true));

    // 2. max_results = 0
    let zero_max = WebSearchParams {
        query: "rust".to_string(),
        max_results: Some(0),
        search_type: None,
        country: None,
        language: None,
        time_range: None,
        domain_filter: None,
        provider_options: None,
    };
    let res = execute_web_search(&client, "search-combo", zero_max).await;
    assert_eq!(res.is_error, Some(true));

    // 3. max_results = 21
    let large_max = WebSearchParams {
        query: "rust".to_string(),
        max_results: Some(21),
        search_type: None,
        country: None,
        language: None,
        time_range: None,
        domain_filter: None,
        provider_options: None,
    };
    let res = execute_web_search(&client, "search-combo", large_max).await;
    assert_eq!(res.is_error, Some(true));
}

#[tokio::test]
async fn test_search_zero_results_is_success() {
    let mock_server = MockServer::start().await;

    let response_body = json!({
        "results": []
    });

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = Config {
        base_url: mock_server.uri(),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).unwrap();

    let params = WebSearchParams {
        query: "query with no hits".to_string(),
        max_results: Some(5),
        search_type: None,
        country: None,
        language: None,
        time_range: None,
        domain_filter: None,
        provider_options: None,
    };

    let result = execute_web_search(&client, &config.search_combo, params).await;
    assert_eq!(result.is_error, Some(false));
    let structured = result.structured_content.unwrap();
    assert_eq!(structured["results"].as_array().unwrap().len(), 0);
}
