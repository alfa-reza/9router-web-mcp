use serde_json::json;
use url::Url;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ninerouter_mcp_web::client::{
    is_same_origin, should_follow_redirect, FetchRequestBody, NineRouterClient, SearchRequestBody,
};
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
    assert!(err.to_string().contains("401"));
    assert!(!err.to_string().contains("403"));

    // 2b. 403 Access forbidden (distinct from 401)
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden resource"))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    let err = client.search(&search_req).await.unwrap_err();
    assert!(matches!(err, AppError::Forbidden(ref msg) if msg.contains("Forbidden resource")));
    assert!(err.to_string().contains("403"));
    assert!(!err.to_string().contains("401"));

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

#[tokio::test]
async fn test_fetch_endpoint_401_and_403_distinct() {
    let mock_server = MockServer::start().await;

    let config = Config {
        base_url: mock_server.uri(),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).unwrap();

    let fetch_req = FetchRequestBody {
        model: "fetch-combo",
        url: "https://example.com/article",
        format: Some("markdown"),
        max_characters: Some(1000),
    };

    // 401 Unauthorized
    Mock::given(method("POST"))
        .and(path("/v1/web/fetch"))
        .respond_with(ResponseTemplate::new(401).set_body_string("API key invalid"))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    let err = client.fetch(&fetch_req).await.unwrap_err();
    assert!(matches!(err, AppError::AuthenticationFailed));
    assert!(err.to_string().contains("401"));
    assert!(!err.to_string().contains("403"));

    // 403 Forbidden
    Mock::given(method("POST"))
        .and(path("/v1/web/fetch"))
        .respond_with(ResponseTemplate::new(403).set_body_string("Account suspended"))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    let err = client.fetch(&fetch_req).await.unwrap_err();
    assert!(matches!(err, AppError::Forbidden(ref msg) if msg.contains("Account suspended")));
    assert!(err.to_string().contains("403"));
    assert!(!err.to_string().contains("401"));
}

#[test]
fn test_endpoint_url_construction_all_base_url_variations() {
    let test_cases = vec![
        // (input_base_url, expected_search, expected_fetch)
        (
            "http://host:20128",
            "http://host:20128/v1/search",
            "http://host:20128/v1/web/fetch",
        ),
        (
            "http://host:20128/",
            "http://host:20128/v1/search",
            "http://host:20128/v1/web/fetch",
        ),
        (
            "http://host:20128/v1",
            "http://host:20128/v1/search",
            "http://host:20128/v1/web/fetch",
        ),
        (
            "http://host:20128/v1/",
            "http://host:20128/v1/search",
            "http://host:20128/v1/web/fetch",
        ),
        (
            "https://example.com/router",
            "https://example.com/router/v1/search",
            "https://example.com/router/v1/web/fetch",
        ),
        (
            "https://example.com/router/",
            "https://example.com/router/v1/search",
            "https://example.com/router/v1/web/fetch",
        ),
        (
            "https://example.com/router/v1",
            "https://example.com/router/v1/search",
            "https://example.com/router/v1/web/fetch",
        ),
        (
            "https://example.com/router/v1/",
            "https://example.com/router/v1/search",
            "https://example.com/router/v1/web/fetch",
        ),
        (
            "https://example.com/api-v1",
            "https://example.com/api-v1/v1/search",
            "https://example.com/api-v1/v1/web/fetch",
        ),
    ];

    for (base_url, expected_search, expected_fetch) in test_cases {
        let config = Config {
            base_url: base_url.to_string(),
            ..Default::default()
        };
        let client = NineRouterClient::new(&config).expect("Client creation failed");

        let search_url = client.endpoint_url("v1/search");
        let fetch_url = client.endpoint_url("v1/web/fetch");

        assert_eq!(
            search_url, expected_search,
            "Search endpoint mismatch for base_url: {}",
            base_url
        );
        assert_eq!(
            fetch_url, expected_fetch,
            "Fetch endpoint mismatch for base_url: {}",
            base_url
        );

        // Explicitly assert that /v1/v1/... is NEVER produced
        assert!(
            !search_url.contains("/v1/v1/search"),
            "Search URL improperly contained /v1/v1/search for base_url: {}",
            base_url
        );
        assert!(
            !fetch_url.contains("/v1/v1/web/fetch"),
            "Fetch URL improperly contained /v1/v1/web/fetch for base_url: {}",
            base_url
        );
    }
}

#[tokio::test]
async fn test_client_observable_requests_with_and_without_v1_base_url() {
    let mock_server = MockServer::start().await;

    // Search and Fetch mock responses
    let search_resp = json!({ "results": [{ "title": "Test", "url": "https://example.com" }] });
    let fetch_resp = json!({ "content": "fetched content" });

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&search_resp))
        .expect(3) // 3 tests: without /v1, with /v1, with /v1/
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/web/fetch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&fetch_resp))
        .expect(3) // 3 tests: without /v1, with /v1, with /v1/
        .mount(&mock_server)
        .await;

    let search_req = SearchRequestBody {
        model: "search-combo",
        query: "test query",
        max_results: Some(1),
        search_type: None,
        country: None,
        language: None,
        time_range: None,
        domain_filter: None,
        provider_options: None,
    };

    let fetch_req = FetchRequestBody {
        model: "fetch-combo",
        url: "https://example.com",
        format: Some("markdown"),
        max_characters: Some(100),
    };

    // 1. Base URL without /v1: e.g. "http://127.0.0.1:PORT"
    let cfg1 = Config {
        base_url: mock_server.uri(),
        ..Default::default()
    };
    let client1 = NineRouterClient::new(&cfg1).unwrap();
    assert!(client1.search(&search_req).await.is_ok());
    assert!(client1.fetch(&fetch_req).await.is_ok());

    // 2. Base URL with /v1: e.g. "http://127.0.0.1:PORT/v1"
    let cfg2 = Config {
        base_url: format!("{}/v1", mock_server.uri()),
        ..Default::default()
    };
    let client2 = NineRouterClient::new(&cfg2).unwrap();
    assert!(client2.search(&search_req).await.is_ok());
    assert!(client2.fetch(&fetch_req).await.is_ok());

    // 3. Base URL with /v1/: e.g. "http://127.0.0.1:PORT/v1/"
    let cfg3 = Config {
        base_url: format!("{}/v1/", mock_server.uri()),
        ..Default::default()
    };
    let client3 = NineRouterClient::new(&cfg3).unwrap();
    assert!(client3.search(&search_req).await.is_ok());
    assert!(client3.fetch(&fetch_req).await.is_ok());
}

#[tokio::test]
async fn test_same_origin_redirect_works_and_preserves_auth() {
    let mock_server = MockServer::start().await;

    let response_body = json!({
        "results": [
            {
                "title": "Redirected Result",
                "url": "https://example.com/item",
                "content": "Content after same-origin redirect"
            }
        ]
    });

    // 1. Initial request to /v1/search redirects within the same origin (relative path Location)
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .and(header("Authorization", "Bearer sk-test-key"))
        .respond_with(ResponseTemplate::new(307).insert_header("Location", "/v1/search-redirected"))
        .expect(1)
        .mount(&mock_server)
        .await;

    // 2. Redirect destination must receive the request AND the Authorization credential
    Mock::given(method("POST"))
        .and(path("/v1/search-redirected"))
        .and(header("Authorization", "Bearer sk-test-key"))
        .and(header("Content-Type", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = Config {
        base_url: mock_server.uri(),
        api_key: Some("sk-test-key".to_string()),
        search_combo: "test-combo".to_string(),
        fetch_combo: "test-combo".to_string(),
        timeout_secs: 10,
    };
    let client = NineRouterClient::new(&config).expect("Client init failed");

    let req = SearchRequestBody {
        model: "test-combo",
        query: "test query",
        max_results: Some(5),
        search_type: None,
        country: None,
        language: None,
        time_range: None,
        domain_filter: None,
        provider_options: None,
    };

    let result = client
        .search(&req)
        .await
        .expect("Search through same-origin redirect must succeed");
    assert_eq!(result["results"][0]["title"], "Redirected Result");

    // Directly verify both endpoints were invoked and received the Authorization header
    let recorded = mock_server
        .received_requests()
        .await
        .expect("Request recording enabled");
    assert_eq!(recorded.len(), 2);
    for r in &recorded {
        let auth = r
            .headers
            .get("authorization")
            .expect("Authorization header must be present on same-origin redirect");
        assert_eq!(auth.to_str().unwrap(), "Bearer sk-test-key");
    }
}

#[tokio::test]
async fn test_same_origin_redirect_works_for_fetch_endpoint() {
    let mock_server = MockServer::start().await;

    let response_body = json!({
        "url": "https://example.com",
        "title": "Fetched Article",
        "content": "Article content after redirect"
    });

    Mock::given(method("POST"))
        .and(path("/v1/web/fetch"))
        .and(header("Authorization", "Bearer sk-fetch-key"))
        .respond_with(
            ResponseTemplate::new(307).insert_header("Location", "/v1/web/fetch-redirected"),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/web/fetch-redirected"))
        .and(header("Authorization", "Bearer sk-fetch-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = Config {
        base_url: mock_server.uri(),
        api_key: Some("sk-fetch-key".to_string()),
        fetch_combo: "test-fetch-combo".to_string(),
        timeout_secs: 10,
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).expect("Client init failed");

    let req = FetchRequestBody {
        model: "test-fetch-combo",
        url: "https://example.com",
        format: Some("markdown"),
        max_characters: Some(5000),
    };

    let result = client
        .fetch(&req)
        .await
        .expect("Fetch through same-origin redirect must succeed");
    assert_eq!(result["title"], "Fetched Article");

    let recorded = mock_server
        .received_requests()
        .await
        .expect("Request recording enabled");
    assert_eq!(recorded.len(), 2);
    for r in &recorded {
        let auth = r
            .headers
            .get("authorization")
            .expect("Authorization must be present on same-origin redirect");
        assert_eq!(auth.to_str().unwrap(), "Bearer sk-fetch-key");
    }
}

#[tokio::test]
async fn test_host_changing_redirect_disallowed_target_receives_no_request_or_credential() {
    let origin_server = MockServer::start().await;
    let target_server = MockServer::start().await;

    // Disallowed target server must receive ZERO requests
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&target_server)
        .await;

    // Redirect to different host (127.0.0.1 -> localhost)
    let redirect_url = format!(
        "http://localhost:{}/v1/search",
        target_server.address().port()
    );

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(307).insert_header("Location", redirect_url.as_str()))
        .expect(1)
        .mount(&origin_server)
        .await;

    let config = Config {
        base_url: origin_server.uri(),
        api_key: Some("sk-secret-token".to_string()),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).expect("Client init failed");

    let req = SearchRequestBody {
        model: "test-combo",
        query: "test query",
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
        AppError::UpstreamServerError { status, .. } => assert_eq!(status, 307),
        other => panic!("Expected UpstreamServerError(307), got {:?}", other),
    }

    // Direct observation: target server received ZERO requests and ZERO credentials
    let target_requests = target_server
        .received_requests()
        .await
        .expect("Request recording enabled");
    assert!(
        target_requests.is_empty(),
        "Disallowed target server must not receive any requests on cross-host redirect"
    );
}

#[tokio::test]
async fn test_effective_port_changing_redirect_disallowed_target_receives_no_request_or_credential()
{
    let origin_server = MockServer::start().await;
    let target_server = MockServer::start().await;

    assert_ne!(
        origin_server.address().port(),
        target_server.address().port(),
        "Origin and target servers must be on different ports"
    );

    // Target server must NOT receive any requests
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&target_server)
        .await;

    // Redirect to same host (127.0.0.1) but different port
    let redirect_url = format!("{}/v1/search", target_server.uri());

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(307).insert_header("Location", redirect_url.as_str()))
        .expect(1)
        .mount(&origin_server)
        .await;

    let config = Config {
        base_url: origin_server.uri(),
        api_key: Some("sk-secret-token".to_string()),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).expect("Client init failed");

    let req = SearchRequestBody {
        model: "test-combo",
        query: "test query",
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
        AppError::UpstreamServerError { status, .. } => assert_eq!(status, 307),
        other => panic!("Expected UpstreamServerError(307), got {:?}", other),
    }

    // Direct observation: target server received ZERO requests and ZERO credentials
    let target_requests = target_server
        .received_requests()
        .await
        .expect("Request recording enabled");
    assert!(
        target_requests.is_empty(),
        "Disallowed target server must not receive any requests on port-changing redirect"
    );
}

#[tokio::test]
async fn test_scheme_changing_redirect_disallowed_target_receives_no_request_or_credential() {
    let origin_server = MockServer::start().await;
    let target_server = MockServer::start().await;

    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&target_server)
        .await;

    // Redirect changing scheme from HTTP to HTTPS
    let redirect_url = format!(
        "https://127.0.0.1:{}/v1/search",
        target_server.address().port()
    );

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(307).insert_header("Location", redirect_url.as_str()))
        .expect(1)
        .mount(&origin_server)
        .await;

    let config = Config {
        base_url: origin_server.uri(),
        api_key: Some("sk-secret-token".to_string()),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).expect("Client init failed");

    let req = SearchRequestBody {
        model: "test-combo",
        query: "test query",
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
        AppError::UpstreamServerError { status, .. } => assert_eq!(status, 307),
        other => panic!("Expected UpstreamServerError(307), got {:?}", other),
    }

    let target_requests = target_server
        .received_requests()
        .await
        .expect("Request recording enabled");
    assert!(
        target_requests.is_empty(),
        "Disallowed target server must not receive any requests on scheme-changing redirect"
    );
}

#[test]
fn test_https_to_http_same_host_and_port_is_rejected_by_policy() {
    // Regression check for invariant:
    // https://example.test:8443/... -> http://example.test:8443/...
    // reqwest 0.12's remove_sensitive_headers does NOT consider scheme when checking cross_host,
    // so reqwest alone would have sent the Authorization header across the scheme downgrade.
    // The same-origin redirect policy must explicitly reject this boundary.
    let https_url = Url::parse("https://example.test:8443/v1/search").unwrap();
    let http_url = Url::parse("http://example.test:8443/v1/search").unwrap();

    assert!(
        !is_same_origin(&https_url, &http_url),
        "HTTPS and HTTP with same hostname and explicit port must NOT be same origin"
    );
    assert!(
        !should_follow_redirect(std::slice::from_ref(&https_url), &http_url),
        "Redirect from HTTPS to HTTP on same host and explicit port must NOT be followed"
    );

    // Conversely, same-origin HTTPS to HTTPS redirect must be allowed
    let https_target = Url::parse("https://example.test:8443/v1/search-redirected").unwrap();
    assert!(
        is_same_origin(&https_url, &https_target),
        "Same-origin HTTPS URLs must be recognized as same origin"
    );
    assert!(
        should_follow_redirect(std::slice::from_ref(&https_url), &https_target),
        "Same-origin HTTPS redirect must be followed"
    );
}

#[test]
fn test_origin_boundary_matrix_exhaustively() {
    // 1. Scheme boundaries (scheme changes)
    let https_default = Url::parse("https://example.com/v1/search").unwrap();
    let http_default = Url::parse("http://example.com/v1/search").unwrap();
    assert!(!is_same_origin(&https_default, &http_default));
    assert!(!should_follow_redirect(
        std::slice::from_ref(&https_default),
        &http_default
    ));
    assert!(!is_same_origin(&http_default, &https_default));
    assert!(!should_follow_redirect(
        std::slice::from_ref(&http_default),
        &https_default
    ));

    // 2. Host boundaries (host changes)
    let origin_a = Url::parse("http://example.com:8080/v1/search").unwrap();
    let origin_b = Url::parse("http://other.com:8080/v1/search").unwrap();
    assert!(!is_same_origin(&origin_a, &origin_b));
    assert!(!should_follow_redirect(
        std::slice::from_ref(&origin_a),
        &origin_b
    ));

    let ip_a = Url::parse("http://127.0.0.1:8080/v1/search").unwrap();
    let ip_b = Url::parse("http://127.0.0.2:8080/v1/search").unwrap();
    assert!(!is_same_origin(&ip_a, &ip_b));
    assert!(!should_follow_redirect(std::slice::from_ref(&ip_a), &ip_b));

    let localhost = Url::parse("http://localhost:8080/v1/search").unwrap();
    assert!(!is_same_origin(&ip_a, &localhost));
    assert!(!should_follow_redirect(
        std::slice::from_ref(&ip_a),
        &localhost
    ));

    let ipv6_a = Url::parse("http://[::1]:8080/v1/search").unwrap();
    let ipv6_b = Url::parse("http://[::2]:8080/v1/search").unwrap();
    assert!(!is_same_origin(&ipv6_a, &ipv6_b));
    assert!(!should_follow_redirect(
        std::slice::from_ref(&ipv6_a),
        &ipv6_b
    ));

    // 3. Effective port boundaries (port changes)
    let http_explicit_80 = Url::parse("http://example.com:80/v1/search").unwrap();
    let http_explicit_8080 = Url::parse("http://example.com:8080/v1/search").unwrap();
    assert!(!is_same_origin(&http_default, &http_explicit_8080));
    assert!(!should_follow_redirect(
        std::slice::from_ref(&http_default),
        &http_explicit_8080
    ));
    assert!(!is_same_origin(&http_explicit_80, &http_explicit_8080));

    let https_explicit_443 = Url::parse("https://example.com:443/v1/search").unwrap();
    let https_explicit_8443 = Url::parse("https://example.com:8443/v1/search").unwrap();
    assert!(!is_same_origin(&https_default, &https_explicit_8443));
    assert!(!should_follow_redirect(
        std::slice::from_ref(&https_default),
        &https_explicit_8443
    ));
    assert!(!is_same_origin(&https_explicit_443, &https_explicit_8443));

    // 4. Same origin equivalence (explicit vs default port)
    assert!(is_same_origin(&http_default, &http_explicit_80));
    assert!(should_follow_redirect(
        std::slice::from_ref(&http_default),
        &http_explicit_80
    ));
    assert!(is_same_origin(&http_explicit_80, &http_default));
    assert!(should_follow_redirect(
        std::slice::from_ref(&http_explicit_80),
        &http_default
    ));

    assert!(is_same_origin(&https_default, &https_explicit_443));
    assert!(should_follow_redirect(
        std::slice::from_ref(&https_default),
        &https_explicit_443
    ));
    assert!(is_same_origin(&https_explicit_443, &https_default));
    assert!(should_follow_redirect(
        std::slice::from_ref(&https_explicit_443),
        &https_default
    ));

    // Same origin with path, query, and fragment differences
    let complex_a = Url::parse("https://api.9router.com:8443/v1/search?q=test#frag").unwrap();
    let complex_b = Url::parse("https://api.9router.com:8443/v1/search/other?page=2").unwrap();
    assert!(is_same_origin(&complex_a, &complex_b));
    assert!(should_follow_redirect(
        std::slice::from_ref(&complex_a),
        &complex_b
    ));

    // 5. Multi-hop chains and hop limits
    let hop0 = Url::parse("https://example.com/start").unwrap();
    let hop1 = Url::parse("https://example.com/step1").unwrap();
    let hop2 = Url::parse("https://example.com/step2").unwrap();
    let cross = Url::parse("https://other.com/step3").unwrap();

    assert!(should_follow_redirect(&[hop0.clone(), hop1.clone()], &hop2));
    assert!(!should_follow_redirect(
        &[hop0.clone(), hop1.clone()],
        &cross
    ));
    assert!(!should_follow_redirect(
        &[hop0.clone(), cross.clone()],
        &hop2
    ));

    // Max 10 hops: 10 previous items must be stopped
    let ten_hops: Vec<Url> = (0..10)
        .map(|i| Url::parse(&format!("https://example.com/hop{}", i)).unwrap())
        .collect();
    assert!(!should_follow_redirect(&ten_hops, &hop2));

    // Empty previous must be stopped
    assert!(!should_follow_redirect(&[], &hop2));
}

#[tokio::test]
async fn test_same_origin_redirect_with_absolute_url_and_308() {
    let mock_server = MockServer::start().await;

    let response_body = json!({
        "results": [
            {
                "title": "Permanent Redirect Result",
                "url": "https://example.com/permanent"
            }
        ]
    });

    let target_absolute_url = format!("{}/v1/search-perm", mock_server.uri());

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .and(header("Authorization", "Bearer sk-test-key"))
        .respond_with(
            ResponseTemplate::new(308).insert_header("Location", target_absolute_url.as_str()),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/search-perm"))
        .and(header("Authorization", "Bearer sk-test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = Config {
        base_url: mock_server.uri(),
        api_key: Some("sk-test-key".to_string()),
        search_combo: "test-combo".to_string(),
        fetch_combo: "test-combo".to_string(),
        timeout_secs: 10,
    };
    let client = NineRouterClient::new(&config).expect("Client init failed");

    let req = SearchRequestBody {
        model: "test-combo",
        query: "test query",
        max_results: Some(1),
        search_type: None,
        country: None,
        language: None,
        time_range: None,
        domain_filter: None,
        provider_options: None,
    };

    let result = client
        .search(&req)
        .await
        .expect("Search through 308 same-origin redirect must succeed");
    assert_eq!(result["results"][0]["title"], "Permanent Redirect Result");
}

#[tokio::test]
async fn test_redirect_loop_stops_at_hop_limit() {
    let mock_server = MockServer::start().await;

    // Set up a redirect ping-pong between /v1/ping and /v1/pong
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(307).insert_header("Location", "/v1/pong"))
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/pong"))
        .respond_with(ResponseTemplate::new(307).insert_header("Location", "/v1/ping"))
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/ping"))
        .respond_with(ResponseTemplate::new(307).insert_header("Location", "/v1/pong"))
        .mount(&mock_server)
        .await;

    let config = Config {
        base_url: mock_server.uri(),
        api_key: Some("sk-test-key".to_string()),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).expect("Client init failed");

    let req = SearchRequestBody {
        model: "test-combo",
        query: "test query",
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
        AppError::UpstreamServerError { status, .. } => assert_eq!(status, 307),
        other => panic!("Expected UpstreamServerError(307), got {:?}", other),
    }

    // Must have stopped around the hop limit (10 redirects), not recursed infinitely
    let reqs = mock_server.received_requests().await.unwrap();
    assert!(
        reqs.len() <= 11,
        "Total requests must be bounded by redirect limit: observed {}",
        reqs.len()
    );
}

#[test]
fn test_domain_case_insensitivity_and_url_origin_equivalence() {
    let lower = Url::parse("https://example.com:8443/test").unwrap();
    let upper = Url::parse("https://EXAMPLE.COM:8443/other").unwrap();
    assert!(is_same_origin(&lower, &upper));
    assert_eq!(lower.origin(), upper.origin());

    let mixed = Url::parse("http://Sub.Domain.Example.Test:80/a").unwrap();
    let canonical = Url::parse("http://sub.domain.example.test/b").unwrap();
    assert!(is_same_origin(&mixed, &canonical));
    assert_eq!(mixed.origin(), canonical.origin());
}

#[tokio::test]
async fn test_scheme_changing_redirect_same_host_and_port_is_stopped() {
    let origin_server = MockServer::start().await;

    // Origin redirects from HTTP to HTTPS on the exact same host and explicit port
    let redirect_url = format!(
        "https://127.0.0.1:{}/v1/search-tls",
        origin_server.address().port()
    );

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(307).insert_header("Location", redirect_url.as_str()))
        .expect(1)
        .mount(&origin_server)
        .await;

    // Disallowed target endpoint must receive ZERO requests
    Mock::given(method("POST"))
        .and(path("/v1/search-tls"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&origin_server)
        .await;

    let config = Config {
        base_url: origin_server.uri(),
        api_key: Some("sk-secret-token".to_string()),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).expect("Client init failed");

    let req = SearchRequestBody {
        model: "test-combo",
        query: "test query",
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
        AppError::UpstreamServerError { status, .. } => assert_eq!(status, 307),
        other => panic!("Expected UpstreamServerError(307), got {:?}", other),
    }

    let origin_requests = origin_server
        .received_requests()
        .await
        .expect("Request recording enabled");
    assert_eq!(
        origin_requests.len(),
        1,
        "Origin server must receive only initial request and never a redirected request across scheme boundary"
    );
}

#[tokio::test]
async fn test_malformed_location_header_handled_gracefully() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(
            ResponseTemplate::new(307).insert_header("Location", "http://[invalid-ipv6-bracket/"),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = Config {
        base_url: mock_server.uri(),
        api_key: Some("sk-secret-token".to_string()),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).expect("Client init failed");

    let req = SearchRequestBody {
        model: "test-combo",
        query: "test query",
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
        AppError::UpstreamServerError { status, .. } => assert_eq!(status, 307),
        other => panic!(
            "Expected UpstreamServerError(307) for unparseable redirect, got {:?}",
            other
        ),
    }

    let reqs = mock_server.received_requests().await.unwrap();
    assert_eq!(
        reqs.len(),
        1,
        "Mock server must receive exactly 1 request and not attempt an invalid redirect"
    );
}

#[test]
fn test_ipv6_normalization_and_origin_equivalence() {
    let compressed = Url::parse("http://[::1]:8080/v1/search").unwrap();
    let expanded = Url::parse("http://[0:0:0:0:0:0:0:1]:8080/v1/search").unwrap();
    let leading_zeros =
        Url::parse("http://[0000:0000:0000:0000:0000:0000:0000:0001]:8080/v1/search").unwrap();

    // IPv6 addresses in different representations normalize to same host
    assert!(is_same_origin(&compressed, &expanded));
    assert!(is_same_origin(&compressed, &leading_zeros));
    assert!(should_follow_redirect(
        std::slice::from_ref(&compressed),
        &expanded
    ));

    // Different IPv6 addresses are not same origin
    let different_ip = Url::parse("http://[::2]:8080/v1/search").unwrap();
    assert!(!is_same_origin(&compressed, &different_ip));
    assert!(!should_follow_redirect(
        std::slice::from_ref(&compressed),
        &different_ip
    ));

    // Port differences on IPv6
    let different_port = Url::parse("http://[::1]:8081/v1/search").unwrap();
    assert!(!is_same_origin(&compressed, &different_port));

    // Default port vs explicit default port on IPv6
    let ipv6_http_default = Url::parse("http://[::1]/v1/search").unwrap();
    let ipv6_http_80 = Url::parse("http://[::1]:80/v1/search").unwrap();
    assert!(is_same_origin(&ipv6_http_default, &ipv6_http_80));

    let ipv6_https_default = Url::parse("https://[::1]/v1/search").unwrap();
    let ipv6_https_443 = Url::parse("https://[::1]:443/v1/search").unwrap();
    assert!(is_same_origin(&ipv6_https_default, &ipv6_https_443));

    // Scheme difference on IPv6 with same host and port
    let ipv6_https_8443 = Url::parse("https://[::1]:8443/v1/search").unwrap();
    let ipv6_http_8443 = Url::parse("http://[::1]:8443/v1/search").unwrap();
    assert!(!is_same_origin(&ipv6_https_8443, &ipv6_http_8443));
    assert!(!should_follow_redirect(
        std::slice::from_ref(&ipv6_https_8443),
        &ipv6_http_8443
    ));
}
