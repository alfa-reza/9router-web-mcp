use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use url::Url;

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};

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

/// Validates and normalizes the 9Router base URL.
///
/// Rules:
/// - Must be an absolute HTTP or HTTPS URL.
/// - Must have a valid host.
/// - Must not contain query parameters or fragments.
/// - Trailing slashes are stripped deterministically while preserving any path prefix.
pub fn validate_and_normalize_base_url(url: &str) -> Result<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(AppError::Config(
            "9Router base URL cannot be empty".to_string(),
        ));
    }

    let parsed = Url::parse(trimmed)
        .map_err(|e| AppError::Config(format!("Invalid 9Router base URL '{}': {}", trimmed, e)))?;

    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(AppError::Config(format!(
            "Unsupported scheme '{}' in 9Router base URL. Only http and https are allowed",
            scheme
        )));
    }

    match parsed.host_str() {
        Some(h) if !h.is_empty() => {}
        _ => {
            return Err(AppError::Config(format!(
                "9Router base URL '{}' must include a valid host",
                trimmed
            )));
        }
    }

    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(AppError::Config(format!(
            "9Router base URL '{}' must not contain query parameters or fragments",
            trimmed
        )));
    }

    let mut normalized = parsed;
    let path = normalized.path().to_string();
    let trimmed_path = path.trim_end_matches('/');
    normalized.set_path(trimmed_path);

    let mut result = normalized.to_string();
    if result.ends_with('/') {
        result.pop();
    }

    Ok(result)
}

/// Normalizes a base URL string, falling back to trimmed stripped-slash if parsing fails.
pub fn normalize_base_url(url: &str) -> String {
    validate_and_normalize_base_url(url).unwrap_or_else(|_| {
        let trimmed = url.trim();
        trimmed.trim_end_matches('/').to_string()
    })
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

    /// Resolve configuration path following precedence:
    /// 1. Explicit CLI override
    /// 2. NINEROUTER_CONFIG environment variable
    /// 3. Default path (~/.config/9router-mcp-web/config.toml)
    pub fn resolve_path(override_path: Option<&Path>) -> Result<PathBuf> {
        if let Some(p) = override_path {
            let p_str = p.to_string_lossy();
            if p_str.trim().is_empty() {
                return Err(AppError::Config(
                    "Configuration path cannot be empty".to_string(),
                ));
            }
            return Ok(p.to_path_buf());
        }

        if let Ok(env_path) = std::env::var("NINEROUTER_CONFIG") {
            let trimmed = env_path.trim();
            if !trimmed.is_empty() {
                return Ok(PathBuf::from(trimmed));
            }
        }

        Self::default_path()
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

        let raw_base_url = raw
            .base_url
            .or_else(|| raw.ninerouter.as_ref().and_then(|t| t.base_url.clone()))
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        let base_url = validate_and_normalize_base_url(&raw_base_url)?;

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

        if timeout_secs == 0 {
            return Err(AppError::Config(
                "timeout_secs must be greater than 0".to_string(),
            ));
        }

        Ok(Some(Self {
            base_url,
            api_key,
            search_combo: search_combo.trim().to_string(),
            fetch_combo: fetch_combo.trim().to_string(),
            timeout_secs,
        }))
    }

    /// Resolve configuration using precedence:
    /// Environment Variables > File Configuration > Defaults
    pub fn resolve(config_path_override: Option<&Path>) -> Result<Self> {
        let config_path = Self::resolve_path(config_path_override)?;
        let file_config = Self::load_from_file(&config_path)?.unwrap_or_default();

        // 1. Base URL override
        let base_url = if let Some(env_url) = std::env::var("NINEROUTER_URL")
            .or_else(|_| std::env::var("NINEROUTER_BASE_URL"))
            .ok()
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty())
        {
            validate_and_normalize_base_url(&env_url)?
        } else {
            file_config.base_url
        };

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
            let trimmed = t.trim();
            if trimmed.is_empty() {
                file_config.timeout_secs
            } else {
                let parsed = trimmed.parse::<u64>().map_err(|_| {
                    AppError::Config(format!(
                        "Invalid timeout '{}' in NINEROUTER_TIMEOUT_SECS: must be a positive integer",
                        trimmed
                    ))
                })?;
                if parsed == 0 {
                    return Err(AppError::Config(
                        "Timeout in NINEROUTER_TIMEOUT_SECS must be greater than 0".to_string(),
                    ));
                }
                parsed
            }
        } else {
            file_config.timeout_secs
        };

        Ok(Self {
            base_url,
            api_key,
            search_combo,
            fetch_combo,
            timeout_secs,
        })
    }

    /// Save configuration securely to disk with mode 0600 on Unix.
    ///
    /// If the parent directory does not exist, it is created with mode 0700 on Unix.
    /// If the parent directory already exists, its permissions are left untouched.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                #[cfg(unix)]
                {
                    let mut builder = fs::DirBuilder::new();
                    builder.recursive(true);
                    builder.mode(0o700);
                    builder.create(parent).map_err(|e| {
                        AppError::Config(format!(
                            "Failed to create config directory {}: {}",
                            parent.display(),
                            e
                        ))
                    })?;
                }
                #[cfg(not(unix))]
                {
                    fs::create_dir_all(parent).map_err(|e| {
                        AppError::Config(format!(
                            "Failed to create config directory {}: {}",
                            parent.display(),
                            e
                        ))
                    })?;
                }
            }
        }

        let toml_str = toml::to_string_pretty(self)
            .map_err(|e| AppError::Config(format!("Failed to serialize config to TOML: {}", e)))?;

        let tmp_path = path.with_extension("tmp");

        {
            let mut options = OpenOptions::new();
            options.create(true).write(true).truncate(true);

            #[cfg(unix)]
            {
                options.mode(0o600);
            }

            let mut file = options.open(&tmp_path).map_err(|e| {
                AppError::Config(format!(
                    "Failed to create temp config file {}: {}",
                    tmp_path.display(),
                    e
                ))
            })?;

            #[cfg(unix)]
            {
                let perms = fs::Permissions::from_mode(0o600);
                let _ = file.set_permissions(perms);
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
    ///
    /// Character-based to prevent panicking on multibyte UTF-8 boundaries.
    pub fn masked_api_key(&self) -> String {
        match &self.api_key {
            None => "(none)".to_string(),
            Some(key) => {
                let chars: Vec<char> = key.chars().collect();
                if chars.len() <= 8 {
                    "***".to_string()
                } else {
                    let prefix: String = chars[..3].iter().collect();
                    let suffix: String = chars[chars.len() - 4..].iter().collect();
                    format!("{}...{}", prefix, suffix)
                }
            }
        }
    }
}

/// Run the interactive `configure` CLI flow.
pub fn run_interactive_configure(config_path_override: Option<&Path>) -> Result<()> {
    let target_path = Config::resolve_path(config_path_override)?;

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
        validate_and_normalize_base_url(base_url_trimmed)?
    };

    // 2. API Key (masked input, explicit clearing via '-' supported)
    let current_key_status = if existing.api_key.is_some() {
        format!(
            " (current: {}, enter '-' to clear)",
            existing.masked_api_key()
        )
    } else {
        String::new()
    };
    eprint!(
        "9Router API key (leave empty to keep/skip){}: ",
        current_key_status
    );
    io::stderr().flush().ok();
    let raw_key_input = match rpassword::prompt_password("") {
        Ok(pass) => pass.trim().to_string(),
        Err(_) => {
            let mut line = String::new();
            reader.read_line(&mut line).ok();
            line.trim().to_string()
        }
    };

    let api_key = if raw_key_input == "-" || raw_key_input == "none" || raw_key_input == "clear" {
        None
    } else if raw_key_input.is_empty() {
        existing.api_key
    } else {
        Some(raw_key_input)
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
        let parsed = timeout_trimmed.parse::<u64>().map_err(|_| {
            AppError::Config(format!(
                "Invalid timeout '{}': must be a positive integer",
                timeout_trimmed
            ))
        })?;
        if parsed == 0 {
            return Err(AppError::Config(
                "Timeout must be greater than 0 seconds".to_string(),
            ));
        }
        parsed
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

/// Parse the optional configuration file path from command line arguments.
///
/// Returns `Ok(Some(PathBuf))` if `--config <PATH>`, `-c <PATH>`, or `--config=<PATH>` is present.
/// Returns `Ok(None)` if no config flag was passed.
/// Returns an error if the flag is provided without a non-empty path.
pub fn parse_config_arg(args: &[String]) -> Result<Option<PathBuf>> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--config" || args[i] == "-c" {
            if i + 1 < args.len() {
                let val = args[i + 1].trim();
                if val.is_empty() {
                    return Err(AppError::Config(
                        "Flag '--config' / '-c' requires a non-empty path argument".to_string(),
                    ));
                }
                return Ok(Some(PathBuf::from(val)));
            } else {
                return Err(AppError::Config(
                    "Flag '--config' / '-c' requires a path argument".to_string(),
                ));
            }
        } else if let Some(stripped) = args[i].strip_prefix("--config=") {
            let val = stripped.trim();
            if val.is_empty() {
                return Err(AppError::Config(
                    "Flag '--config=' requires a non-empty path argument".to_string(),
                ));
            }
            return Ok(Some(PathBuf::from(val)));
        }
        i += 1;
    }
    Ok(None)
}
