use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use rmcp::handler::server::ServerHandler;
use rmcp::model::CallToolRequestParams;
use rmcp::ServiceExt;

use ninerouter_mcp_web::config::Config;
use ninerouter_mcp_web::server::NineRouterMcpServer;

#[tokio::test]
async fn test_server_handler_list_tools_exact_two() {
    let config = Config::default();
    let server = NineRouterMcpServer::new(&config).unwrap();

    let server_info = server.get_info();
    assert_eq!(server_info.server_info.name, "9router-mcp-web");
    assert_eq!(server_info.server_info.version, env!("CARGO_PKG_VERSION"));

    // In-memory duplex connection
    let (client_io, server_io) = tokio::io::duplex(8192);
    let (client_read, client_write) = tokio::io::split(client_io);
    let (server_read, server_write) = tokio::io::split(server_io);

    let server_handle = tokio::spawn(async move {
        let running = rmcp::service::serve_server(server, (server_read, server_write))
            .await
            .expect("Server init failed");
        let _ = running.waiting().await;
    });

    // Run client over duplex transport
    let client = ().serve((client_read, client_write)).await.expect("Client init failed");

    let tools = client
        .peer()
        .list_tools(Default::default())
        .await
        .expect("List tools failed");

    assert_eq!(tools.tools.len(), 2);
    let tool_names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
    assert!(tool_names.contains(&"web_search"));
    assert!(tool_names.contains(&"web_fetch"));

    // Verify tool annotations for both tools (A. Tool annotations)
    for tool in &tools.tools {
        assert!(
            tool.annotations.is_some(),
            "Tool {} missing annotations",
            tool.name
        );
        let annotations = tool.annotations.as_ref().unwrap();
        assert_eq!(
            annotations.read_only_hint,
            Some(true),
            "Tool {} read_only_hint must be true",
            tool.name
        );
        assert_eq!(
            annotations.open_world_hint,
            Some(true),
            "Tool {} open_world_hint must be true",
            tool.name
        );
        assert_eq!(
            annotations.destructive_hint, None,
            "Tool {} destructive_hint should not be set",
            tool.name
        );
        assert_eq!(
            annotations.idempotent_hint, None,
            "Tool {} idempotent_hint should not be set",
            tool.name
        );

        let serialized_annotations = serde_json::to_value(annotations).unwrap();
        assert_eq!(
            serialized_annotations.get("readOnlyHint"),
            Some(&json!(true))
        );
        assert_eq!(
            serialized_annotations.get("openWorldHint"),
            Some(&json!(true))
        );
        assert!(serialized_annotations.get("destructiveHint").is_none());
        assert!(serialized_annotations.get("idempotentHint").is_none());
    }

    // Verify schemas do NOT expose model, provider, or credentials (AC-03, AC-05)
    // Verify schemas do NOT expose model, provider, credentials, or baseUrl (AC-03, AC-05)
    for tool in &tools.tools {
        let schema_str = serde_json::to_string(&tool.input_schema).unwrap();
        assert!(
            !schema_str.contains("\"model\""),
            "Tool schema should not expose model: {}",
            tool.name
        );
        assert!(
            !schema_str.contains("\"provider\""),
            "Tool schema should not expose provider: {}",
            tool.name
        );
        assert!(
            !schema_str.contains("\"api_key\""),
            "Tool schema should not expose api_key: {}",
            tool.name
        );
        assert!(
            !schema_str.contains("\"baseUrl\""),
            "Tool schema should not expose baseUrl: {}",
            tool.name
        );
    }

    // Verify web_search schema restricts provider_options to verified options
    let search_tool = tools.tools.iter().find(|t| t.name == "web_search").unwrap();
    let search_schema_val = serde_json::to_value(&search_tool.input_schema).unwrap();
    let provider_opts_schema = &search_schema_val["properties"]["provider_options"];
    let opt_subschema =
        if let Some(any_of) = provider_opts_schema.get("anyOf").and_then(|a| a.as_array()) {
            any_of
                .iter()
                .find(|s| s.get("properties").is_some() || s.get("type") == Some(&json!("object")))
                .expect("provider_options anyOf must contain an object schema")
        } else {
            provider_opts_schema
        };
    assert_eq!(opt_subschema["additionalProperties"], json!(false));
    let opt_props = opt_subschema["properties"].as_object().unwrap();
    assert!(opt_props.contains_key("cx"));
    assert!(opt_props.contains_key("depth"));
    assert!(opt_props.contains_key("cursor"));
    assert!(opt_props.contains_key("queryType"));
    assert!(!opt_props.contains_key("baseUrl"));
    assert_eq!(opt_props.len(), 4);

    // Verify web_search schema describes domain_filter as an array of strings
    let domain_filter_schema = &search_schema_val["properties"]["domain_filter"];
    assert!(
        domain_filter_schema["type"] == "array"
            || domain_filter_schema["type"] == json!(["array", "null"])
            || domain_filter_schema.get("anyOf").is_some(),
        "MCP schema must describe domain_filter as array"
    );
    let items = domain_filter_schema
        .get("items")
        .or_else(|| {
            domain_filter_schema
                .get("anyOf")
                .and_then(|arr| arr.as_array())
                .and_then(|subschemas| subschemas.iter().find_map(|s| s.get("items")))
        })
        .expect("domain_filter schema must define items");
    assert_eq!(
        items["type"], "string",
        "domain_filter items must be string"
    );

    // Call web_search tool through client
    let args = json!({ "query": "test query" })
        .as_object()
        .unwrap()
        .clone();
    let search_call = client
        .peer()
        .call_tool(CallToolRequestParams::new("web_search").with_arguments(args))
        .await
        .expect("Search call failed");

    // Even if 9Router is offline (mock not mounted), server gracefully returns tool error
    assert_eq!(search_call.is_error, Some(true));

    server_handle.abort();
}

#[tokio::test]
async fn test_mcp_full_flow_with_mock_9router() {
    let mock_server = MockServer::start().await;

    let search_payload = json!({
        "results": [
            { "title": "WireMock Test", "url": "https://wiremock.org" }
        ]
    });
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&search_payload))
        .mount(&mock_server)
        .await;

    let fetch_payload = json!({
        "title": "WireMock Home",
        "content": "# WireMock\nFlexible API mocking",
        "url": "https://wiremock.org"
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

    // 1. Verify web_search returns structured content and compact JSON text content
    let search_args = json!({ "query": "find wiremock" })
        .as_object()
        .unwrap()
        .clone();
    let search_res = client
        .peer()
        .call_tool(CallToolRequestParams::new("web_search").with_arguments(search_args))
        .await
        .expect("Search call failed");

    assert_eq!(search_res.is_error, Some(false));
    assert!(search_res.structured_content.is_some());
    let search_structured = search_res.structured_content.unwrap();
    assert_eq!(search_structured["results"][0]["title"], "WireMock Test");

    assert_eq!(search_res.content.len(), 1);
    let search_text = search_res.content[0]
        .as_text()
        .expect("Expected text content block")
        .text
        .as_str();
    assert!(
        !search_text.contains('\n'),
        "search text content must be compact JSON"
    );
    let search_parsed: serde_json::Value = serde_json::from_str(search_text).unwrap();
    assert_eq!(search_parsed, search_structured);

    // 2. Verify web_fetch returns structured content and compact JSON text content
    let fetch_args = json!({ "url": "https://wiremock.org" })
        .as_object()
        .unwrap()
        .clone();
    let fetch_res = client
        .peer()
        .call_tool(CallToolRequestParams::new("web_fetch").with_arguments(fetch_args))
        .await
        .expect("Fetch call failed");

    assert_eq!(fetch_res.is_error, Some(false));
    assert!(fetch_res.structured_content.is_some());
    let fetch_structured = fetch_res.structured_content.unwrap();
    assert_eq!(fetch_structured["title"], "WireMock Home");

    assert_eq!(fetch_res.content.len(), 1);
    let fetch_text = fetch_res.content[0]
        .as_text()
        .expect("Expected text content block")
        .text
        .as_str();
    assert!(
        !fetch_text.contains('\n'),
        "fetch text content must be compact JSON"
    );
    let fetch_parsed: serde_json::Value = serde_json::from_str(fetch_text).unwrap();
    assert_eq!(fetch_parsed, fetch_structured);

    server_handle.abort();
}

#[tokio::test]
async fn test_mcp_full_flow_with_domain_filter_and_search_type() {
    let mock_server = MockServer::start().await;

    // Upstream 9Router must receive the domain_filter array preserving inclusions and exclusions,
    // and unrelated fields like search_type = "x" must not be regressed.
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .and(body_json(json!({
            "model": "search-combo",
            "query": "find wiremock",
            "search_type": "x",
            "domain_filter": ["github.com", "-reddit.com"]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [
                { "title": "WireMock Test", "url": "https://wiremock.org" }
            ]
        })))
        .expect(1)
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

    let args = json!({
        "query": "find wiremock",
        "search_type": "x",
        "domain_filter": ["github.com", "-reddit.com"]
    })
    .as_object()
    .unwrap()
    .clone();

    let search_res = client
        .peer()
        .call_tool(CallToolRequestParams::new("web_search").with_arguments(args))
        .await
        .expect("Search call failed");

    assert_eq!(search_res.is_error, Some(false));
    assert!(search_res.structured_content.is_some());
    assert!(!search_res.content.is_empty());

    server_handle.abort();
}

#[test]
fn test_mcp_server_runtime_error_distinct_from_network_unreachable() {
    use ninerouter_mcp_web::error::AppError;

    let runtime_err = AppError::ServerRuntime("task panicked".to_string());
    let msg = runtime_err.to_string();
    assert!(msg.contains("MCP server runtime error: task panicked"));
    assert!(!msg.contains("failed to connect to 9Router"));
    assert!(!msg.contains("Network error"));

    let net_err = AppError::NetworkUnreachable("http://localhost:20128".to_string());
    let net_msg = net_err.to_string();
    assert!(
        net_msg.contains("Network error: failed to connect to 9Router at http://localhost:20128")
    );
    assert!(!net_msg.contains("MCP server runtime error"));
}
