use std::ffi::OsString;
use std::fs;
use tempfile::tempdir;
use wiremock::matchers::{body_string, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ninerouter_mcp_web::discovery::{
    check_health, detect_keyless, discover_local_9router_with, discovery_http_client,
    extract_custom_port_from_cmdline, is_binary_on_path_in, probe_endpoint_auth, AuthProbeResult,
    DEFAULT_LOCAL_PORT,
};

#[tokio::test]
async fn test_health_check_expected_ok() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "ok": true })))
        .mount(&mock_server)
        .await;

    let client = discovery_http_client().unwrap();
    let is_healthy = check_health(&client, &mock_server.uri()).await;
    assert!(is_healthy, "Expected health check to pass for ok: true");
}

#[tokio::test]
async fn test_health_check_rejections() {
    let client = discovery_http_client().unwrap();

    // 1. Wrong JSON body: {"ok": false}
    let server_false = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "ok": false })))
        .mount(&server_false)
        .await;
    assert!(!check_health(&client, &server_false.uri()).await);

    // 2. Wrong JSON field: {"status": "ok"}
    let server_field = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "status": "ok" })),
        )
        .mount(&server_field)
        .await;
    assert!(!check_health(&client, &server_field.uri()).await);

    // 3. HTTP 404
    let server_404 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server_404)
        .await;
    assert!(!check_health(&client, &server_404.uri()).await);

    // 4. HTTP 500
    let server_500 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server_500)
        .await;
    assert!(!check_health(&client, &server_500.uri()).await);

    // 5. Unreachable port
    assert!(!check_health(&client, "http://127.0.0.1:59999").await);
}

#[tokio::test]
async fn test_health_check_redirect_is_not_followed() {
    let mock_server = MockServer::start().await;

    // A redirect response (302) pointing elsewhere
    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", "http://malicious.example.com/api/health"),
        )
        .mount(&mock_server)
        .await;

    let client = discovery_http_client().unwrap();
    // Because redirect policy is none, 302 status is not 200, so health check returns false
    assert!(!check_health(&client, &mock_server.uri()).await);
}

#[tokio::test]
async fn test_keyless_probe_both_pass() {
    let mock_server = MockServer::start().await;

    let expected_missing_provider = serde_json::json!({
        "error": {
            "message": "Missing required field: provider (or model)",
            "type": "invalid_request_error",
            "code": ""
        }
    });

    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .and(body_string("{}"))
        .respond_with(ResponseTemplate::new(400).set_body_json(expected_missing_provider.clone()))
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/web/fetch"))
        .and(body_string("{}"))
        .respond_with(ResponseTemplate::new(400).set_body_json(expected_missing_provider))
        .mount(&mock_server)
        .await;

    let client = discovery_http_client().unwrap();
    let keyless = detect_keyless(&client, &mock_server.uri()).await;
    assert!(
        keyless,
        "Both endpoints returned missing provider validation 400 -> keyless must be true"
    );
}

#[tokio::test]
async fn test_keyless_probe_auth_required_scenarios() {
    let client = discovery_http_client().unwrap();

    let missing_provider_body = serde_json::json!({
        "error": {
            "message": "Missing required field: provider (or model)"
        }
    });

    let auth_error_body = serde_json::json!({
        "error": {
            "message": "Missing API key"
        }
    });

    // 1. Both endpoints return 401
    let server_401 = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(401).set_body_json(auth_error_body.clone()))
        .mount(&server_401)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/web/fetch"))
        .respond_with(ResponseTemplate::new(401).set_body_json(auth_error_body.clone()))
        .mount(&server_401)
        .await;
    assert!(!detect_keyless(&client, &server_401.uri()).await);

    // 2. Search is keyless (400), but Fetch requires auth (401)
    let server_fetch_auth = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(400).set_body_json(missing_provider_body.clone()))
        .mount(&server_fetch_auth)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/web/fetch"))
        .respond_with(ResponseTemplate::new(401).set_body_json(auth_error_body.clone()))
        .mount(&server_fetch_auth)
        .await;
    assert!(!detect_keyless(&client, &server_fetch_auth.uri()).await);

    // 3. Search requires auth (403), Fetch is keyless (400)
    let server_search_auth = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(403).set_body_json(auth_error_body.clone()))
        .mount(&server_search_auth)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/web/fetch"))
        .respond_with(ResponseTemplate::new(400).set_body_json(missing_provider_body.clone()))
        .mount(&server_search_auth)
        .await;
    assert!(!detect_keyless(&client, &server_search_auth.uri()).await);

    // 4. Arbitrary unexpected 400 error message (e.g. from upstream gateway)
    let server_unexpected_400 = MockServer::start().await;
    let arbitrary_400_body = serde_json::json!({
        "error": {
            "message": "Invalid request: malformed query parameter"
        }
    });
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(400).set_body_json(arbitrary_400_body.clone()))
        .mount(&server_unexpected_400)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/web/fetch"))
        .respond_with(ResponseTemplate::new(400).set_body_json(missing_provider_body.clone()))
        .mount(&server_unexpected_400)
        .await;
    assert!(
        !detect_keyless(&client, &server_unexpected_400.uri()).await,
        "Arbitrary 400 must NOT be classified as keyless"
    );
}

#[test]
fn test_probe_endpoint_auth_unit_statuses() {
    // Individual probe endpoint result classification
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mock_server = MockServer::start().await;
        let client = discovery_http_client().unwrap();

        // 401
        Mock::given(method("POST"))
            .and(path("/test-401"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&mock_server)
            .await;
        assert_eq!(
            probe_endpoint_auth(&client, &format!("{}/test-401", mock_server.uri())).await,
            AuthProbeResult::AuthRequired
        );

        // 403
        Mock::given(method("POST"))
            .and(path("/test-403"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&mock_server)
            .await;
        assert_eq!(
            probe_endpoint_auth(&client, &format!("{}/test-403", mock_server.uri())).await,
            AuthProbeResult::AuthRequired
        );

        // 400 with expected message
        Mock::given(method("POST"))
            .and(path("/test-keyless"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": { "message": "Missing required field: provider (or model)" }
            })))
            .mount(&mock_server)
            .await;
        assert_eq!(
            probe_endpoint_auth(&client, &format!("{}/test-keyless", mock_server.uri())).await,
            AuthProbeResult::Keyless
        );

        // 400 with unexpected message
        Mock::given(method("POST"))
            .and(path("/test-other-400"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": { "message": "Some other error" }
            })))
            .mount(&mock_server)
            .await;
        assert_eq!(
            probe_endpoint_auth(&client, &format!("{}/test-other-400", mock_server.uri())).await,
            AuthProbeResult::Ambiguous
        );

        // 500
        Mock::given(method("POST"))
            .and(path("/test-500"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;
        assert_eq!(
            probe_endpoint_auth(&client, &format!("{}/test-500", mock_server.uri())).await,
            AuthProbeResult::Ambiguous
        );
    });
}

#[test]
fn test_is_binary_on_path() {
    let dir = tempdir().unwrap();
    let bin_path = dir.path().join("9router");

    // 1. Binary does not exist
    let mut custom_path = OsString::new();
    custom_path.push(dir.path().as_os_str());
    assert!(!is_binary_on_path_in("9router", Some(&custom_path)));

    // 2. Binary exists but not executable (mode 0644)
    fs::write(&bin_path, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bin_path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!is_binary_on_path_in("9router", Some(&custom_path)));
    }

    // 3. Binary exists and is executable (mode 0755)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bin_path, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_binary_on_path_in("9router", Some(&custom_path)));
    }

    // 4. Binary in second PATH directory
    let dir2 = tempdir().unwrap();
    let bin2_path = dir2.path().join("9router");
    fs::write(&bin2_path, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bin2_path, fs::Permissions::from_mode(0o755)).unwrap();

        let empty_dir = tempdir().unwrap();
        let joined_path = std::env::join_paths([empty_dir.path(), dir2.path()]).unwrap();
        assert!(is_binary_on_path_in("9router", Some(&joined_path)));
    }

    // 5. None PATH
    assert!(!is_binary_on_path_in("9router", None));
}

#[test]
fn test_extract_custom_port_from_cmdline_cases() {
    // 1. Direct binary with --port <port>
    assert_eq!(
        extract_custom_port_from_cmdline(&["9router", "--port", "2519"]),
        Some(2519)
    );
    assert_eq!(
        extract_custom_port_from_cmdline(&["/usr/local/bin/9router", "--port", "2519"]),
        Some(2519)
    );

    // 2. Direct binary with -p <port>
    assert_eq!(
        extract_custom_port_from_cmdline(&["9router", "-p", "2519"]),
        Some(2519)
    );
    assert_eq!(
        extract_custom_port_from_cmdline(&["/home/user/.nvm/bin/9router", "-p", "3000"]),
        Some(3000)
    );

    // 3. Direct binary with equals format: --port=2519 and -p=2519
    assert_eq!(
        extract_custom_port_from_cmdline(&["9router", "--port=2519"]),
        Some(2519)
    );
    assert_eq!(
        extract_custom_port_from_cmdline(&["9router", "-p=2519"]),
        Some(2519)
    );

    // 4. Node launcher: node .../9router/cli/cli.js -p 2519
    assert_eq!(
        extract_custom_port_from_cmdline(&[
            "node",
            "/usr/local/lib/node_modules/9router/cli/cli.js",
            "-p",
            "2519"
        ]),
        Some(2519)
    );

    // 5. Node launcher background tray: node --flags .../9router/cli/cli.js --tray -p 2519
    assert_eq!(
        extract_custom_port_from_cmdline(&[
            "node",
            "--dns-result-order=ipv4first",
            "/home/user/.9router/cli.js",
            "--tray",
            "--skip-update",
            "-p",
            "2519"
        ]),
        Some(2519)
    );

    // 6. Host handling: loopback and wildcard bind hosts allowed
    assert_eq!(
        extract_custom_port_from_cmdline(&["9router", "-p", "2519", "-H", "0.0.0.0"]),
        Some(2519)
    );
    assert_eq!(
        extract_custom_port_from_cmdline(&["9router", "-p", "2519", "--host", "127.0.0.1"]),
        Some(2519)
    );
    assert_eq!(
        extract_custom_port_from_cmdline(&["9router", "-p", "2519", "-H=localhost"]),
        Some(2519)
    );

    // 7. Non-loopback host rejected (must return None to avoid auto-adopting remote/LAN binds)
    assert_eq!(
        extract_custom_port_from_cmdline(&["9router", "-p", "2519", "-H", "192.168.1.100"]),
        None
    );
    assert_eq!(
        extract_custom_port_from_cmdline(&["9router", "-p", "2519", "--host=10.0.0.5"]),
        None
    );

    // 8. Default port (20128) must return None (already covered by Step 1)
    assert_eq!(
        extract_custom_port_from_cmdline(&["9router", "-p", &DEFAULT_LOCAL_PORT.to_string()]),
        None
    );
    assert_eq!(extract_custom_port_from_cmdline(&["9router"]), None);

    // 9. Unrelated processes must be ignored
    assert_eq!(
        extract_custom_port_from_cmdline(&["grep", "9router", "-p", "2519"]),
        None
    );
    assert_eq!(
        extract_custom_port_from_cmdline(&["cargo", "test", "9router", "--port", "2519"]),
        None
    );
    assert_eq!(
        extract_custom_port_from_cmdline(&["vim", "/path/to/9router.txt", "-p", "2519"]),
        None
    );
    assert_eq!(
        extract_custom_port_from_cmdline(&["node", "server.js", "--port", "2519"]),
        None
    );

    // 10. 9router-mcp-web itself must be ignored
    assert_eq!(
        extract_custom_port_from_cmdline(&["9router-mcp-web", "serve", "-p", "2519"]),
        None
    );
    assert_eq!(
        extract_custom_port_from_cmdline(&["/usr/bin/9router-mcp-web", "-p", "2519"]),
        None
    );

    // 11. Malformed process data must not panic
    assert_eq!(extract_custom_port_from_cmdline::<&str>(&[]), None);
    assert_eq!(extract_custom_port_from_cmdline(&[""]), None);
    assert_eq!(extract_custom_port_from_cmdline(&["9router", "-p"]), None);
    assert_eq!(
        extract_custom_port_from_cmdline(&["9router", "-p", "not-a-port"]),
        None
    );
    assert_eq!(
        extract_custom_port_from_cmdline(&["9router", "-p", "0"]),
        None
    );
    assert_eq!(
        extract_custom_port_from_cmdline(&["9router", "-p", "-5"]),
        None
    );
}

#[tokio::test]
async fn test_full_discovery_pipeline() {
    let client = discovery_http_client().unwrap();

    let expected_missing_provider = serde_json::json!({
        "error": { "message": "Missing required field: provider (or model)" }
    });

    // Pipeline Case 1: Step 1 succeeds on default URL -> keyless detected
    let default_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "ok": true })))
        .mount(&default_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(400).set_body_json(expected_missing_provider.clone()))
        .mount(&default_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/web/fetch"))
        .respond_with(ResponseTemplate::new(400).set_body_json(expected_missing_provider.clone()))
        .mount(&default_server)
        .await;

    let res = discover_local_9router_with(&client, &default_server.uri(), false, || None).await;
    assert!(res.is_some());
    let disc = res.unwrap();
    assert_eq!(disc.base_url, default_server.uri());
    assert!(disc.is_keyless);

    // Pipeline Case 2: Step 1 fails, binary NOT installed -> stops immediately, returns None
    let res =
        discover_local_9router_with(&client, "http://127.0.0.1:59998", false, || Some(2519)).await;
    assert!(
        res.is_none(),
        "When binary is not installed, discovery must stop"
    );

    // Pipeline Case 3: Step 1 fails, binary installed, but no running process found -> returns None
    let res = discover_local_9router_with(&client, "http://127.0.0.1:59998", true, || None).await;
    assert!(
        res.is_none(),
        "When no process with custom port is found, discovery must stop"
    );

    // Pipeline Case 4: Step 1 fails, binary installed, running process found with custom port, health fails -> returns None
    let res =
        discover_local_9router_with(&client, "http://127.0.0.1:59998", true, || Some(59997)).await;
    assert!(
        res.is_none(),
        "When custom port health fails, discovery must stop"
    );

    // Pipeline Case 5: Step 1 fails, binary installed, running process with custom port, health succeeds, auth required
    // (mock server on custom port)
    let custom_server = MockServer::start().await;
    let custom_port = custom_server.address().port();

    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "ok": true })))
        .mount(&custom_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(serde_json::json!({ "error": "Missing key" })),
        )
        .mount(&custom_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/web/fetch"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(serde_json::json!({ "error": "Missing key" })),
        )
        .mount(&custom_server)
        .await;

    let res = discover_local_9router_with(&client, "http://127.0.0.1:59998", true, || {
        Some(custom_port)
    })
    .await;
    assert!(res.is_some());
    let disc = res.unwrap();
    assert_eq!(disc.base_url, format!("http://127.0.0.1:{}", custom_port));
    assert!(
        !disc.is_keyless,
        "Auth required should report is_keyless: false"
    );
}
