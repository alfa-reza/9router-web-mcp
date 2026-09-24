use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use rmcp::model::CallToolRequestParams;
use rmcp::ServiceExt;

use ninerouter_mcp_web::client::{NineRouterClient, SearchProviderOptions, SearchRequestBody};
use ninerouter_mcp_web::config::Config;
use ninerouter_mcp_web::server::NineRouterMcpServer;
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
    let structured = result.structured_content.unwrap();
    assert_eq!(structured["results"][0]["title"], "Result 1");

    assert_eq!(result.content.len(), 1);
    let text = result.content[0]
        .as_text()
        .expect("Expected text content block")
        .text
        .as_str();
    assert!(
        !text.contains('\n'),
        "Search text content must be compact JSON: {}",
        text
    );
    let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed, structured);
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
    // 1. JSON Schema inspection: provider_options must be a closed/restricted object schema
    let schema = schemars::schema_for!(WebSearchParams);
    let schema_json = serde_json::to_value(&schema).unwrap();
    let provider_options_schema = &schema_json["properties"]["provider_options"];

    // Find the object subschema (whether inlined directly, or within anyOf for Option/nullable)
    let opt_schema = if let Some(any_of) = provider_options_schema
        .get("anyOf")
        .and_then(|a| a.as_array())
    {
        any_of
            .iter()
            .find(|s| s.get("properties").is_some() || s.get("type") == Some(&json!("object")))
            .expect("provider_options anyOf must contain an object schema")
    } else {
        provider_options_schema
    };

    // additionalProperties must be explicitly false (closed/restricted)
    assert_eq!(
        opt_schema["additionalProperties"],
        json!(false),
        "provider_options schema must set additionalProperties: false to fail closed"
    );

    // Only verified supported options are advertised
    let props = opt_schema["properties"]
        .as_object()
        .expect("properties must be an object");
    assert!(props.contains_key("cx"), "provider_options must expose cx");
    assert!(
        props.contains_key("depth"),
        "provider_options must expose depth"
    );
    assert!(
        props.contains_key("cursor"),
        "provider_options must expose cursor"
    );
    assert!(
        props.contains_key("queryType"),
        "provider_options must expose queryType"
    );

    // baseUrl and arbitrary keys must NOT be advertised
    assert!(
        !props.contains_key("baseUrl"),
        "provider_options must NOT expose baseUrl"
    );
    assert_eq!(
        props.len(),
        4,
        "provider_options must advertise exactly the 4 supported options (cx, depth, cursor, queryType)"
    );

    // Schema wire types must be string (or nullable string)
    let is_string_or_nullable_string =
        |val: &serde_json::Value| val == "string" || val == &json!(["string", "null"]);
    assert!(is_string_or_nullable_string(&props["cx"]["type"]));
    assert!(is_string_or_nullable_string(&props["depth"]["type"]));
    assert!(is_string_or_nullable_string(&props["cursor"]["type"]));
    assert!(is_string_or_nullable_string(&props["queryType"]["type"]));

    // Whole schema string must not contain baseUrl anywhere
    let full_schema_str = serde_json::to_string(&schema_json).unwrap();
    assert!(!full_schema_str.contains("\"baseUrl\""));

    // 2. Deserializing each supported option individually succeeds
    let valid_cx = json!({
        "query": "rust",
        "provider_options": { "cx": "cse-12345" }
    });
    let parsed: WebSearchParams = serde_json::from_value(valid_cx).unwrap();
    assert_eq!(
        parsed.provider_options.as_ref().unwrap().cx.as_deref(),
        Some("cse-12345")
    );

    let valid_depth = json!({
        "query": "rust",
        "provider_options": { "depth": "fast" }
    });
    let parsed: WebSearchParams = serde_json::from_value(valid_depth).unwrap();
    assert_eq!(
        parsed.provider_options.as_ref().unwrap().depth.as_deref(),
        Some("fast")
    );

    let valid_cursor = json!({
        "query": "rust",
        "provider_options": { "cursor": "cursor-abc" }
    });
    let parsed: WebSearchParams = serde_json::from_value(valid_cursor).unwrap();
    assert_eq!(
        parsed.provider_options.as_ref().unwrap().cursor.as_deref(),
        Some("cursor-abc")
    );

    let valid_query_type = json!({
        "query": "rust",
        "provider_options": { "queryType": "Latest" }
    });
    let parsed: WebSearchParams = serde_json::from_value(valid_query_type).unwrap();
    assert_eq!(
        parsed
            .provider_options
            .as_ref()
            .unwrap()
            .query_type
            .as_deref(),
        Some("Latest")
    );

    // 3. Deserializing valid combinations succeeds
    let valid_all = json!({
        "query": "rust",
        "provider_options": {
            "cx": "cse-12345",
            "depth": "standard",
            "cursor": "cursor-xyz",
            "queryType": "Top"
        }
    });
    let parsed: WebSearchParams = serde_json::from_value(valid_all).unwrap();
    let opts = parsed.provider_options.unwrap();
    assert_eq!(opts.cx.as_deref(), Some("cse-12345"));
    assert_eq!(opts.depth.as_deref(), Some("standard"));
    assert_eq!(opts.cursor.as_deref(), Some("cursor-xyz"));
    assert_eq!(opts.query_type.as_deref(), Some("Top"));

    // 4. Empty object, null, and omitted options succeed
    let empty_json = json!({
        "query": "rust",
        "provider_options": {}
    });
    let parsed: WebSearchParams = serde_json::from_value(empty_json).unwrap();
    assert_eq!(
        parsed.provider_options,
        Some(SearchProviderOptions::default())
    );

    let null_json = json!({
        "query": "rust",
        "provider_options": null
    });
    let parsed: WebSearchParams = serde_json::from_value(null_json).unwrap();
    assert_eq!(parsed.provider_options, None);

    let omitted_json = json!({
        "query": "rust"
    });
    let parsed: WebSearchParams = serde_json::from_value(omitted_json).unwrap();
    assert_eq!(parsed.provider_options, None);

    // 5. Valid combinations remain serializable with correct wire names
    let req = SearchRequestBody {
        model: "search-combo",
        query: "rust",
        max_results: Some(5),
        search_type: Some("web"),
        country: None,
        language: None,
        time_range: None,
        domain_filter: None,
        provider_options: Some(&opts),
    };
    let serialized = serde_json::to_value(&req).unwrap();
    assert_eq!(serialized["provider_options"]["cx"], "cse-12345");
    assert_eq!(serialized["provider_options"]["depth"], "standard");
    assert_eq!(serialized["provider_options"]["cursor"], "cursor-xyz");
    assert_eq!(serialized["provider_options"]["queryType"], "Top");

    // 6. provider_options.baseUrl MUST fail closed
    let base_url_json = json!({
        "query": "rust",
        "provider_options": {
            "baseUrl": "http://127.0.0.1:8080"
        }
    });
    let parsed_base_url: std::result::Result<WebSearchParams, _> =
        serde_json::from_value(base_url_json);
    assert!(parsed_base_url.is_err(), "baseUrl must be rejected");
    let err_msg = parsed_base_url.unwrap_err().to_string();
    assert!(
        err_msg.contains("unknown field `baseUrl`") || err_msg.contains("baseUrl"),
        "error message should indicate unknown field: {}",
        err_msg
    );

    // 7. Unknown arbitrary keys MUST fail closed
    let arbitrary_json = json!({
        "query": "rust",
        "provider_options": {
            "custom_flag": true,
            "engine": "google"
        }
    });
    let parsed_arbitrary: std::result::Result<WebSearchParams, _> =
        serde_json::from_value(arbitrary_json);
    assert!(
        parsed_arbitrary.is_err(),
        "arbitrary provider options must be rejected"
    );

    // 8. Deserializing invalid non-object types fails
    let scalar_json = json!({
        "query": "rust",
        "provider_options": "not-an-object"
    });
    assert!(serde_json::from_value::<WebSearchParams>(scalar_json).is_err());

    let array_json = json!({
        "query": "rust",
        "provider_options": [1, 2, 3]
    });
    assert!(serde_json::from_value::<WebSearchParams>(array_json).is_err());

    let wrong_type_json = json!({
        "query": "rust",
        "provider_options": {
            "cx": 12345
        }
    });
    assert!(serde_json::from_value::<WebSearchParams>(wrong_type_json).is_err());
}

#[tokio::test]
async fn test_provider_options_base_url_and_unknown_keys_rejected_no_upstream_request() {
    let mock_server = MockServer::start().await;

    // Upstream server expects EXACTLY ZERO requests because input validation must fail closed
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&mock_server)
        .await;

    let config = Config {
        base_url: mock_server.uri(),
        search_combo: "search-combo".to_string(),
        fetch_combo: "fetch-combo".to_string(),
        ..Default::default()
    };
    let server = NineRouterMcpServer::new(&config).unwrap();

    let (client_io, server_io) = tokio::io::duplex(8192);
    let (client_read, client_write) = tokio::io::split(client_io);
    let (server_read, server_write) = tokio::io::split(server_io);

    let server_handle = tokio::spawn(async move {
        let running = rmcp::service::serve_server(server, (server_read, server_write))
            .await
            .expect("Server init failed");
        let _ = running.waiting().await;
    });

    let client = ().serve((client_read, client_write)).await.expect("Client init failed");

    // 1. Call with provider_options.baseUrl -> MUST be rejected and no HTTP request sent
    let args_base_url = json!({
        "query": "security test",
        "provider_options": {
            "baseUrl": "http://127.0.0.1:20128/ssrf-target"
        }
    })
    .as_object()
    .unwrap()
    .clone();

    let call_result = client
        .peer()
        .call_tool(CallToolRequestParams::new("web_search").with_arguments(args_base_url))
        .await;

    // rmcp either returns an Err protocol response or a CallToolResult with is_error = true
    if let Ok(res) = call_result {
        assert_eq!(res.is_error, Some(true));
    }

    // 2. Call with unknown arbitrary key -> MUST be rejected and no HTTP request sent
    let args_unknown = json!({
        "query": "security test",
        "provider_options": {
            "unapproved_key": "injected_value"
        }
    })
    .as_object()
    .unwrap()
    .clone();

    let call_result = client
        .peer()
        .call_tool(CallToolRequestParams::new("web_search").with_arguments(args_unknown))
        .await;

    if let Ok(res) = call_result {
        assert_eq!(res.is_error, Some(true));
    }

    server_handle.abort();

    // Verify mock_server received exactly 0 requests (enforced by wiremock drop check as well)
    let received_requests = mock_server.received_requests().await.unwrap();
    assert_eq!(
        received_requests.len(),
        0,
        "No upstream HTTP request must be sent when provider_options contains baseUrl or unknown keys"
    );
}

#[tokio::test]
async fn test_provider_options_valid_forwarded_to_upstream() {
    let mock_server = MockServer::start().await;

    let expected_payload = json!({
        "model": "test-search-combo",
        "query": "valid options search",
        "provider_options": {
            "cx": "cse-id-999",
            "depth": "deep",
            "cursor": "next-page-tok",
            "queryType": "Latest"
        }
    });

    let response_body = json!({
        "results": [
            { "title": "Verified Result", "url": "https://example.com/verified" }
        ],
        "query": "valid options search"
    });

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .and(body_json(&expected_payload))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = Config {
        base_url: mock_server.uri(),
        search_combo: "test-search-combo".to_string(),
        fetch_combo: "fetch-combo".to_string(),
        ..Default::default()
    };
    let server = NineRouterMcpServer::new(&config).unwrap();

    let (client_io, server_io) = tokio::io::duplex(8192);
    let (client_read, client_write) = tokio::io::split(client_io);
    let (server_read, server_write) = tokio::io::split(server_io);

    let server_handle = tokio::spawn(async move {
        let running = rmcp::service::serve_server(server, (server_read, server_write))
            .await
            .expect("Server init failed");
        let _ = running.waiting().await;
    });

    let client = ().serve((client_read, client_write)).await.expect("Client init failed");

    let valid_args = json!({
        "query": "valid options search",
        "provider_options": {
            "cx": "cse-id-999",
            "depth": "deep",
            "cursor": "next-page-tok",
            "queryType": "Latest"
        }
    })
    .as_object()
    .unwrap()
    .clone();

    let call_result = client
        .peer()
        .call_tool(CallToolRequestParams::new("web_search").with_arguments(valid_args))
        .await
        .expect("Valid search call should succeed");

    assert_eq!(call_result.is_error, Some(false));
    assert!(call_result.structured_content.is_some());

    server_handle.abort();
}

#[tokio::test]
async fn test_unrelated_search_behavior_unchanged() {
    let mock_server = MockServer::start().await;

    let expected_payload = json!({
        "model": "combo",
        "query": "unrelated test",
        "max_results": 10,
        "search_type": "x",
        "country": "US",
        "language": "en",
        "time_range": "week",
        "domain_filter": "github.com"
    });

    let response_body = json!({
        "results": [
            { "title": "X Post", "url": "https://x.com/post/1" }
        ]
    });

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .and(body_json(&expected_payload))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = Config {
        base_url: mock_server.uri(),
        search_combo: "combo".to_string(),
        fetch_combo: "combo".to_string(),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).unwrap();

    let params = WebSearchParams {
        query: "unrelated test".to_string(),
        max_results: Some(10),
        search_type: Some(SearchType::X),
        country: Some("US".to_string()),
        language: Some("en".to_string()),
        time_range: Some("week".to_string()),
        domain_filter: Some("github.com".to_string()),
        provider_options: None,
    };

    let result = execute_web_search(&client, &config.search_combo, params).await;
    assert_eq!(result.is_error, Some(false));
    let structured = result.structured_content.unwrap();
    assert_eq!(structured["results"][0]["title"], "X Post");
}
