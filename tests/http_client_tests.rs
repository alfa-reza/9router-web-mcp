use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ninerouter_mcp_web::client::{FetchRequestBody, NineRouterClient, SearchRequestBody};
use ninerouter_mcp_web::config::Config;
use ninerouter_mcp_web::error::AppError;

#[tokio::test]
async fn test_search_endpoint_with_auth_and_combo() {
    let mock_server = MockServer::start().await;

    let response_body = json!({
        "results": [
            {
                "title": "Rust Programming",
                "url": "https://rust-lang.org",
                "content": "A language empowering everyone..."
            }
        ],
        "custom_upstream_field": 12345
    });

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .and(header("Authorization", "Bearer sk-test-key"))
        .and(header("Content-Type", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = Config {
        base_url: mock_server.uri(),
        api_key: Some("sk-test-key".to_string()),
        search_combo: "test-search-combo".to_string(),
        fetch_combo: "test-fetch-combo".to_string(),
        timeout_secs: 10,
    };

    let client = NineRouterClient::new(&config).expect("Client init failed");

    let req = SearchRequestBody {
        model: &config.search_combo,
        query: "rust programming",
        max_results: Some(5),
        search_type: Some("web"),
        country: None,
        language: None,
        time_range: None,
        domain_filter: None,
        provider_options: None,
    };

    let result = client.search(&req).await.expect("Search failed");

    // Verify unknown field preserved (AC-16)
    assert_eq!(result.get("custom_upstream_field").unwrap(), 12345);
    assert_eq!(result["results"][0]["title"], "Rust Programming");
}

#[tokio::test]
async fn test_fetch_endpoint_without_auth() {
    let mock_server = MockServer::start().await;

    let response_body = json!({
        "url": "https://example.com",
        "title": "Example Domain",
        "content": "# Example Domain\n\nThis domain is for use in illustrative examples.",
        "extra_meta": { "cached": false }
    });

    struct NoAuthHeaderMatcher;
    impl wiremock::Match for NoAuthHeaderMatcher {
        fn matches(&self, request: &wiremock::Request) -> bool {
            !request.headers.contains_key("authorization")
        }
    }

    Mock::given(method("POST"))
        .and(path("/v1/web/fetch"))
        .and(NoAuthHeaderMatcher)
        .and(header("Content-Type", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = Config {
        base_url: mock_server.uri(),
        api_key: None,
        search_combo: "test-search-combo".to_string(),
        fetch_combo: "test-fetch-combo".to_string(),
        timeout_secs: 10,
    };

    let client = NineRouterClient::new(&config).expect("Client init failed");

    let req = FetchRequestBody {
        model: &config.fetch_combo,
        url: "https://example.com",
        format: Some("markdown"),
        max_characters: Some(8000),
    };

    let result = client.fetch(&req).await.expect("Fetch failed");
    assert_eq!(result["title"], "Example Domain");
    assert_eq!(result["extra_meta"]["cached"], false);
}

#[tokio::test]
async fn test_all_upstream_error_codes() {
    let mock_server = MockServer::start().await;

    let config = Config {
        base_url: mock_server.uri(),
        api_key: Some("sk-key".to_string()),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).unwrap();

    let search_req = SearchRequestBody {
        model: "search-combo",
        query: "test",
        max_results: None,
        search_type: None,
        country: None,
        language: None,
        time_range: None,
        domain_filter: None,
        provider_options: None,
    };

    // 1. 400 Bad Request
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({ "error": "Invalid domain filter" })),
        )
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    let err = client.search(&search_req).await.unwrap_err();
    assert!(matches!(err, AppError::BadRequest(msg) if msg.contains("Invalid domain filter")));

    // 2. 401 Authentication failed
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    let err = client.search(&search_req).await.unwrap_err();
    assert!(matches!(err, AppError::AuthenticationFailed));

    // 3. 429 Rate limited
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(
            ResponseTemplate::new(429).set_body_json(json!({ "message": "Rate limit exceeded" })),
        )
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    let err = client.search(&search_req).await.unwrap_err();
    assert!(matches!(err, AppError::RateLimited(msg) if msg.contains("Rate limit exceeded")));

    // 4. 503 Service unavailable
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(503).set_body_string("No provider available"))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    let err = client.search(&search_req).await.unwrap_err();
    assert!(
        matches!(err, AppError::ServiceUnavailable(msg) if msg.contains("No provider available"))
    );

    // 5. 500 Upstream server error
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal crash"))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    let err = client.search(&search_req).await.unwrap_err();
    assert!(matches!(
        err,
        AppError::UpstreamServerError { status: 500, .. }
    ));
}

#[tokio::test]
async fn test_response_too_large_content_length() {
    let mock_server = MockServer::start().await;

    let config = Config {
        base_url: mock_server.uri(),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).unwrap();

    let oversized_bytes = 10 * 1024 * 1024 + 1; // 10MB + 1

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; oversized_bytes]))
        .expect(1)
        .mount(&mock_server)
        .await;

    let req = SearchRequestBody {
        model: "search-combo",
        query: "test",
        max_results: None,
        search_type: None,
        country: None,
        language: None,
        time_range: None,
        domain_filter: None,
        provider_options: None,
    };

    let err = client.search(&req).await.unwrap_err();
    match &err {
        AppError::ResponseTooLarge { limit, observed } => {
            assert_eq!(*limit, 10 * 1024 * 1024);
            assert_eq!(*observed, Some(oversized_bytes));
        }
        other => panic!("Expected ResponseTooLarge, got {:?}", other),
    }
    assert!(err.to_string().contains("observed 10485761 bytes"));
}

#[tokio::test]
async fn test_error_message_bounded_preview() {
    let mock_server = MockServer::start().await;

    let config = Config {
        base_url: mock_server.uri(),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).unwrap();

    let huge_error = "A".repeat(50_000);

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(500).set_body_string(&huge_error))
        .expect(1)
        .mount(&mock_server)
        .await;

    let req = SearchRequestBody {
        model: "search-combo",
        query: "test",
        max_results: None,
        search_type: None,
        country: None,
        language: None,
        time_range: None,
        domain_filter: None,
        provider_options: None,
    };

    let err = client.search(&req).await.unwrap_err();
    match err {
        AppError::UpstreamServerError { status, message } => {
            assert_eq!(status, 500);
            assert!(message.contains("... [truncated]"));
            assert!(message.len() < 2000);
        }
        other => panic!("Expected UpstreamServerError, got {:?}", other),
    }
}

#[tokio::test]
async fn test_invalid_json_bounded_preview() {
    let mock_server = MockServer::start().await;

    let config = Config {
        base_url: mock_server.uri(),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).unwrap();

    let huge_invalid_json = "NOT_JSON ".repeat(5_000);

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&huge_invalid_json))
        .expect(1)
        .mount(&mock_server)
        .await;

    let req = SearchRequestBody {
        model: "search-combo",
        query: "test",
        max_results: None,
        search_type: None,
        country: None,
        language: None,
        time_range: None,
        domain_filter: None,
        provider_options: None,
    };

    let err = client.search(&req).await.unwrap_err();
    match err {
        AppError::InvalidResponseJson(msg) => {
            assert!(msg.contains("... [truncated]"));
            assert!(msg.len() < 2000);
        }
        other => panic!("Expected InvalidResponseJson, got {:?}", other),
    }
}
