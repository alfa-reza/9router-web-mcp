use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use url::Url;

use crate::error::{AppError, Result};

pub const DEFAULT_BASE_URL: &str = "http://localhost:20128";
pub const DEFAULT_SEARCH_COMBO: &str = "search-combo";
pub const DEFAULT_FETCH_COMBO: &str = "fetch-combo";
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

fn default_search_combo() -> String {
    DEFAULT_SEARCH_COMBO.to_string()
}

fn default_fetch_combo() -> String {
    DEFAULT_FETCH_COMBO.to_string()
}

fn default_timeout_secs() -> u64 {
    DEFAULT_TIMEOUT_SECS
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub base_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default = "default_search_combo")]
    pub search_combo: String,
    #[serde(default = "default_fetch_combo")]
    pub fetch_combo: String,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: None,
            search_combo: default_search_combo(),
            fetch_combo: default_fetch_combo(),
            timeout_secs: default_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
struct RawConfigFile {
    base_url: Option<String>,
    api_key: Option<String>,
    search_combo: Option<String>,
    fetch_combo: Option<String>,
    timeout_secs: Option<u64>,
    ninerouter: Option<RawNineRouterTable>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct RawNineRouterTable {
    base_url: Option<String>,
    api_key: Option<String>,
    search_combo: Option<String>,
    fetch_combo: Option<String>,
    timeout_secs: Option<u64>,
}

pub fn normalize_base_url(url: &str) -> String {
    let trimmed = url.trim();
    trimmed.strip_suffix('/').unwrap_or(trimmed).to_string()
}

impl Config {
    /// Return the standard default configuration file path:
    /// $XDG_CONFIG_HOME/9router-mcp-web/config.toml or ~/.config/9router-mcp-web/config.toml
    pub fn default_path() -> Result<PathBuf> {
        let base_dir = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.trim().is_empty() {
                PathBuf::from(xdg.trim())
            } else if let Ok(home) = std::env::var("HOME") {
                PathBuf::from(home).join(".config")
            } else {
                return Err(AppError::Config(
                    "Cannot determine HOME directory".to_string(),
                ));
            }
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".config")
        } else {
            return Err(AppError::Config(
                "Cannot determine HOME directory".to_string(),
            ));
        };

        Ok(base_dir.join("9router-mcp-web").join("config.toml"))
    }

    /// Load config from file if it exists, returning None if file does not exist.
    pub fn load_from_file(path: &Path) -> Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(path).map_err(|e| {
            AppError::Config(format!(
                "Failed to read config file {}: {}",
                path.display(),
                e
            ))
        })?;

        let raw: RawConfigFile = toml::from_str(&content).map_err(|e| {
            AppError::Config(format!(
                "Failed to parse config file {}: {}",
                path.display(),
                e
            ))
        })?;

        let base_url = raw
            .base_url
            .or_else(|| raw.ninerouter.as_ref().and_then(|t| t.base_url.clone()))
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        let api_key = raw
            .api_key
            .or_else(|| raw.ninerouter.as_ref().and_then(|t| t.api_key.clone()))
            .and_then(|k| {
                let trimmed = k.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            });

        let search_combo = raw
            .search_combo
            .or_else(|| raw.ninerouter.as_ref().and_then(|t| t.search_combo.clone()))
            .unwrap_or_else(default_search_combo);

        let fetch_combo = raw
            .fetch_combo
            .or_else(|| raw.ninerouter.as_ref().and_then(|t| t.fetch_combo.clone()))
            .unwrap_or_else(default_fetch_combo);

        let timeout_secs = raw
            .timeout_secs
            .or_else(|| raw.ninerouter.as_ref().and_then(|t| t.timeout_secs))
            .unwrap_or(DEFAULT_TIMEOUT_SECS);

        Ok(Some(Self {
            base_url: normalize_base_url(&base_url),
            api_key,
            search_combo: search_combo.trim().to_string(),
            fetch_combo: fetch_combo.trim().to_string(),
            timeout_secs,
        }))
    }

    /// Resolve configuration using precedence:
    /// Environment Variables > File Configuration > Defaults
    pub fn resolve(config_path_override: Option<&Path>) -> Result<Self> {
        let config_path = match config_path_override {
            Some(p) => p.to_path_buf(),
            None => {
                if let Ok(env_path) = std::env::var("NINEROUTER_CONFIG") {
                    if !env_path.trim().is_empty() {
                        PathBuf::from(env_path.trim())
                    } else {
                        Self::default_path()?
                    }
                } else {
                    Self::default_path()?
                }
            }
        };

        let file_config = Self::load_from_file(&config_path)?.unwrap_or_default();

        // 1. Base URL override
        let base_url = std::env::var("NINEROUTER_URL")
            .or_else(|_| std::env::var("NINEROUTER_BASE_URL"))
            .ok()
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty())
            .unwrap_or(file_config.base_url);

        let normalized_base_url = normalize_base_url(&base_url);

        // 2. API Key override
        let api_key = std::env::var("NINEROUTER_KEY")
            .or_else(|_| std::env::var("NINEROUTER_API_KEY"))
            .ok()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .or(file_config.api_key);

        // 3. Search Combo override
        let search_combo = std::env::var("NINEROUTER_SEARCH_COMBO")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or(file_config.search_combo);

        // 4. Fetch Combo override
        let fetch_combo = std::env::var("NINEROUTER_FETCH_COMBO")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or(file_config.fetch_combo);

        // 5. Timeout override
        let timeout_secs = if let Ok(t) = std::env::var("NINEROUTER_TIMEOUT_SECS") {
            t.trim().parse::<u64>().unwrap_or(file_config.timeout_secs)
        } else {
            file_config.timeout_secs
        };

        Ok(Self {
            base_url: normalized_base_url,
            api_key,
            search_combo,
            fetch_combo,
            timeout_secs,
        })
    }

    /// Save configuration securely to disk with mode 0600 and directory mode 0700.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                AppError::Config(format!(
                    "Failed to create config directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;

            // Set parent directory permissions to 0700
            #[cfg(unix)]
            {
                let perms = fs::Permissions::from_mode(0o700);
                let _ = fs::set_permissions(parent, perms);
            }
        }

        let toml_str = toml::to_string_pretty(self)
            .map_err(|e| AppError::Config(format!("Failed to serialize config to TOML: {}", e)))?;

        let tmp_path = path.with_extension("tmp");

        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp_path)
                .map_err(|e| {
                    AppError::Config(format!(
                        "Failed to create temp config file {}: {}",
                        tmp_path.display(),
                        e
                    ))
                })?;

            #[cfg(unix)]
            {
                let perms = fs::Permissions::from_mode(0o600);
                file.set_permissions(perms).map_err(|e| {
                    AppError::Config(format!(
                        "Failed to set 0600 permissions on {}: {}",
                        tmp_path.display(),
                        e
                    ))
                })?;
            }

            file.write_all(toml_str.as_bytes()).map_err(|e| {
                AppError::Config(format!(
                    "Failed to write to temp config file {}: {}",
                    tmp_path.display(),
                    e
                ))
            })?;

            file.sync_all().map_err(|e| {
                AppError::Config(format!(
                    "Failed to flush temp config file {}: {}",
                    tmp_path.display(),
                    e
                ))
            })?;
        }

        fs::rename(&tmp_path, path).map_err(|e| {
            AppError::Config(format!(
                "Failed to atomically rename {} to {}: {}",
                tmp_path.display(),
                path.display(),
                e
            ))
        })?;

        Ok(())
    }

    /// Check if plaintext HTTP is used with an API key against a non-local host (R-CFG-07).
    pub fn check_plain_http_warning(&self) -> Option<String> {
        self.api_key.as_ref()?;

        if let Ok(parsed) = Url::parse(&self.base_url) {
            if parsed.scheme() == "http" {
                if let Some(host) = parsed.host_str() {
                    let host_lower = host.to_lowercase();
                    if host_lower != "localhost"
                        && host_lower != "127.0.0.1"
                        && host_lower != "::1"
                        && host_lower != "[::1]"
                    {
                        return Some(format!(
                            "WARNING: 9Router API key is configured over unencrypted plaintext HTTP to non-local host '{}'. Credentials may be intercepted in transit!",
                            host
                        ));
                    }
                }
            }
        }
        None
    }

    /// Return a safe, masked representation of the API key for diagnostics.
    pub fn masked_api_key(&self) -> String {
        match &self.api_key {
            None => "(none)".to_string(),
            Some(key) => {
                if key.len() <= 8 {
                    "***".to_string()
                } else {
                    let prefix = &key[..3];
                    let suffix = &key[key.len() - 4..];
                    format!("{}...{}", prefix, suffix)
                }
            }
        }
    }
}

/// Run the interactive `configure` CLI flow.
pub fn run_interactive_configure(config_path_override: Option<&Path>) -> Result<()> {
    let target_path = match config_path_override {
        Some(p) => p.to_path_buf(),
        None => Config::default_path()?,
    };

    let existing = Config::load_from_file(&target_path)?.unwrap_or_default();

    eprintln!("=== 9router-mcp-web Configuration ===");
    eprintln!("Config file destination: {}", target_path.display());
    eprintln!();

    let stdin = io::stdin();
    let mut reader = stdin.lock();

    // 1. Base URL
    eprint!("9Router base URL [{}]: ", existing.base_url);
    io::stderr().flush().ok();
    let mut base_url_input = String::new();
    reader.read_line(&mut base_url_input).ok();
    let base_url_trimmed = base_url_input.trim();
    let base_url = if base_url_trimmed.is_empty() {
        existing.base_url.clone()
    } else {
        normalize_base_url(base_url_trimmed)
    };

    // 2. API Key (masked input)
    let current_key_status = if existing.api_key.is_some() {
        format!(" (current: {})", existing.masked_api_key())
    } else {
        String::new()
    };
    eprint!(
        "9Router API key (leave empty to keep/skip){}: ",
        current_key_status
    );
    io::stderr().flush().ok();
    let api_key = match rpassword::prompt_password("") {
        Ok(pass) => {
            let pass_trimmed = pass.trim().to_string();
            if pass_trimmed.is_empty() {
                existing.api_key
            } else {
                Some(pass_trimmed)
            }
        }
        Err(_) => {
            let mut line = String::new();
            reader.read_line(&mut line).ok();
            let line_trimmed = line.trim().to_string();
            if line_trimmed.is_empty() {
                existing.api_key
            } else {
                Some(line_trimmed)
            }
        }
    };

    // 3. Search Combo
    eprint!("Search combo name [{}]: ", existing.search_combo);
    io::stderr().flush().ok();
    let mut search_input = String::new();
    reader.read_line(&mut search_input).ok();
    let search_trimmed = search_input.trim();
    let search_combo = if search_trimmed.is_empty() {
        existing.search_combo
    } else {
        search_trimmed.to_string()
    };

    // 4. Fetch Combo
    eprint!("Fetch combo name [{}]: ", existing.fetch_combo);
    io::stderr().flush().ok();
    let mut fetch_input = String::new();
    reader.read_line(&mut fetch_input).ok();
    let fetch_trimmed = fetch_input.trim();
    let fetch_combo = if fetch_trimmed.is_empty() {
        existing.fetch_combo
    } else {
        fetch_trimmed.to_string()
    };

    // 5. Timeout
    eprint!("Request timeout in seconds [{}]: ", existing.timeout_secs);
    io::stderr().flush().ok();
    let mut timeout_input = String::new();
    reader.read_line(&mut timeout_input).ok();
    let timeout_trimmed = timeout_input.trim();
    let timeout_secs = if timeout_trimmed.is_empty() {
        existing.timeout_secs
    } else {
        timeout_trimmed
            .parse::<u64>()
            .unwrap_or(existing.timeout_secs)
    };

    let updated_config = Config {
        base_url,
        api_key,
        search_combo,
        fetch_combo,
        timeout_secs,
    };

    if let Some(warning) = updated_config.check_plain_http_warning() {
        eprintln!();
        eprintln!("{}", warning);
    }

    updated_config.save(&target_path)?;

    eprintln!();
    eprintln!(
        "Configuration saved successfully to: {}",
        target_path.display()
    );
    Ok(())
}
