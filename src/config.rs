use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use url::Url;

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

use crate::discovery::{discover_local_9router, DiscoveredInstance};
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

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("base_url", &self.base_url)
            .field(
                "api_key",
                &self.api_key.as_ref().map(|_| self.masked_api_key()),
            )
            .field("search_combo", &self.search_combo)
            .field("fetch_combo", &self.fetch_combo)
            .field("timeout_secs", &self.timeout_secs)
            .finish()
    }
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

/// Representation of the configuration persisted to disk.
///
/// If `base_url` was discovered automatically rather than configured manually by the user,
/// it is omitted (`None`) so that it remains runtime-derived and does not harden a temporary
/// discovery result into authoritative manual configuration.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PersistedConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default = "default_search_combo")]
    pub search_combo: String,
    #[serde(default = "default_fetch_combo")]
    pub fetch_combo: String,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

impl PersistedConfig {
    pub fn canonical(&self) -> Result<Self> {
        let base_url = match &self.base_url {
            Some(u) => Some(validate_and_normalize_base_url(u)?),
            None => None,
        };

        let search_combo = self.search_combo.trim();
        if search_combo.is_empty() {
            return Err(AppError::Config(
                "search_combo cannot be empty or whitespace-only".to_string(),
            ));
        }

        let fetch_combo = self.fetch_combo.trim();
        if fetch_combo.is_empty() {
            return Err(AppError::Config(
                "fetch_combo cannot be empty or whitespace-only".to_string(),
            ));
        }

        if self.timeout_secs == 0 {
            return Err(AppError::Config(
                "timeout_secs must be greater than 0".to_string(),
            ));
        }

        let api_key = self.api_key.as_deref().and_then(|k| {
            let trimmed = k.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });

        Ok(Self {
            base_url,
            api_key,
            search_combo: search_combo.to_string(),
            fetch_combo: fetch_combo.to_string(),
            timeout_secs: self.timeout_secs,
        })
    }

    /// Save configuration securely to disk with mode 0600 on Unix.
    pub fn save(&self, path: &Path) -> Result<()> {
        let canonical = self.canonical()?;

        let parent = match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => Path::new("."),
        };

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

        let toml_str = toml::to_string_pretty(&canonical)
            .map_err(|e| AppError::Config(format!("Failed to serialize config to TOML: {}", e)))?;

        let mut tmp_file = tempfile::Builder::new()
            .prefix(".config.tmp.")
            .tempfile_in(parent)
            .map_err(|e| {
                AppError::Config(format!(
                    "Failed to create temporary config file in {}: {}",
                    parent.display(),
                    e
                ))
            })?;

        #[cfg(unix)]
        {
            let perms = fs::Permissions::from_mode(0o600);
            tmp_file.as_file().set_permissions(perms).map_err(|e| {
                AppError::Config(format!(
                    "Failed to set private permissions on temporary config file: {}",
                    e
                ))
            })?;
        }

        tmp_file.write_all(toml_str.as_bytes()).map_err(|e| {
            AppError::Config(format!(
                "Failed to write config data to temporary file: {}",
                e
            ))
        })?;

        tmp_file.as_file().sync_all().map_err(|e| {
            AppError::Config(format!("Failed to flush temporary config file: {}", e))
        })?;

        tmp_file.persist(path).map_err(|e| {
            AppError::Config(format!(
                "Failed to atomically replace config file at {}: {}",
                path.display(),
                e.error
            ))
        })?;

        Ok(())
    }
}

/// Validated configuration loaded from disk.
///
/// Distinguishes between an explicitly set `base_url` vs an omitted (`None`) `base_url`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileConfig {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub search_combo: Option<String>,
    pub fetch_combo: Option<String>,
    pub timeout_secs: Option<u64>,
}

impl FileConfig {
    pub fn load(path: &Path) -> Result<Option<Self>> {
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
            .or_else(|| raw.ninerouter.as_ref().and_then(|t| t.base_url.clone()));

        let base_url = match raw_base_url {
            Some(u) => Some(validate_and_normalize_base_url(&u)?),
            None => None,
        };

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
            .or_else(|| raw.ninerouter.as_ref().and_then(|t| t.search_combo.clone()));

        let search_combo = match search_combo {
            Some(s) => {
                let trimmed = s.trim().to_string();
                if trimmed.is_empty() {
                    return Err(AppError::Config(
                        "search_combo cannot be empty or whitespace-only".to_string(),
                    ));
                }
                Some(trimmed)
            }
            None => None,
        };

        let fetch_combo = raw
            .fetch_combo
            .or_else(|| raw.ninerouter.as_ref().and_then(|t| t.fetch_combo.clone()));

        let fetch_combo = match fetch_combo {
            Some(f) => {
                let trimmed = f.trim().to_string();
                if trimmed.is_empty() {
                    return Err(AppError::Config(
                        "fetch_combo cannot be empty or whitespace-only".to_string(),
                    ));
                }
                Some(trimmed)
            }
            None => None,
        };

        let raw_timeout = raw
            .timeout_secs
            .or_else(|| raw.ninerouter.as_ref().and_then(|t| t.timeout_secs));

        let timeout_secs = match raw_timeout {
            Some(0) => {
                return Err(AppError::Config(
                    "timeout_secs must be greater than 0".to_string(),
                ));
            }
            Some(t) => Some(t),
            None => None,
        };

        Ok(Some(Self {
            base_url,
            api_key,
            search_combo,
            fetch_combo,
            timeout_secs,
        }))
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

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(AppError::Config(
            "9Router base URL must not contain embedded credentials (username or password)"
                .to_string(),
        ));
    }

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

fn env_non_empty(var_name: &str) -> Option<String> {
    std::env::var(var_name).ok().and_then(|val| {
        let trimmed = val.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
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

        if let Some(env_path) = env_non_empty("NINEROUTER_CONFIG") {
            return Ok(PathBuf::from(env_path));
        }

        Self::default_path()
    }

    /// Load config from file if it exists, returning None if file does not exist.
    pub fn load_from_file(path: &Path) -> Result<Option<Self>> {
        let file_cfg = match FileConfig::load(path)? {
            Some(fc) => fc,
            None => return Ok(None),
        };

        Ok(Some(Self {
            base_url: file_cfg
                .base_url
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            api_key: file_cfg.api_key,
            search_combo: file_cfg.search_combo.unwrap_or_else(default_search_combo),
            fetch_combo: file_cfg.fetch_combo.unwrap_or_else(default_fetch_combo),
            timeout_secs: file_cfg.timeout_secs.unwrap_or_else(default_timeout_secs),
        }))
    }

    /// Resolve configuration using precedence:
    /// Environment Variables > File Configuration > Local Discovery
    ///
    /// If neither environment nor configuration file specifies a Base URL,
    /// automatic local discovery is executed.
    pub async fn resolve(config_path_override: Option<&Path>) -> Result<Self> {
        Self::resolve_with_discovery(config_path_override, discover_local_9router()).await
    }

    /// Resolves configuration with an injected discovery future for testing.
    pub async fn resolve_with_discovery<F>(
        config_path_override: Option<&Path>,
        discovery_fut: F,
    ) -> Result<Self>
    where
        F: std::future::Future<Output = Option<DiscoveredInstance>>,
    {
        let config_path = Self::resolve_path(config_path_override)?;
        let file_config = FileConfig::load(&config_path)?;

        // 1. Base URL: Env > File > Local Discovery
        let env_base_url =
            env_non_empty("NINEROUTER_URL").or_else(|| env_non_empty("NINEROUTER_BASE_URL"));
        let file_base_url = file_config.as_ref().and_then(|f| f.base_url.clone());

        let (base_url, discovered) = if let Some(env_url) = env_base_url {
            (validate_and_normalize_base_url(&env_url)?, None)
        } else if let Some(file_url) = file_base_url {
            (file_url, None)
        } else {
            let disc = discovery_fut.await;
            match disc {
                Some(d) => (d.base_url.clone(), Some(d)),
                None => {
                    return Err(AppError::Config(
                        "No 9Router URL is configured and no local 9Router instance was detected.\nRun `9router-mcp-web configure` or set NINEROUTER_URL.".to_string(),
                    ));
                }
            }
        };

        // 2. API Key: Env > File > Discovered State
        let env_key =
            env_non_empty("NINEROUTER_KEY").or_else(|| env_non_empty("NINEROUTER_API_KEY"));
        let file_key = file_config.as_ref().and_then(|f| f.api_key.clone());
        let explicit_key = env_key.or(file_key);

        let api_key = match explicit_key {
            Some(key) => Some(key),
            None => {
                if let Some(disc) = &discovered {
                    if disc.is_keyless {
                        None
                    } else {
                        return Err(AppError::Config(format!(
                            "Local 9Router was detected at {}, but Web Search/Web Fetch require an API key.\nSet NINEROUTER_KEY or run `9router-mcp-web configure`.",
                            disc.base_url
                        )));
                    }
                } else {
                    None
                }
            }
        };

        // 3. Search Combo: Env > File > Default
        let search_combo = env_non_empty("NINEROUTER_SEARCH_COMBO")
            .or_else(|| file_config.as_ref().and_then(|f| f.search_combo.clone()))
            .unwrap_or_else(default_search_combo);

        // 4. Fetch Combo: Env > File > Default
        let fetch_combo = env_non_empty("NINEROUTER_FETCH_COMBO")
            .or_else(|| file_config.as_ref().and_then(|f| f.fetch_combo.clone()))
            .unwrap_or_else(default_fetch_combo);

        // 5. Timeout: Env > File > Default
        let timeout_secs = if let Ok(t) = std::env::var("NINEROUTER_TIMEOUT_SECS") {
            let trimmed = t.trim();
            if trimmed.is_empty() {
                file_config
                    .as_ref()
                    .and_then(|f| f.timeout_secs)
                    .unwrap_or_else(default_timeout_secs)
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
            file_config
                .as_ref()
                .and_then(|f| f.timeout_secs)
                .unwrap_or_else(default_timeout_secs)
        };

        Ok(Self {
            base_url,
            api_key,
            search_combo,
            fetch_combo,
            timeout_secs,
        })
    }

    /// Validate and normalize configuration into its canonical form.
    pub fn canonical(&self) -> Result<Self> {
        let base_url = validate_and_normalize_base_url(&self.base_url)?;

        let search_combo = self.search_combo.trim();
        if search_combo.is_empty() {
            return Err(AppError::Config(
                "search_combo cannot be empty or whitespace-only".to_string(),
            ));
        }

        let fetch_combo = self.fetch_combo.trim();
        if fetch_combo.is_empty() {
            return Err(AppError::Config(
                "fetch_combo cannot be empty or whitespace-only".to_string(),
            ));
        }

        if self.timeout_secs == 0 {
            return Err(AppError::Config(
                "timeout_secs must be greater than 0".to_string(),
            ));
        }

        let api_key = self.api_key.as_deref().and_then(|k| {
            let trimmed = k.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });

        Ok(Self {
            base_url,
            api_key,
            search_combo: search_combo.to_string(),
            fetch_combo: fetch_combo.to_string(),
            timeout_secs: self.timeout_secs,
        })
    }

    /// Save configuration securely to disk with mode 0600 on Unix.
    pub fn save(&self, path: &Path) -> Result<()> {
        let persisted = PersistedConfig {
            base_url: Some(self.base_url.clone()),
            api_key: self.api_key.clone(),
            search_combo: self.search_combo.clone(),
            fetch_combo: self.fetch_combo.clone(),
            timeout_secs: self.timeout_secs,
        };
        persisted.save(path)
    }

    /// Check if plaintext HTTP is used with an API key against a non-local host (R-CFG-07).
    pub fn check_plain_http_warning(&self) -> Option<String> {
        check_plain_http_warning_for_url_and_key(&self.base_url, self.api_key.as_deref())
    }

    /// Return a safe, masked representation of the API key for diagnostics.
    pub fn masked_api_key(&self) -> String {
        mask_api_key(self.api_key.as_deref())
    }
}

/// Check if plaintext HTTP is used with an API key against a non-local host.
pub fn check_plain_http_warning_for_url_and_key(
    base_url: &str,
    api_key: Option<&str>,
) -> Option<String> {
    api_key?;

    if let Ok(parsed) = Url::parse(base_url) {
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

/// Masks an API key for safe diagnostics display.
pub fn mask_api_key(api_key: Option<&str>) -> String {
    match api_key {
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

fn prompt_api_key<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    existing_key: Option<&str>,
    use_rpassword: bool,
) -> Result<Option<String>> {
    if let Some(key) = existing_key {
        write!(
            writer,
            "9Router API key [current: {}] (Enter to keep, '-' to clear): ",
            mask_api_key(Some(key))
        )
        .ok();
    } else {
        write!(writer, "9Router API key (optional, press Enter to skip): ").ok();
    }
    writer.flush().ok();

    let raw_key_input = if use_rpassword {
        match rpassword::prompt_password("") {
            Ok(pass) => pass.trim().to_string(),
            Err(_) => {
                let mut line = String::new();
                reader.read_line(&mut line).ok();
                line.trim().to_string()
            }
        }
    } else {
        let mut line = String::new();
        reader.read_line(&mut line).ok();
        line.trim().to_string()
    };

    let api_key = if raw_key_input == "-" || raw_key_input == "none" || raw_key_input == "clear" {
        None
    } else if raw_key_input.is_empty() {
        existing_key.map(|k| k.to_string())
    } else {
        Some(raw_key_input)
    };

    Ok(api_key)
}

/// Run the interactive `configure` CLI flow.
pub async fn run_interactive_configure(config_path_override: Option<&Path>) -> Result<()> {
    run_interactive_configure_with(
        config_path_override,
        &mut io::stdin().lock(),
        &mut io::stderr(),
        discover_local_9router(),
        true,
    )
    .await
}

/// Run the interactive `configure` CLI flow with injectable reader, writer, and discovery future.
pub async fn run_interactive_configure_with<R, W, F>(
    config_path_override: Option<&Path>,
    reader: &mut R,
    writer: &mut W,
    discovery_fut: F,
    use_rpassword: bool,
) -> Result<()>
where
    R: BufRead,
    W: Write,
    F: std::future::Future<Output = Option<DiscoveredInstance>>,
{
    let target_path = Config::resolve_path(config_path_override)?;

    let existing_file = FileConfig::load(&target_path)?;
    let existing_base_url = existing_file.as_ref().and_then(|f| f.base_url.clone());
    let existing_api_key = existing_file.as_ref().and_then(|f| f.api_key.clone());
    let existing_search = existing_file
        .as_ref()
        .and_then(|f| f.search_combo.clone())
        .unwrap_or_else(default_search_combo);
    let existing_fetch = existing_file
        .as_ref()
        .and_then(|f| f.fetch_combo.clone())
        .unwrap_or_else(default_fetch_combo);
    let existing_timeout = existing_file
        .as_ref()
        .and_then(|f| f.timeout_secs)
        .unwrap_or_else(default_timeout_secs);

    writeln!(writer, "=== 9router-mcp-web Configuration ===").ok();
    writeln!(writer, "Config file destination: {}", target_path.display()).ok();
    writeln!(writer).ok();

    // If Base URL is absent, attempt local discovery first
    let discovery = if existing_base_url.is_none() {
        discovery_fut.await
    } else {
        None
    };

    let (base_url_persisted, api_key_persisted, effective_base_url) = match discovery {
        Some(disc) if disc.is_keyless => {
            writeln!(writer, "Detected local 9Router at {}", disc.base_url).ok();
            writeln!(writer, "API key is not required.").ok();
            writeln!(writer).ok();
            // Discovered Base URL remains runtime-derived and is not persisted.
            // Keyless instance does not require an API key.
            (None, None, disc.base_url)
        }
        Some(disc) => {
            writeln!(writer, "Detected local 9Router at {}", disc.base_url).ok();
            writeln!(writer).ok();
            // Discovered Base URL remains runtime-derived, ask for API key
            let api_key =
                prompt_api_key(reader, writer, existing_api_key.as_deref(), use_rpassword)?;
            (None, api_key, disc.base_url)
        }
        None => {
            // Manual flow: prompt for Base URL and API key
            let default_prompt_url = existing_base_url.as_deref().unwrap_or(DEFAULT_BASE_URL);

            write!(
                writer,
                "9Router URL (with or without /v1) [{}]: ",
                default_prompt_url
            )
            .ok();
            writer.flush().ok();
            let mut base_url_input = String::new();
            reader.read_line(&mut base_url_input).ok();
            let base_url_trimmed = base_url_input.trim();
            let base_url = if base_url_trimmed.is_empty() {
                default_prompt_url.to_string()
            } else {
                validate_and_normalize_base_url(base_url_trimmed)?
            };

            let api_key =
                prompt_api_key(reader, writer, existing_api_key.as_deref(), use_rpassword)?;
            (Some(base_url.clone()), api_key, base_url)
        }
    };

    // 3. Search Combo
    write!(writer, "Search combo name [{}]: ", existing_search).ok();
    writer.flush().ok();
    let mut search_input = String::new();
    reader.read_line(&mut search_input).ok();
    let search_trimmed = search_input.trim();
    let search_combo = if search_trimmed.is_empty() {
        existing_search
    } else {
        search_trimmed.to_string()
    };

    // 4. Fetch Combo
    write!(writer, "Fetch combo name [{}]: ", existing_fetch).ok();
    writer.flush().ok();
    let mut fetch_input = String::new();
    reader.read_line(&mut fetch_input).ok();
    let fetch_trimmed = fetch_input.trim();
    let fetch_combo = if fetch_trimmed.is_empty() {
        existing_fetch
    } else {
        fetch_trimmed.to_string()
    };

    // 5. Timeout
    write!(
        writer,
        "Request timeout in seconds [{}]: ",
        existing_timeout
    )
    .ok();
    writer.flush().ok();
    let mut timeout_input = String::new();
    reader.read_line(&mut timeout_input).ok();
    let timeout_trimmed = timeout_input.trim();
    let timeout_secs = if timeout_trimmed.is_empty() {
        existing_timeout
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

    let persisted = PersistedConfig {
        base_url: base_url_persisted,
        api_key: api_key_persisted,
        search_combo,
        fetch_combo,
        timeout_secs,
    };

    if let Some(key) = &persisted.api_key {
        if let Some(warning) =
            check_plain_http_warning_for_url_and_key(&effective_base_url, Some(key))
        {
            writeln!(writer).ok();
            writeln!(writer, "{}", warning).ok();
        }
    }

    persisted.save(&target_path)?;

    writeln!(writer).ok();
    writeln!(
        writer,
        "Configuration saved successfully to: {}",
        target_path.display()
    )
    .ok();
    Ok(())
}

/// Parse the optional configuration file path from command line arguments.
pub fn parse_config_arg(args: &[String]) -> Result<Option<PathBuf>> {
    let cli = crate::cli::Cli::parse(args)?;
    Ok(cli.config_path)
}
