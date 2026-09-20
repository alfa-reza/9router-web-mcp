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

#[test]
fn test_provider_options_schema_and_validation() {
    // 1. JSON Schema inspection: provider_options must be an object
    let schema = schemars::schema_for!(WebSearchParams);
    let schema_json = serde_json::to_value(&schema).unwrap();
    let provider_options_schema = &schema_json["properties"]["provider_options"];
    assert!(
        provider_options_schema["type"] == "object"
            || provider_options_schema["type"] == json!(["object", "null"])
            || provider_options_schema.get("anyOf").is_some()
    );

    // 2. Deserializing a valid object succeeds
    let valid_json = json!({
        "query": "rust",
        "provider_options": {
            "custom_flag": true,
            "engine": "google"
        }
    });
    let parsed: std::result::Result<WebSearchParams, _> = serde_json::from_value(valid_json);
    assert!(parsed.is_ok());
    let params = parsed.unwrap();
    assert!(params.provider_options.is_some());
    assert_eq!(
        params.provider_options.unwrap().get("custom_flag").unwrap(),
        &json!(true)
    );

    // 3. Deserializing a scalar string fails
    let scalar_json = json!({
        "query": "rust",
        "provider_options": "not-an-object"
    });
    let parsed_scalar: std::result::Result<WebSearchParams, _> =
        serde_json::from_value(scalar_json);
    assert!(parsed_scalar.is_err());

    // 4. Deserializing an array fails
    let array_json = json!({
        "query": "rust",
        "provider_options": [1, 2, 3]
    });
    let parsed_array: std::result::Result<WebSearchParams, _> = serde_json::from_value(array_json);
    assert!(parsed_array.is_err());
}
