use serde_json::json;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use rmcp::model::{CallToolRequestParams, ClientConfig};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;

use ninerouter_mcp_web::config::Config;
use ninerouter_mcp_web::http::{bind_http_listener, serve_with_listener};
use ninerouter_mcp_web::server::NineRouterMcpServer;

/// Helper to spawn an HTTP server on an ephemeral port (127.0.0.1:0)
async fn spawn_test_server(
    server: NineRouterMcpServer,
    ct: CancellationToken,
) -> (
    std::net::SocketAddr,
    tokio::task::JoinHandle<ninerouter_mcp_web::error::Result<()>>,
) {
    let listener = bind_http_listener(0)
        .await
        .expect("Failed to bind ephemeral port");
    let addr = listener.local_addr().expect("Failed to get local addr");
    let handle = tokio::spawn(serve_with_listener(listener, server, ct));
    (addr, handle)
}

#[tokio::test]
async fn test_http_transport_tools_list_and_call_with_mock_9router() {
    let mock_server = MockServer::start().await;

    let search_payload = json!({
        "results": [
            { "title": "Streamable HTTP Search", "url": "https://example.com/search" }
        ]
    });
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&search_payload))
        .mount(&mock_server)
        .await;

    let fetch_payload = json!({
        "title": "Streamable HTTP Fetch",
        "content": "# Streamable HTTP Content",
        "url": "https://example.com/page"
    });
    Mock::given(method("POST"))
        .and(path("/v1/web/fetch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&fetch_payload))
        .mount(&mock_server)
        .await;

    let config = Config {
        base_url: mock_server.uri(),
        search_combo: "search-combo".to_string(),
        fetch_combo: "fetch-combo".to_string(),
        ..Default::default()
    };
    let server = NineRouterMcpServer::new(&config).unwrap();

    let ct = CancellationToken::new();
    let (addr, server_handle) = spawn_test_server(server, ct.clone()).await;

    // Connect official rmcp Streamable HTTP client to /mcp
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!("http://{addr}/mcp")),
    );
    let client = ClientConfig::default()
        .serve(transport)
        .await
        .expect("Client should connect to /mcp");

    // 1. tools/list exposes exactly web_search and web_fetch
    let tools = client
        .list_tools(Default::default())
        .await
        .expect("List tools should succeed");
    assert_eq!(tools.tools.len(), 2);
    let tool_names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
    assert!(tool_names.contains(&"web_search"));
    assert!(tool_names.contains(&"web_fetch"));

    // 2. call web_search tool over HTTP transport
    let search_args = json!({ "query": "test streamable http" })
        .as_object()
        .unwrap()
        .clone();
    let search_res = client
        .call_tool(CallToolRequestParams::new("web_search").with_arguments(search_args))
        .await
        .expect("Search tool call should succeed");
    assert_eq!(search_res.is_error, Some(false));
    assert!(search_res.structured_content.is_some());
    let structured = search_res.structured_content.unwrap();
    assert_eq!(structured["results"][0]["title"], "Streamable HTTP Search");

    // 3. call web_fetch tool over HTTP transport
    let fetch_args = json!({ "url": "https://example.com/page" })
        .as_object()
        .unwrap()
        .clone();
    let fetch_res = client
        .call_tool(CallToolRequestParams::new("web_fetch").with_arguments(fetch_args))
        .await
        .expect("Fetch tool call should succeed");
    assert_eq!(fetch_res.is_error, Some(false));
    assert!(fetch_res.structured_content.is_some());
    let fetch_structured = fetch_res.structured_content.unwrap();
    assert_eq!(fetch_structured["title"], "Streamable HTTP Fetch");

    // 4. Clean shutdown
    ct.cancel();
    tokio::time::timeout(Duration::from_secs(5), server_handle)
        .await
        .expect("Server should stop within timeout")
        .expect("Server task should not panic")
        .expect("Server should exit Ok(())");
}

#[tokio::test]
async fn test_http_endpoint_routing_and_method_not_allowed() {
    let config = Config::default();
    let server = NineRouterMcpServer::new(&config).unwrap();

    let ct = CancellationToken::new();
    let (addr, server_handle) = spawn_test_server(server, ct.clone()).await;

    let http_client = reqwest::Client::new();

    // 1. GET / returns 404 Not Found (only /mcp is mounted)
    let res_root = http_client
        .get(format!("http://{addr}/"))
        .send()
        .await
        .expect("GET / should connect");
    assert_eq!(res_root.status(), reqwest::StatusCode::NOT_FOUND);

    // 2. GET /unknown returns 404 Not Found
    let res_unknown = http_client
        .get(format!("http://{addr}/healthz"))
        .send()
        .await
        .expect("GET /healthz should connect");
    assert_eq!(res_unknown.status(), reqwest::StatusCode::NOT_FOUND);

    // 3. GET /mcp without accept header returns 405 Method Not Allowed or 406 Not Acceptable (never SSE stream)
    let res_get_mcp = http_client
        .get(format!("http://{addr}/mcp"))
        .send()
        .await
        .expect("GET /mcp should connect");
    assert!(
        res_get_mcp.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED
            || res_get_mcp.status() == reqwest::StatusCode::NOT_ACCEPTABLE,
        "GET /mcp without proper headers must not serve stream, got status: {}",
        res_get_mcp.status()
    );

    // 4. POST /mcp is handled by MCP service
    let init_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "raw-test-client", "version": "1.0" }
        }
    });
    let res_post = http_client
        .post(format!("http://{addr}/mcp"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&init_body)
        .send()
        .await
        .expect("POST /mcp should connect");
    assert_eq!(res_post.status(), reqwest::StatusCode::OK);

    ct.cancel();
    let _ = server_handle.await;
}

#[tokio::test]
async fn test_http_security_host_and_origin_validation() {
    let config = Config::default();
    let server = NineRouterMcpServer::new(&config).unwrap();

    let ct = CancellationToken::new();
    let (addr, server_handle) = spawn_test_server(server, ct.clone()).await;

    let http_client = reqwest::Client::new();
    let init_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "sec-test-client", "version": "1.0" }
        }
    });

    // 1. Valid local Host header (127.0.0.1:<port>) is accepted
    let res_valid_host = http_client
        .post(format!("http://{addr}/mcp"))
        .header("Host", addr.to_string())
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&init_body)
        .send()
        .await
        .expect("Request should send");
    assert_eq!(res_valid_host.status(), reqwest::StatusCode::OK);

    // Verify no CORS header is returned
    assert!(
        res_valid_host
            .headers()
            .get("access-control-allow-origin")
            .is_none(),
        "No CORS headers should be returned"
    );

    // 2. Disallowed Host header (e.g. evil.com) is rejected with 403 Forbidden
    let res_disallowed_host = http_client
        .post(format!("http://{addr}/mcp"))
        .header("Host", "evil.com")
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&init_body)
        .send()
        .await
        .expect("Request should send");
    assert_eq!(
        res_disallowed_host.status(),
        reqwest::StatusCode::FORBIDDEN,
        "Disallowed host must receive 403 Forbidden"
    );

    // 2b. DNS rebinding host (e.g. 127.0.0.1.nip.io) is rejected with 403 Forbidden
    let res_rebinding_host = http_client
        .post(format!("http://{addr}/mcp"))
        .header("Host", format!("127.0.0.1.nip.io:{}", addr.port()))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&init_body)
        .send()
        .await
        .expect("Request should send");
    assert_eq!(
        res_rebinding_host.status(),
        reqwest::StatusCode::FORBIDDEN,
        "DNS rebinding host must receive 403 Forbidden"
    );

    // 2c. Normal local Host with 'localhost' name is accepted
    let res_localhost = http_client
        .post(format!("http://{addr}/mcp"))
        .header("Host", format!("localhost:{}", addr.port()))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&init_body)
        .send()
        .await
        .expect("Request should send");
    assert_eq!(
        res_localhost.status(),
        reqwest::StatusCode::OK,
        "Host: localhost:<port> must be accepted"
    );

    // 3. Request with Origin header is rejected with 403 Forbidden (enforce_origin_validation with empty allowlist)
    let res_disallowed_origin = http_client
        .post(format!("http://{addr}/mcp"))
        .header("Host", addr.to_string())
        .header("Origin", "http://evil.com")
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&init_body)
        .send()
        .await
        .expect("Request should send");
    assert_eq!(
        res_disallowed_origin.status(),
        reqwest::StatusCode::FORBIDDEN,
        "Disallowed Origin must receive 403 Forbidden"
    );

    // 4. Another Origin header test (e.g. localhost browser origin) is also rejected
    let res_browser_origin = http_client
        .post(format!("http://{addr}/mcp"))
        .header("Host", addr.to_string())
        .header("Origin", "http://localhost:3000")
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&init_body)
        .send()
        .await
        .expect("Request should send");
    assert_eq!(
        res_browser_origin.status(),
        reqwest::StatusCode::FORBIDDEN,
        "Any present Origin must be rejected with 403 Forbidden"
    );

    // 4b. Origin: null (e.g. sandboxed iframe or file://) is also rejected
    let res_null_origin = http_client
        .post(format!("http://{addr}/mcp"))
        .header("Host", addr.to_string())
        .header("Origin", "null")
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&init_body)
        .send()
        .await
        .expect("Request should send");
    assert_eq!(
        res_null_origin.status(),
        reqwest::StatusCode::FORBIDDEN,
        "Origin: null must be rejected with 403 Forbidden"
    );

    // 4c. OPTIONS preflight request is rejected and does not return CORS headers
    let res_options = http_client
        .request(reqwest::Method::OPTIONS, format!("http://{addr}/mcp"))
        .header("Host", addr.to_string())
        .send()
        .await
        .expect("Request should send");
    assert_eq!(
        res_options.status(),
        reqwest::StatusCode::METHOD_NOT_ALLOWED,
        "OPTIONS request must return 405 Method Not Allowed"
    );
    assert!(
        res_options
            .headers()
            .get("access-control-allow-origin")
            .is_none(),
        "No CORS headers should be returned on OPTIONS preflight"
    );

    // 5. Request without Origin proceeds to normal MCP validation (as verified in #1)
    ct.cancel();
    let _ = server_handle.await;
}

#[tokio::test]
async fn test_http_request_body_size_limit_rejection() {
    let config = Config::default();
    let server = NineRouterMcpServer::new(&config).unwrap();

    let ct = CancellationToken::new();
    let (addr, server_handle) = spawn_test_server(server, ct.clone()).await;

    let http_client = reqwest::Client::new();
    // Default limit is 4 MiB (4 * 1024 * 1024 bytes). Send a 5 MiB body.
    let oversized_body = vec![b'a'; 5 * 1024 * 1024];

    let res = http_client
        .post(format!("http://{addr}/mcp"))
        .header("Host", addr.to_string())
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(oversized_body)
        .send()
        .await
        .expect("Oversized POST request should send");

    assert_eq!(
        res.status(),
        reqwest::StatusCode::PAYLOAD_TOO_LARGE,
        "Payload exceeding 4 MiB body limit must receive 413 Payload Too Large"
    );

    ct.cancel();
    let _ = server_handle.await;
}

#[tokio::test]
async fn test_http_graceful_shutdown_via_cancellation_token() {
    let config = Config::default();
    let server = NineRouterMcpServer::new(&config).unwrap();

    let ct = CancellationToken::new();
    let (addr, server_handle) = spawn_test_server(server, ct.clone()).await;

    // Confirm server is responsive
    let http_client = reqwest::Client::new();
    let res = http_client
        .get(format!("http://{addr}/"))
        .send()
        .await
        .expect("Server must be running");
    assert_eq!(res.status(), reqwest::StatusCode::NOT_FOUND);

    // Cancel token
    ct.cancel();

    // Server should shut down cleanly
    let result = tokio::time::timeout(Duration::from_secs(5), server_handle).await;
    assert!(result.is_ok(), "Server should terminate within 5 seconds");
    let join_res = result.unwrap().expect("Task should join cleanly");
    assert!(join_res.is_ok(), "serve_with_listener should return Ok(())");

    // After shutdown, connections to the address must fail
    let after_shutdown = http_client.get(format!("http://{addr}/")).send().await;
    assert!(
        after_shutdown.is_err(),
        "Server must not accept connections after shutdown"
    );
}

#[tokio::test]
async fn test_http_bind_error_propagation_no_panic() {
    // Bind an ephemeral port first
    let listener = bind_http_listener(0).await.expect("Failed first bind");
    let bound_port = listener.local_addr().unwrap().port();

    // Attempting to bind the same port again must return AppError::ServerRuntime without panic
    let second_bind = bind_http_listener(bound_port).await;
    assert!(second_bind.is_err(), "Second bind on same port must fail");
    let err = second_bind.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("failed to bind HTTP listener on 127.0.0.1:"),
        "Error message did not match expected: {}",
        err_str
    );
    assert!(
        err_str.contains("address already in use") || err_str.contains("Address already in use"),
        "Error message did not indicate address in use: {}",
        err_str
    );

    drop(listener);
}
