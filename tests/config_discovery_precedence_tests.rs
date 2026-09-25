use std::fs;
use tempfile::tempdir;

use ninerouter_mcp_web::config::{Config, FileConfig, PersistedConfig};
use ninerouter_mcp_web::discovery::DiscoveredInstance;

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    vars: Vec<&'static str>,
}

impl EnvGuard {
    fn new(vars: Vec<&'static str>) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for var in &vars {
            std::env::remove_var(var);
        }
        Self { _lock: lock, vars }
    }

    fn set(&mut self, key: &'static str, val: &str) {
        std::env::set_var(key, val);
        if !self.vars.contains(&key) {
            self.vars.push(key);
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for var in &self.vars {
            std::env::remove_var(var);
        }
    }
}

#[tokio::test]
async fn test_env_base_url_bypasses_discovery() {
    let mut guard = EnvGuard::new(vec!["NINEROUTER_URL", "NINEROUTER_KEY"]);
    guard.set("NINEROUTER_URL", "http://explicit-env-host:20128");

    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    let resolved = Config::resolve_with_discovery(Some(&config_path), async {
        panic!("Discovery must NOT run when explicit env Base URL is present!")
    })
    .await
    .expect("Resolve should succeed");

    assert_eq!(resolved.base_url, "http://explicit-env-host:20128");
}

#[tokio::test]
async fn test_file_base_url_bypasses_discovery() {
    let _guard = EnvGuard::new(vec!["NINEROUTER_URL", "NINEROUTER_BASE_URL"]);

    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    let persisted = PersistedConfig {
        base_url: Some("http://explicit-file-host:20128".to_string()),
        api_key: None,
        search_combo: "search-combo".to_string(),
        fetch_combo: "fetch-combo".to_string(),
        timeout_secs: 30,
    };
    persisted.save(&config_path).unwrap();

    let resolved = Config::resolve_with_discovery(Some(&config_path), async {
        panic!("Discovery must NOT run when explicit file Base URL is present!")
    })
    .await
    .expect("Resolve should succeed");

    assert_eq!(resolved.base_url, "http://explicit-file-host:20128");
}

#[tokio::test]
async fn test_invalid_config_file_returns_error_and_never_falls_back_to_discovery() {
    let _guard = EnvGuard::new(vec!["NINEROUTER_URL", "NINEROUTER_BASE_URL"]);

    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    // 1. Malformed TOML
    fs::write(&config_path, "not a valid toml [[[").unwrap();
    let res = Config::resolve_with_discovery(Some(&config_path), async {
        panic!("Discovery must NOT run when config file is malformed!")
    })
    .await;
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("Failed to parse config file"));

    // 2. Invalid Base URL (credentials in URL)
    fs::write(&config_path, r#"base_url = "http://user:pass@host:20128""#).unwrap();
    let res2 = Config::resolve_with_discovery(Some(&config_path), async {
        panic!("Discovery must NOT run when config file has invalid Base URL!")
    })
    .await;
    assert!(res2.is_err());
    assert!(res2
        .unwrap_err()
        .to_string()
        .contains("embedded credentials"));

    // 3. Invalid timeout (0)
    fs::write(&config_path, "timeout_secs = 0\n").unwrap();
    let res3 = Config::resolve_with_discovery(Some(&config_path), async {
        panic!("Discovery must NOT run when config file has invalid timeout!")
    })
    .await;
    assert!(res3.is_err());
    assert!(res3
        .unwrap_err()
        .to_string()
        .contains("timeout_secs must be greater than 0"));
}

#[tokio::test]
async fn test_valid_file_without_base_url_fills_base_url_from_discovery_and_retains_combos() {
    let _guard = EnvGuard::new(vec!["NINEROUTER_URL", "NINEROUTER_BASE_URL"]);

    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    // Valid file without base_url
    let toml_content = r#"
search_combo = "my-custom-search"
fetch_combo = "my-custom-fetch"
timeout_secs = 45
"#;
    fs::write(&config_path, toml_content).unwrap();

    let resolved = Config::resolve_with_discovery(Some(&config_path), async {
        Some(DiscoveredInstance {
            base_url: "http://127.0.0.1:2519".to_string(),
            is_keyless: true,
        })
    })
    .await
    .expect("Resolve should succeed");

    assert_eq!(resolved.base_url, "http://127.0.0.1:2519");
    assert_eq!(resolved.search_combo, "my-custom-search");
    assert_eq!(resolved.fetch_combo, "my-custom-fetch");
    assert_eq!(resolved.timeout_secs, 45);
    assert_eq!(resolved.api_key, None);
}

#[tokio::test]
async fn test_discovery_failure_returns_actionable_serve_error() {
    let _guard = EnvGuard::new(vec!["NINEROUTER_URL", "NINEROUTER_BASE_URL"]);

    let dir = tempdir().unwrap();
    let config_path = dir.path().join("nonexistent.toml");

    let err = Config::resolve_with_discovery(Some(&config_path), async { None })
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(
        msg.contains("No 9Router URL is configured and no local 9Router instance was detected"),
        "Expected missing 9Router URL message, got: {}",
        msg
    );
    assert!(
        msg.contains("9router-mcp-web configure"),
        "Expected configure command suggestion, got: {}",
        msg
    );
}

#[tokio::test]
async fn test_discovery_auth_required_without_key_returns_actionable_error() {
    let _guard = EnvGuard::new(vec![
        "NINEROUTER_URL",
        "NINEROUTER_KEY",
        "NINEROUTER_API_KEY",
    ]);

    let dir = tempdir().unwrap();
    let config_path = dir.path().join("nonexistent.toml");

    let err = Config::resolve_with_discovery(Some(&config_path), async {
        Some(DiscoveredInstance {
            base_url: "http://127.0.0.1:2519".to_string(),
            is_keyless: false,
        })
    })
    .await
    .unwrap_err();

    let msg = err.to_string();
    assert!(
        msg.contains("Local 9Router was detected at http://127.0.0.1:2519, but Web Search/Web Fetch require an API key"),
        "Expected auth required message with discovered URL, got: {}",
        msg
    );
    assert!(
        msg.contains("Set NINEROUTER_KEY or run `9router-mcp-web configure`"),
        "Expected key guidance, got: {}",
        msg
    );
}

#[tokio::test]
async fn test_discovery_auth_required_with_explicit_key_succeeds() {
    let mut guard = EnvGuard::new(vec!["NINEROUTER_URL", "NINEROUTER_KEY"]);
    guard.set("NINEROUTER_KEY", "sk-discovered-instance-key");

    let dir = tempdir().unwrap();
    let config_path = dir.path().join("nonexistent.toml");

    let resolved = Config::resolve_with_discovery(Some(&config_path), async {
        Some(DiscoveredInstance {
            base_url: "http://127.0.0.1:2519".to_string(),
            is_keyless: false,
        })
    })
    .await
    .expect("Resolve should succeed when key is provided");

    assert_eq!(resolved.base_url, "http://127.0.0.1:2519");
    assert_eq!(
        resolved.api_key.as_deref(),
        Some("sk-discovered-instance-key")
    );
}

#[test]
fn test_persisted_config_omits_base_url_when_none() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    let persisted = PersistedConfig {
        base_url: None,
        api_key: None,
        search_combo: "search-combo".to_string(),
        fetch_combo: "fetch-combo".to_string(),
        timeout_secs: 30,
    };
    persisted.save(&config_path).expect("Save should succeed");

    let content = fs::read_to_string(&config_path).unwrap();
    assert!(
        !content.contains("base_url"),
        "File should not contain base_url when omitted, got: {}",
        content
    );

    let loaded_fc = FileConfig::load(&config_path).unwrap().unwrap();
    assert_eq!(loaded_fc.base_url, None);
    assert_eq!(loaded_fc.search_combo.as_deref(), Some("search-combo"));
    assert_eq!(loaded_fc.fetch_combo.as_deref(), Some("fetch-combo"));
    assert_eq!(loaded_fc.timeout_secs, Some(30));
}

#[tokio::test]
async fn test_configure_flow_keyless_discovery() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    // Input: only 3 prompts: search combo, fetch combo, timeout
    let mut input = std::io::Cursor::new(b"discovered-search\ndiscovered-fetch\n45\n");
    let mut output = Vec::new();

    ninerouter_mcp_web::config::run_interactive_configure_with(
        Some(&config_path),
        &mut input,
        &mut output,
        async {
            Some(DiscoveredInstance {
                base_url: "http://127.0.0.1:20128".to_string(),
                is_keyless: true,
            })
        },
        false,
    )
    .await
    .expect("Configure should succeed");

    let out_str = String::from_utf8_lossy(&output);
    assert!(out_str.contains("Detected local 9Router at http://127.0.0.1:20128"));
    assert!(out_str.contains("API key is not required."));
    assert!(!out_str.contains("9Router URL (with or without /v1)"));
    assert!(!out_str.contains("9Router API key"));
    assert!(out_str.contains("Search combo name"));
    assert!(out_str.contains("Fetch combo name"));
    assert!(out_str.contains("Request timeout in seconds"));

    // Verify config file: base_url and api_key are omitted, custom combos and timeout saved
    let loaded = FileConfig::load(&config_path).unwrap().unwrap();
    assert_eq!(loaded.base_url, None);
    assert_eq!(loaded.api_key, None);
    assert_eq!(loaded.search_combo.as_deref(), Some("discovered-search"));
    assert_eq!(loaded.fetch_combo.as_deref(), Some("discovered-fetch"));
    assert_eq!(loaded.timeout_secs, Some(45));
}

#[tokio::test]
async fn test_configure_flow_auth_required_discovery() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    // Input: 4 prompts: api key, search combo, fetch combo, timeout
    let mut input = std::io::Cursor::new(b"sk-user-entered-key\ncustom-search\ncustom-fetch\n25\n");
    let mut output = Vec::new();

    ninerouter_mcp_web::config::run_interactive_configure_with(
        Some(&config_path),
        &mut input,
        &mut output,
        async {
            Some(DiscoveredInstance {
                base_url: "http://127.0.0.1:2519".to_string(),
                is_keyless: false,
            })
        },
        false,
    )
    .await
    .expect("Configure should succeed");

    let out_str = String::from_utf8_lossy(&output);
    assert!(out_str.contains("Detected local 9Router at http://127.0.0.1:2519"));
    assert!(!out_str.contains("API key is not required."));
    assert!(!out_str.contains("9Router URL (with or without /v1)"));
    assert!(out_str.contains("9Router API key"));
    assert!(out_str.contains("Search combo name"));
    assert!(out_str.contains("Fetch combo name"));
    assert!(out_str.contains("Request timeout in seconds"));

    // Verify config file: base_url is omitted, api_key is saved
    let loaded = FileConfig::load(&config_path).unwrap().unwrap();
    assert_eq!(loaded.base_url, None);
    assert_eq!(loaded.api_key.as_deref(), Some("sk-user-entered-key"));
    assert_eq!(loaded.search_combo.as_deref(), Some("custom-search"));
    assert_eq!(loaded.fetch_combo.as_deref(), Some("custom-fetch"));
    assert_eq!(loaded.timeout_secs, Some(25));
}

#[tokio::test]
async fn test_configure_flow_discovery_failure_uses_manual_flow() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    // Input: 5 prompts: base url, api key, search combo, fetch combo, timeout
    let mut input = std::io::Cursor::new(
        b"http://manual-server:20128\nsk-manual-token\nmanual-search\nmanual-fetch\n60\n",
    );
    let mut output = Vec::new();

    ninerouter_mcp_web::config::run_interactive_configure_with(
        Some(&config_path),
        &mut input,
        &mut output,
        async { None },
        false,
    )
    .await
    .expect("Configure should succeed");

    let out_str = String::from_utf8_lossy(&output);
    assert!(out_str.contains("9Router URL (with or without /v1)"));
    assert!(out_str.contains("9Router API key"));
    assert!(out_str.contains("Search combo name"));
    assert!(out_str.contains("Fetch combo name"));
    assert!(out_str.contains("Request timeout in seconds"));

    // Verify config file: base_url and api_key are explicitly saved
    let loaded = FileConfig::load(&config_path).unwrap().unwrap();
    assert_eq!(
        loaded.base_url.as_deref(),
        Some("http://manual-server:20128")
    );
    assert_eq!(loaded.api_key.as_deref(), Some("sk-manual-token"));
    assert_eq!(loaded.search_combo.as_deref(), Some("manual-search"));
    assert_eq!(loaded.fetch_combo.as_deref(), Some("manual-fetch"));
    assert_eq!(loaded.timeout_secs, Some(60));
}
