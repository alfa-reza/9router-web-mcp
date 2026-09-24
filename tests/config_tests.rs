use ninerouter_mcp_web::config::{
    parse_config_arg, validate_and_normalize_base_url, Config, DEFAULT_BASE_URL,
    DEFAULT_FETCH_COMBO, DEFAULT_SEARCH_COMBO, DEFAULT_TIMEOUT_SECS,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tempfile::tempdir;

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
    assert!(warning
        .unwrap()
        .contains("WARNING: 9Router API key is configured over unencrypted plaintext HTTP"));
}

#[test]
fn test_masked_api_key() {
    let no_key = Config {
        api_key: None,
        ..Default::default()
    };
    assert_eq!(no_key.masked_api_key(), "(none)");

    let short_key = Config {
        api_key: Some("secret".to_string()),
        ..Default::default()
    };
    assert_eq!(short_key.masked_api_key(), "***");

    let long_key = Config {
        api_key: Some("sk-1234567890abcdef".to_string()),
        ..Default::default()
    };
    assert_eq!(long_key.masked_api_key(), "sk-...cdef");

    // Multibyte UTF-8 tests (avoid byte-slice panics)
    let multibyte_short = Config {
        api_key: Some("🔑秘密キー".to_string()), // 6 characters
        ..Default::default()
    };
    assert_eq!(multibyte_short.masked_api_key(), "***");

    let multibyte_long = Config {
        api_key: Some("🔑秘密APIキー12345".to_string()), // 14 characters
        ..Default::default()
    };
    assert_eq!(multibyte_long.masked_api_key(), "🔑秘密...2345");
}

#[test]
fn test_save_permissions_existing_parent_preserved() {
    let dir = tempdir().unwrap();
    let parent = dir.path().join("existing_dir");
    fs::create_dir_all(&parent).unwrap();

    // Set existing parent permissions to 0755
    let perms = fs::Permissions::from_mode(0o755);
    fs::set_permissions(&parent, perms).unwrap();

    let config_path = parent.join("config.toml");
    let cfg = Config::default();
    cfg.save(&config_path).expect("Failed to save config");

    // Existing parent permissions must remain 0755 (not overwritten to 0700)
    let parent_meta = fs::metadata(&parent).unwrap();
    assert_eq!(parent_meta.permissions().mode() & 0o777, 0o755);

    // File permissions must still be 0600
    let file_meta = fs::metadata(&config_path).unwrap();
    assert_eq!(file_meta.permissions().mode() & 0o777, 0o600);
}

#[test]
fn test_validate_and_normalize_base_url_detailed() {
    // Valid cases
    assert_eq!(
        validate_and_normalize_base_url("http://localhost:20128").unwrap(),
        "http://localhost:20128"
    );
    assert_eq!(
        validate_and_normalize_base_url("http://localhost:20128/").unwrap(),
        "http://localhost:20128"
    );
    assert_eq!(
        validate_and_normalize_base_url("http://localhost:20128///").unwrap(),
        "http://localhost:20128"
    );
    assert_eq!(
        validate_and_normalize_base_url("https://api.example.com/v1///").unwrap(),
        "https://api.example.com/v1"
    );
    assert_eq!(
        validate_and_normalize_base_url("https://example.com/api/v1").unwrap(),
        "https://example.com/api/v1"
    );

    // Invalid scheme
    assert!(validate_and_normalize_base_url("ftp://localhost:20128").is_err());
    assert!(validate_and_normalize_base_url("ws://localhost:20128").is_err());

    // Invalid / missing host or malformed URL
    assert!(validate_and_normalize_base_url("not-a-url").is_err());
    assert!(validate_and_normalize_base_url("http://").is_err());
    assert!(validate_and_normalize_base_url("http://:8080").is_err());

    // Query params or fragments
    assert!(validate_and_normalize_base_url("http://localhost:20128?foo=bar").is_err());
    assert!(validate_and_normalize_base_url("http://localhost:20128#section").is_err());

    // Empty / whitespace
    assert!(validate_and_normalize_base_url("").is_err());
    assert!(validate_and_normalize_base_url("   ").is_err());
}

#[test]
fn test_resolve_path_precedence() {
    let mut guard = EnvGuard::new(vec!["NINEROUTER_CONFIG"]);
    let dir = tempdir().unwrap();
    let cli_path = dir.path().join("cli_config.toml");
    let env_path = dir.path().join("env_config.toml");

    // 1. CLI override takes highest precedence
    guard.set("NINEROUTER_CONFIG", env_path.to_str().unwrap());
    let resolved = Config::resolve_path(Some(&cli_path)).unwrap();
    assert_eq!(resolved, cli_path);

    // 2. Empty CLI override returns error
    let empty_cli = PathBuf::from("  ");
    assert!(Config::resolve_path(Some(&empty_cli)).is_err());

    // 3. Fallback to NINEROUTER_CONFIG if no CLI override
    let resolved_env = Config::resolve_path(None).unwrap();
    assert_eq!(resolved_env, env_path);

    // 4. Default path when neither is provided
    std::env::remove_var("NINEROUTER_CONFIG");
    let resolved_default = Config::resolve_path(None).unwrap();
    assert!(resolved_default.ends_with("9router-mcp-web/config.toml"));
}

#[test]
fn test_parse_config_arg() {
    // --config <path>
    let args = vec!["prog".into(), "--config".into(), "/my/config.toml".into()];
    assert_eq!(
        parse_config_arg(&args).unwrap(),
        Some(PathBuf::from("/my/config.toml"))
    );

    // -c <path>
    let args = vec!["prog".into(), "-c".into(), "/my/config.toml".into()];
    assert_eq!(
        parse_config_arg(&args).unwrap(),
        Some(PathBuf::from("/my/config.toml"))
    );

    // --config=<path>
    let args = vec!["prog".into(), "--config=/my/config.toml".into()];
    assert_eq!(
        parse_config_arg(&args).unwrap(),
        Some(PathBuf::from("/my/config.toml"))
    );

    // Missing value after --config
    let args = vec!["prog".into(), "--config".into()];
    assert!(parse_config_arg(&args).is_err());

    // Missing value after -c
    let args = vec!["prog".into(), "-c".into()];
    assert!(parse_config_arg(&args).is_err());

    // Empty value for --config
    let args = vec!["prog".into(), "--config".into(), "".into()];
    assert!(parse_config_arg(&args).is_err());

    // Empty value for --config=
    let args = vec!["prog".into(), "--config=".into()];
    assert!(parse_config_arg(&args).is_err());

    // No config argument present
    let args = vec!["prog".into(), "--help".into()];
    assert_eq!(parse_config_arg(&args).unwrap(), None);

    // --config followed by option flag must not treat flag as path
    let args = vec!["prog".into(), "--config".into(), "--help".into()];
    assert!(parse_config_arg(&args).is_err());

    let args = vec!["prog".into(), "-c".into(), "--help".into()];
    assert!(parse_config_arg(&args).is_err());

    // Order invariance between command and --config
    let args = vec![
        "prog".into(),
        "configure".into(),
        "--config".into(),
        "/my/config.toml".into(),
    ];
    assert_eq!(
        parse_config_arg(&args).unwrap(),
        Some(PathBuf::from("/my/config.toml"))
    );

    let args = vec![
        "prog".into(),
        "--config".into(),
        "/my/config.toml".into(),
        "configure".into(),
    ];
    assert_eq!(
        parse_config_arg(&args).unwrap(),
        Some(PathBuf::from("/my/config.toml"))
    );
}

#[test]
fn test_timeout_zero_rejected() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    fs::write(&config_path, "timeout_secs = 0\n").unwrap();

    let loaded = Config::load_from_file(&config_path);
    assert!(loaded.is_err());
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

    let mut guard = EnvGuard::new(vec![
        "NINEROUTER_URL",
        "NINEROUTER_KEY",
        "NINEROUTER_SEARCH_COMBO",
        "NINEROUTER_FETCH_COMBO",
        "NINEROUTER_TIMEOUT_SECS",
    ]);

    // Set environment overrides
    guard.set("NINEROUTER_URL", "http://env-host:20128/");
    guard.set("NINEROUTER_KEY", "sk-env-key");
    guard.set("NINEROUTER_SEARCH_COMBO", "env-search");
    guard.set("NINEROUTER_FETCH_COMBO", "env-fetch");
    guard.set("NINEROUTER_TIMEOUT_SECS", "50");

    let resolved = Config::resolve(Some(&config_path)).expect("Resolve failed");

    assert_eq!(resolved.base_url, "http://env-host:20128");
    assert_eq!(resolved.api_key, Some("sk-env-key".to_string()));
    assert_eq!(resolved.search_combo, "env-search");
    assert_eq!(resolved.fetch_combo, "env-fetch");
    assert_eq!(resolved.timeout_secs, 50);
}

#[test]
fn test_save_preexisting_unrelated_temp_file_unchanged() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let unrelated_tmp = dir.path().join("config.tmp");
    let unrelated_dot_tmp = dir.path().join(".config.tmp");

    fs::write(&unrelated_tmp, "UNRELATED_TEMP_DATA_PRESERVED").unwrap();
    fs::write(&unrelated_dot_tmp, "UNRELATED_DOT_TMP_DATA_PRESERVED").unwrap();

    let cfg = Config {
        base_url: "http://127.0.0.1:20128".to_string(),
        api_key: Some("sk-test-key".to_string()),
        search_combo: "test-search".to_string(),
        fetch_combo: "test-fetch".to_string(),
        timeout_secs: 15,
    };

    cfg.save(&config_path).expect("Save should succeed");

    // Verify unrelated temporary files remain untouched
    let tmp_content = fs::read_to_string(&unrelated_tmp).unwrap();
    assert_eq!(tmp_content, "UNRELATED_TEMP_DATA_PRESERVED");

    let dot_tmp_content = fs::read_to_string(&unrelated_dot_tmp).unwrap();
    assert_eq!(dot_tmp_content, "UNRELATED_DOT_TMP_DATA_PRESERVED");

    // Verify config file was correctly written and loaded
    let loaded = Config::load_from_file(&config_path)
        .expect("Load should succeed")
        .expect("Config should exist");
    assert_eq!(loaded, cfg);

    // Verify private permissions on saved config
    let metadata = fs::metadata(&config_path).unwrap();
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
}

#[test]
#[cfg(unix)]
fn test_save_temp_path_symlink_cannot_modify_target() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let target_file = dir.path().join("sensitive_target.txt");
    let symlink_tmp = dir.path().join("config.tmp");
    let dot_symlink_tmp = dir.path().join(".config.tmp");

    fs::write(&target_file, "SENSITIVE_ORIGINAL_CONTENT").unwrap();
    std::os::unix::fs::symlink(&target_file, &symlink_tmp).unwrap();
    std::os::unix::fs::symlink(&target_file, &dot_symlink_tmp).unwrap();

    let cfg = Config {
        base_url: "http://127.0.0.1:20128".to_string(),
        api_key: Some("sk-secret-token".to_string()),
        search_combo: "search-model".to_string(),
        fetch_combo: "fetch-model".to_string(),
        timeout_secs: 30,
    };

    cfg.save(&config_path).expect("Save should succeed");

    // Verify the target of the symlink was NOT overwritten or modified
    let target_content = fs::read_to_string(&target_file).unwrap();
    assert_eq!(target_content, "SENSITIVE_ORIGINAL_CONTENT");

    // Verify symlinks still point to the target file
    assert_eq!(fs::read_link(&symlink_tmp).unwrap(), target_file);
    assert_eq!(fs::read_link(&dot_symlink_tmp).unwrap(), target_file);

    // Verify config file was properly written
    let loaded = Config::load_from_file(&config_path)
        .expect("Load should succeed")
        .expect("Config should exist");
    assert_eq!(loaded, cfg);
}

#[test]
fn test_save_existing_config_replacement_succeeds() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    let initial_cfg = Config {
        base_url: "http://initial:20128".to_string(),
        api_key: Some("sk-initial".to_string()),
        search_combo: "initial-search".to_string(),
        fetch_combo: "initial-fetch".to_string(),
        timeout_secs: 10,
    };
    initial_cfg.save(&config_path).unwrap();

    let updated_cfg = Config {
        base_url: "http://updated:20128".to_string(),
        api_key: Some("sk-updated".to_string()),
        search_combo: "updated-search".to_string(),
        fetch_combo: "updated-fetch".to_string(),
        timeout_secs: 25,
    };
    updated_cfg.save(&config_path).unwrap();

    let loaded = Config::load_from_file(&config_path)
        .expect("Load failed")
        .expect("Config should exist");
    assert_eq!(loaded, updated_cfg);

    let metadata = fs::metadata(&config_path).unwrap();
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
}

#[test]
fn test_empty_or_whitespace_persisted_search_combo_rejected() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    // Whitespace-only search_combo
    fs::write(&config_path, "search_combo = \"   \"\n").unwrap();
    let err = Config::load_from_file(&config_path).unwrap_err();
    assert!(err
        .to_string()
        .contains("search_combo cannot be empty or whitespace-only"));

    // Empty string search_combo
    fs::write(&config_path, "search_combo = \"\"\n").unwrap();
    let err = Config::load_from_file(&config_path).unwrap_err();
    assert!(err
        .to_string()
        .contains("search_combo cannot be empty or whitespace-only"));

    // Legacy table whitespace search_combo
    let legacy_toml = "[ninerouter]\nsearch_combo = \"  \"\n";
    fs::write(&config_path, legacy_toml).unwrap();
    let err = Config::load_from_file(&config_path).unwrap_err();
    assert!(err
        .to_string()
        .contains("search_combo cannot be empty or whitespace-only"));
}

#[test]
fn test_empty_or_whitespace_persisted_fetch_combo_rejected() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    // Whitespace-only fetch_combo
    fs::write(&config_path, "fetch_combo = \" \t \"\n").unwrap();
    let err = Config::load_from_file(&config_path).unwrap_err();
    assert!(err
        .to_string()
        .contains("fetch_combo cannot be empty or whitespace-only"));

    // Empty string fetch_combo
    fs::write(&config_path, "fetch_combo = \"\"\n").unwrap();
    let err = Config::load_from_file(&config_path).unwrap_err();
    assert!(err
        .to_string()
        .contains("fetch_combo cannot be empty or whitespace-only"));

    // Legacy table whitespace fetch_combo
    let legacy_toml = "[ninerouter]\nfetch_combo = \"  \"\n";
    fs::write(&config_path, legacy_toml).unwrap();
    let err = Config::load_from_file(&config_path).unwrap_err();
    assert!(err
        .to_string()
        .contains("fetch_combo cannot be empty or whitespace-only"));
}

#[test]
fn test_credentials_in_base_url_rejected_and_not_leaked() {
    // Both user and password
    let err =
        validate_and_normalize_base_url("https://user:secret123@router.example.com").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("embedded credentials"));
    assert!(!msg.contains("secret123"));

    // Username only
    let err = validate_and_normalize_base_url("http://admin@localhost:20128").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("embedded credentials"));
    assert!(!msg.contains("admin"));

    // Password only
    let err = validate_and_normalize_base_url("http://:onlypass@localhost:20128").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("embedded credentials"));
    assert!(!msg.contains("onlypass"));
}

#[test]
fn test_failed_save_does_not_corrupt_existing_config() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    let initial_cfg = Config {
        base_url: "http://127.0.0.1:20128".to_string(),
        api_key: Some("sk-valid-key".to_string()),
        search_combo: "valid-search".to_string(),
        fetch_combo: "valid-fetch".to_string(),
        timeout_secs: 30,
    };
    initial_cfg
        .save(&config_path)
        .expect("Initial save must succeed");

    // 1. Attempt to save invalid config (timeout 0)
    let invalid_timeout_cfg = Config {
        timeout_secs: 0,
        ..initial_cfg.clone()
    };
    let err = invalid_timeout_cfg.save(&config_path).unwrap_err();
    assert!(err.to_string().contains("timeout_secs"));

    // Verify existing config is untouched
    let loaded = Config::load_from_file(&config_path).unwrap().unwrap();
    assert_eq!(loaded, initial_cfg);

    // 2. Attempt to save invalid config (whitespace search_combo)
    let invalid_search_cfg = Config {
        search_combo: "   ".to_string(),
        ..initial_cfg.clone()
    };
    let err = invalid_search_cfg.save(&config_path).unwrap_err();
    assert!(err.to_string().contains("search_combo"));

    // Verify existing config is still untouched
    let loaded = Config::load_from_file(&config_path).unwrap().unwrap();
    assert_eq!(loaded, initial_cfg);

    // 3. Attempt to save invalid config (credentials in base_url)
    let invalid_url_cfg = Config {
        base_url: "https://user:pass@router.com".to_string(),
        ..initial_cfg.clone()
    };
    let err = invalid_url_cfg.save(&config_path).unwrap_err();
    assert!(err.to_string().contains("embedded credentials"));

    // Verify existing config is still untouched
    let loaded = Config::load_from_file(&config_path).unwrap().unwrap();
    assert_eq!(loaded, initial_cfg);

    // 4. On Unix, make directory read-only to force filesystem error during save
    #[cfg(unix)]
    {
        let perms = fs::Permissions::from_mode(0o500);
        fs::set_permissions(dir.path(), perms).unwrap();

        let valid_new_cfg = Config {
            timeout_secs: 99,
            ..initial_cfg.clone()
        };
        let res = valid_new_cfg.save(&config_path);
        assert!(res.is_err());

        // Restore write permissions to inspect
        let restore_perms = fs::Permissions::from_mode(0o700);
        fs::set_permissions(dir.path(), restore_perms).unwrap();

        // Existing config MUST remain intact
        let loaded = Config::load_from_file(&config_path).unwrap().unwrap();
        assert_eq!(loaded, initial_cfg);
    }
}

#[test]
fn test_concurrent_saves_do_not_collide() {
    use std::sync::Arc;
    use std::thread;

    let dir = tempdir().unwrap();
    let config_path = Arc::new(dir.path().join("config.toml"));

    let mut handles = Vec::new();
    for i in 1..=8 {
        let path = Arc::clone(&config_path);
        let handle = thread::spawn(move || {
            let cfg = Config {
                base_url: "http://127.0.0.1:20128".to_string(),
                api_key: Some(format!("sk-test-{}", i)),
                search_combo: format!("search-model-{}", i),
                fetch_combo: format!("fetch-model-{}", i),
                timeout_secs: i as u64 + 10,
            };
            cfg.save(&path)
                .expect("Concurrent save should succeed without collision");
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("Thread panicked during save");
    }

    // Verify resulting config file exists and is valid
    let loaded = Config::load_from_file(&config_path)
        .expect("Load should succeed")
        .expect("Config should exist");
    assert!(loaded.timeout_secs >= 11 && loaded.timeout_secs <= 18);

    #[cfg(unix)]
    {
        let metadata = fs::metadata(&*config_path).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }

    // Verify no temporary files remain in directory
    let entries: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    for entry in entries {
        assert!(
            !entry.starts_with(".config.tmp."),
            "Found leftover temp file: {}",
            entry
        );
    }
}

#[test]
fn test_client_and_server_reject_malformed_config() {
    use ninerouter_mcp_web::client::NineRouterClient;
    use ninerouter_mcp_web::server::NineRouterMcpServer;

    let bad_url_cfg = Config {
        base_url: "https://user:pass@router.com".to_string(),
        ..Config::default()
    };
    assert!(NineRouterClient::new(&bad_url_cfg).is_err());
    assert!(NineRouterMcpServer::new(&bad_url_cfg).is_err());

    let bad_search_cfg = Config {
        search_combo: "   ".to_string(),
        ..Config::default()
    };
    assert!(NineRouterMcpServer::new(&bad_search_cfg).is_err());

    let bad_fetch_cfg = Config {
        fetch_combo: "\t".to_string(),
        ..Config::default()
    };
    assert!(NineRouterMcpServer::new(&bad_fetch_cfg).is_err());
}

#[test]
fn test_config_debug_secret_safe_and_retains_useful_fields() {
    let secret = "sk-super-secret-api-token-987654321";
    let cfg = Config {
        base_url: "http://127.0.0.1:20128".to_string(),
        api_key: Some(secret.to_string()),
        search_combo: "my-search-combo".to_string(),
        fetch_combo: "my-fetch-combo".to_string(),
        timeout_secs: 42,
    };

    let normal_debug = format!("{:?}", cfg);
    let pretty_debug = format!("{:#?}", cfg);

    // Raw API key must NEVER be contained in normal or pretty Debug
    assert!(
        !normal_debug.contains(secret),
        "Normal Debug leaked raw API key: {}",
        normal_debug
    );
    assert!(
        !pretty_debug.contains(secret),
        "Pretty Debug leaked raw API key: {}",
        pretty_debug
    );

    // Masked representation should be present
    assert!(normal_debug.contains("sk-...4321"));
    assert!(pretty_debug.contains("sk-...4321"));

    // Useful safe fields must remain visible in both forms
    for (label, output) in [("normal", &normal_debug), ("pretty", &pretty_debug)] {
        assert!(
            output.contains("http://127.0.0.1:20128"),
            "{} Debug missing base_url value",
            label
        );
        assert!(
            output.contains("my-search-combo"),
            "{} Debug missing search_combo value",
            label
        );
        assert!(
            output.contains("my-fetch-combo"),
            "{} Debug missing fetch_combo value",
            label
        );
        assert!(
            output.contains("42"),
            "{} Debug missing timeout_secs value",
            label
        );
        assert!(
            output.contains("base_url"),
            "{} Debug missing base_url field name",
            label
        );
        assert!(
            output.contains("search_combo"),
            "{} Debug missing search_combo field name",
            label
        );
        assert!(
            output.contains("fetch_combo"),
            "{} Debug missing fetch_combo field name",
            label
        );
        assert!(
            output.contains("timeout_secs"),
            "{} Debug missing timeout_secs field name",
            label
        );
    }

    // Config with None API key
    let no_key_cfg = Config {
        api_key: None,
        ..cfg.clone()
    };
    let normal_no_key = format!("{:?}", no_key_cfg);
    let pretty_no_key = format!("{:#?}", no_key_cfg);
    assert!(normal_no_key.contains("api_key: None"));
    assert!(pretty_no_key.contains("api_key: None"));

    // Config with short secret (<= 8 characters)
    let short_secret = "12345678";
    let short_cfg = Config {
        api_key: Some(short_secret.to_string()),
        ..cfg.clone()
    };
    let normal_short = format!("{:?}", short_cfg);
    let pretty_short = format!("{:#?}", short_cfg);
    assert!(!normal_short.contains(short_secret));
    assert!(!pretty_short.contains(short_secret));
    assert!(normal_short.contains("***"));
    assert!(pretty_short.contains("***"));

    // Config with multibyte UTF-8 secret
    let utf8_secret = "🔑秘密APIキー99887766";
    let utf8_cfg = Config {
        api_key: Some(utf8_secret.to_string()),
        ..cfg
    };
    let normal_utf8 = format!("{:?}", utf8_cfg);
    let pretty_utf8 = format!("{:#?}", utf8_cfg);
    assert!(!normal_utf8.contains(utf8_secret));
    assert!(!pretty_utf8.contains(utf8_secret));
}

#[test]
fn test_env_preferred_aliases_keep_precedence() {
    let mut guard = EnvGuard::new(vec![
        "NINEROUTER_URL",
        "NINEROUTER_BASE_URL",
        "NINEROUTER_KEY",
        "NINEROUTER_API_KEY",
    ]);

    guard.set("NINEROUTER_URL", "http://primary-host:20128");
    guard.set("NINEROUTER_BASE_URL", "http://secondary-host:20128");
    guard.set("NINEROUTER_KEY", "sk-primary-key");
    guard.set("NINEROUTER_API_KEY", "sk-secondary-key");

    let cfg = Config::resolve(None).expect("Resolve should succeed");
    assert_eq!(cfg.base_url, "http://primary-host:20128");
    assert_eq!(cfg.api_key.as_deref(), Some("sk-primary-key"));
}

#[test]
fn test_env_empty_and_whitespace_preferred_aliases_fall_through() {
    let mut guard = EnvGuard::new(vec![
        "NINEROUTER_URL",
        "NINEROUTER_BASE_URL",
        "NINEROUTER_KEY",
        "NINEROUTER_API_KEY",
    ]);

    // 1. Empty string preferred aliases fall through to secondary
    guard.set("NINEROUTER_URL", "");
    guard.set("NINEROUTER_BASE_URL", "http://secondary-host:20128");
    guard.set("NINEROUTER_KEY", "");
    guard.set("NINEROUTER_API_KEY", "sk-secondary-key");

    let cfg = Config::resolve(None).expect("Resolve should succeed");
    assert_eq!(cfg.base_url, "http://secondary-host:20128");
    assert_eq!(cfg.api_key.as_deref(), Some("sk-secondary-key"));

    // 2. Whitespace-only string preferred aliases fall through to secondary
    guard.set("NINEROUTER_URL", "   \t  \n");
    guard.set("NINEROUTER_KEY", "   \t  ");

    let cfg2 = Config::resolve(None).expect("Resolve should succeed");
    assert_eq!(cfg2.base_url, "http://secondary-host:20128");
    assert_eq!(cfg2.api_key.as_deref(), Some("sk-secondary-key"));
}

#[test]
fn test_env_empty_aliases_fall_through_to_file_and_defaults() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    let file_cfg = Config {
        base_url: "http://file-host:20128".to_string(),
        api_key: Some("sk-file-key".to_string()),
        search_combo: "search-file".to_string(),
        fetch_combo: "fetch-file".to_string(),
        timeout_secs: 15,
    };
    file_cfg.save(&config_path).unwrap();

    let mut guard = EnvGuard::new(vec![
        "NINEROUTER_URL",
        "NINEROUTER_BASE_URL",
        "NINEROUTER_KEY",
        "NINEROUTER_API_KEY",
    ]);

    // Both aliases empty/whitespace -> falls through to file config
    guard.set("NINEROUTER_URL", "  ");
    guard.set("NINEROUTER_BASE_URL", "");
    guard.set("NINEROUTER_KEY", "");
    guard.set("NINEROUTER_API_KEY", "  \t ");

    let resolved = Config::resolve(Some(&config_path)).expect("Resolve should succeed");
    assert_eq!(resolved.base_url, "http://file-host:20128");
    assert_eq!(resolved.api_key.as_deref(), Some("sk-file-key"));

    // Without file config, falls through to defaults
    let resolved_default = Config::resolve(Some(&dir.path().join("nonexistent.toml")))
        .expect("Resolve should succeed");
    assert_eq!(resolved_default.base_url, DEFAULT_BASE_URL);
    assert_eq!(resolved_default.api_key, None);
}

#[test]
fn test_save_persists_canonical_normalized_values() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    let non_canonical_cfg = Config {
        base_url: "http://localhost:20128///".to_string(),
        api_key: Some("   sk-padded-secret-key   ".to_string()),
        search_combo: "   custom-search   ".to_string(),
        fetch_combo: "   custom-fetch   ".to_string(),
        timeout_secs: 25,
    };

    non_canonical_cfg
        .save(&config_path)
        .expect("Save should succeed");

    // Inspect the raw file content on disk to verify canonical values were persisted
    let raw_content = fs::read_to_string(&config_path).unwrap();
    assert!(
        raw_content.contains(r#"base_url = "http://localhost:20128""#),
        "Raw content must contain normalized base_url without trailing slash, got: {}",
        raw_content
    );
    assert!(
        raw_content.contains(r#"api_key = "sk-padded-secret-key""#),
        "Raw content must contain trimmed api_key, got: {}",
        raw_content
    );
    assert!(
        raw_content.contains(r#"search_combo = "custom-search""#),
        "Raw content must contain trimmed search_combo, got: {}",
        raw_content
    );
    assert!(
        raw_content.contains(r#"fetch_combo = "custom-fetch""#),
        "Raw content must contain trimmed fetch_combo, got: {}",
        raw_content
    );

    // Load back and verify it matches the canonical equivalent
    let loaded = Config::load_from_file(&config_path)
        .expect("Load should succeed")
        .expect("Config should exist");

    let expected_canonical = non_canonical_cfg.canonical().unwrap();
    assert_eq!(loaded, expected_canonical);
    assert_eq!(loaded.base_url, "http://localhost:20128");
    assert_eq!(loaded.api_key.as_deref(), Some("sk-padded-secret-key"));
    assert_eq!(loaded.search_combo, "custom-search");
    assert_eq!(loaded.fetch_combo, "custom-fetch");
    assert_eq!(loaded.timeout_secs, 25);
}

#[test]
fn test_save_normalizes_empty_or_whitespace_api_key_to_none() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    let cfg_with_empty_key = Config {
        base_url: "http://localhost:20128/".to_string(),
        api_key: Some("   \t  ".to_string()),
        search_combo: "search-combo".to_string(),
        fetch_combo: "fetch-combo".to_string(),
        timeout_secs: 30,
    };

    cfg_with_empty_key
        .save(&config_path)
        .expect("Save should succeed");

    let raw_content = fs::read_to_string(&config_path).unwrap();
    assert!(
        !raw_content.contains("api_key"),
        "Raw content should omit empty api_key, got: {}",
        raw_content
    );

    let loaded = Config::load_from_file(&config_path)
        .expect("Load failed")
        .expect("Config should exist");

    assert_eq!(loaded.api_key, None);
    assert_eq!(loaded, cfg_with_empty_key.canonical().unwrap());
}

#[test]
fn test_save_load_round_trip_idempotent() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    let initial = Config {
        base_url: "https://remote.server.example:8443/prefix///".to_string(),
        api_key: Some("  sk-test-token-roundtrip  ".to_string()),
        search_combo: "  s-combo  ".to_string(),
        fetch_combo: "  f-combo  ".to_string(),
        timeout_secs: 60,
    };

    // First save & load
    initial.save(&config_path).unwrap();
    let round_1 = Config::load_from_file(&config_path).unwrap().unwrap();

    assert_eq!(round_1, initial.canonical().unwrap());
    assert_eq!(
        round_1.base_url,
        "https://remote.server.example:8443/prefix"
    );
    assert_eq!(round_1.api_key.as_deref(), Some("sk-test-token-roundtrip"));
    assert_eq!(round_1.search_combo, "s-combo");
    assert_eq!(round_1.fetch_combo, "f-combo");

    // Second save & load
    round_1.save(&config_path).unwrap();
    let round_2 = Config::load_from_file(&config_path).unwrap().unwrap();

    assert_eq!(round_1, round_2);
}
