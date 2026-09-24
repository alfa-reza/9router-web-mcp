use serde_json::json;
use wiremock::matchers::{method, path};
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

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [
                { "title": "WireMock Test", "url": "https://wiremock.org" }
            ]
        })))
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

    let args = json!({ "query": "find wiremock" })
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
