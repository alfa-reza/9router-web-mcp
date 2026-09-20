use std::fs;
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;
use ninerouter_mcp_web::config::{
    normalize_base_url, Config, DEFAULT_BASE_URL, DEFAULT_FETCH_COMBO, DEFAULT_SEARCH_COMBO,
    DEFAULT_TIMEOUT_SECS,
};

#[test]
fn test_default_config() {
    let cfg = Config::default();
    assert_eq!(cfg.base_url, DEFAULT_BASE_URL);
    assert_eq!(cfg.api_key, None);
    assert_eq!(cfg.search_combo, DEFAULT_SEARCH_COMBO);
    assert_eq!(cfg.fetch_combo, DEFAULT_FETCH_COMBO);
    assert_eq!(cfg.timeout_secs, DEFAULT_TIMEOUT_SECS);
}

#[test]
fn test_normalize_base_url() {
    assert_eq!(normalize_base_url("http://localhost:20128/"), "http://localhost:20128");
    assert_eq!(normalize_base_url("http://localhost:20128///"), "http://localhost:20128//");
    assert_eq!(normalize_base_url("https://example.com/api/v1/"), "https://example.com/api/v1");
    assert_eq!(normalize_base_url("https://example.com/api/v1"), "https://example.com/api/v1");
}

#[test]
fn test_save_and_load_config_file() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("9router-mcp-web").join("config.toml");

    let cfg = Config {
        base_url: "http://127.0.0.1:8080".to_string(),
        api_key: Some("sk-secret123456789".to_string()),
        search_combo: "my-search".to_string(),
        fetch_combo: "my-fetch".to_string(),
        timeout_secs: 45,
    };

    cfg.save(&config_path).expect("Failed to save config");

    // Check file permissions are 0600
    let metadata = fs::metadata(&config_path).expect("Metadata failed");
    let permissions = metadata.permissions();
    assert_eq!(permissions.mode() & 0o777, 0o600);

    // Check directory permissions are 0700
    let dir_metadata = fs::metadata(config_path.parent().unwrap()).expect("Parent metadata failed");
    assert_eq!(dir_metadata.permissions().mode() & 0o777, 0o700);

    // Load back and verify
    let loaded = Config::load_from_file(&config_path)
        .expect("Load failed")
        .expect("Config should exist");

    assert_eq!(loaded, cfg);
}

#[test]
fn test_load_legacy_ninerouter_table() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    let toml_content = r#"
[ninerouter]
base_url = "http://remote-server:20128/"
api_key = "sk-legacy-key"
search_combo = "remote-search"
fetch_combo = "remote-fetch"
timeout_secs = 60
"#;

    fs::write(&config_path, toml_content).unwrap();

    let loaded = Config::load_from_file(&config_path)
        .expect("Load failed")
        .expect("Config should exist");

    assert_eq!(loaded.base_url, "http://remote-server:20128");
    assert_eq!(loaded.api_key, Some("sk-legacy-key".to_string()));
    assert_eq!(loaded.search_combo, "remote-search");
    assert_eq!(loaded.fetch_combo, "remote-fetch");
    assert_eq!(loaded.timeout_secs, 60);
}

#[test]
fn test_plain_http_warning() {
    // 1. Local HTTP - no warning
    let local_cfg = Config {
        base_url: "http://localhost:20128".to_string(),
        api_key: Some("sk-12345".to_string()),
        ..Default::default()
    };
    assert!(local_cfg.check_plain_http_warning().is_none());

    let loopback_cfg = Config {
        base_url: "http://127.0.0.1:20128".to_string(),
        api_key: Some("sk-12345".to_string()),
        ..Default::default()
    };
    assert!(loopback_cfg.check_plain_http_warning().is_none());

    // 2. Remote HTTPS - no warning
    let remote_https = Config {
        base_url: "https://remote.9router.com".to_string(),
        api_key: Some("sk-12345".to_string()),
        ..Default::default()
    };
    assert!(remote_https.check_plain_http_warning().is_none());

    // 3. Remote HTTP without API key - no warning
    let remote_http_no_key = Config {
        base_url: "http://192.168.1.100:20128".to_string(),
        api_key: None,
        ..Default::default()
    };
    assert!(remote_http_no_key.check_plain_http_warning().is_none());

    // 4. Remote HTTP WITH API key - MUST trigger warning
    let remote_http_with_key = Config {
        base_url: "http://192.168.1.100:20128".to_string(),
        api_key: Some("sk-12345".to_string()),
        ..Default::default()
    };
    let warning = remote_http_with_key.check_plain_http_warning();
    assert!(warning.is_some());
    assert!(warning.unwrap().contains("WARNING: 9Router API key is configured over unencrypted plaintext HTTP"));
}

#[test]
fn test_masked_api_key() {
    let no_key = Config { api_key: None, ..Default::default() };
    assert_eq!(no_key.masked_api_key(), "(none)");

    let short_key = Config { api_key: Some("secret".to_string()), ..Default::default() };
    assert_eq!(short_key.masked_api_key(), "***");

    let long_key = Config { api_key: Some("sk-1234567890abcdef".to_string()), ..Default::default() };
    assert_eq!(long_key.masked_api_key(), "sk-...cdef");
}

#[test]
fn test_env_overrides_precedence() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    let file_cfg = Config {
        base_url: "http://file-host:20128".to_string(),
        api_key: Some("sk-file-key".to_string()),
        search_combo: "file-search".to_string(),
        fetch_combo: "file-fetch".to_string(),
        timeout_secs: 20,
    };
    file_cfg.save(&config_path).unwrap();

    // Set environment overrides
    std::env::set_var("NINEROUTER_URL", "http://env-host:20128/");
    std::env::set_var("NINEROUTER_KEY", "sk-env-key");
    std::env::set_var("NINEROUTER_SEARCH_COMBO", "env-search");
    std::env::set_var("NINEROUTER_FETCH_COMBO", "env-fetch");
    std::env::set_var("NINEROUTER_TIMEOUT_SECS", "50");

    let resolved = Config::resolve(Some(&config_path)).expect("Resolve failed");

    // Clear environment overrides
    std::env::remove_var("NINEROUTER_URL");
    std::env::remove_var("NINEROUTER_KEY");
    std::env::remove_var("NINEROUTER_SEARCH_COMBO");
    std::env::remove_var("NINEROUTER_FETCH_COMBO");
    std::env::remove_var("NINEROUTER_TIMEOUT_SECS");

    assert_eq!(resolved.base_url, "http://env-host:20128");
    assert_eq!(resolved.api_key, Some("sk-env-key".to_string()));
    assert_eq!(resolved.search_combo, "env-search");
    assert_eq!(resolved.fetch_combo, "env-fetch");
    assert_eq!(resolved.timeout_secs, 50);
}

