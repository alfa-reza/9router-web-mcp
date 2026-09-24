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
async fn test_utf8_truncation_multibyte_crossing_boundary_no_replacement_char() {
    let mock_server = MockServer::start().await;

    let config = Config {
        base_url: mock_server.uri(),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).unwrap();

    // 1023 ASCII bytes + 4-byte crab emoji (\u{1F980} = [0xF0, 0x9F, 0xA6, 0x80]) + trailing bytes
    // Crab emoji crosses bytes 1023..1027, crossing the 1024-byte preview limit.
    let mut body = "a".repeat(1023).into_bytes();
    body.extend_from_slice("🦀".as_bytes());
    body.extend_from_slice(b"extra_tail_data");

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(500).set_body_bytes(body))
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
            // Must NOT contain Unicode replacement character U+FFFD
            assert!(
                !message.contains('\u{FFFD}'),
                "Error message contained replacement character due to split multibyte UTF-8: {}",
                message
            );
            // Sliced content before "... [truncated]" must stop cleanly before the split crab emoji (1023 'a's)
            let prefix = message.strip_suffix("... [truncated]").unwrap();
            assert_eq!(prefix.len(), 1023);
            assert_eq!(prefix, "a".repeat(1023));
        }
        other => panic!("Expected UpstreamServerError, got {:?}", other),
    }
}

#[tokio::test]
async fn test_utf8_truncation_multibyte_ending_exactly_at_limit() {
    let mock_server = MockServer::start().await;

    let config = Config {
        base_url: mock_server.uri(),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).unwrap();

    // 1020 ASCII bytes + 4-byte crab emoji (ends exactly at 1024) + trailing bytes
    let mut body = "a".repeat(1020).into_bytes();
    body.extend_from_slice("🦀".as_bytes());
    body.extend_from_slice(b"extra_tail_data");

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(500).set_body_bytes(body))
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
            assert!(!message.contains('\u{FFFD}'));
            let prefix = message.strip_suffix("... [truncated]").unwrap();
            assert_eq!(prefix.len(), 1024);
            assert!(prefix.ends_with('🦀'));
        }
        other => panic!("Expected UpstreamServerError, got {:?}", other),
    }
}

#[tokio::test]
async fn test_utf8_truncation_invalid_json_body_multibyte_crossing_boundary() {
    let mock_server = MockServer::start().await;

    let config = Config {
        base_url: mock_server.uri(),
        ..Default::default()
    };
    let client = NineRouterClient::new(&config).unwrap();

    let mut body = "NOT_JSON ".repeat(113).into_bytes(); // 9 * 113 = 1017 bytes
    body.extend_from_slice("bbbbbb".as_bytes()); // 1017 + 6 = 1023 bytes
    body.extend_from_slice("🦀".as_bytes()); // 1023..1027
    body.extend_from_slice(b"trailing invalid json content");

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
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
            assert!(
                !msg.contains('\u{FFFD}'),
                "Invalid response JSON preview contained replacement character: {}",
                msg
            );
        }
        other => panic!("Expected InvalidResponseJson, got {:?}", other),
    }
}
