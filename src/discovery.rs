use std::path::Path;
use std::time::Duration;
use url::Url;

use crate::error::{AppError, Result};

pub const DEFAULT_LOCAL_URL: &str = "http://127.0.0.1:20128";
pub const DEFAULT_LOCAL_PORT: u16 = 20128;
pub const DISCOVERY_TIMEOUT_MS: u64 = 1500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredInstance {
    pub base_url: String,
    pub is_keyless: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthProbeResult {
    Keyless,
    AuthRequired,
    Ambiguous,
}

#[derive(serde::Deserialize)]
struct HealthResponseBody {
    ok: bool,
}

/// Builds an HTTP client tailored for local discovery:
/// - Redirects are strictly rejected to prevent loopback redirects to remote hosts.
/// - Connect and request timeouts are short and bounded.
pub fn discovery_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_millis(DISCOVERY_TIMEOUT_MS))
        .timeout(Duration::from_millis(DISCOVERY_TIMEOUT_MS))
        .build()
        .map_err(|e| AppError::Config(format!("Failed to build discovery HTTP client: {}", e)))
}

/// Checks whether a candidate Base URL responds to `GET /api/health` with `{"ok": true}`.
pub async fn check_health(client: &reqwest::Client, base_url: &str) -> bool {
    let clean_base = base_url.trim_end_matches('/');
    let health_url = format!("{}/api/health", clean_base);

    let res = match client.get(&health_url).send().await {
        Ok(r) => r,
        Err(_) => return false,
    };

    if !res.status().is_success() {
        return false;
    }

    match res.json::<HealthResponseBody>().await {
        Ok(body) => body.ok,
        Err(_) => false,
    }
}

/// Probes a specific endpoint with an empty JSON object to evaluate authentication requirements.
///
/// In 9Router, the Search (`/v1/search`) and Fetch (`/v1/web/fetch`) handlers evaluate API key
/// enforcement (`requireApiKey`) before parameter validation.
///
/// If an API key is required:
/// - Returns 401 or 403 Unauthorized ("Missing API key").
///
/// If keyless (API key not required):
/// - Passes the auth gate and returns 400 Bad Request with missing provider validation:
///   `"Missing required field: provider (or model)"`.
///
/// Any other status or unexpected error message is treated conservatively as Ambiguous.
pub async fn probe_endpoint_auth(client: &reqwest::Client, endpoint_url: &str) -> AuthProbeResult {
    let res = match client
        .post(endpoint_url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body("{}")
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return AuthProbeResult::Ambiguous,
    };

    let status = res.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return AuthProbeResult::AuthRequired;
    }

    if status == reqwest::StatusCode::BAD_REQUEST {
        if let Ok(val) = res.json::<serde_json::Value>().await {
            let msg = val
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("");

            if msg.contains("Missing required field: provider") {
                return AuthProbeResult::Keyless;
            }
        }
    }

    AuthProbeResult::Ambiguous
}

/// Constructs full endpoint URLs preserving or appending the `/v1` segment safely.
fn endpoint_url(base_url: &str, subpath: &str) -> String {
    let clean_subpath = subpath.strip_prefix('/').unwrap_or(subpath);
    let subpath_without_v1 = clean_subpath.strip_prefix("v1/").unwrap_or(clean_subpath);

    if let Ok(mut url) = Url::parse(base_url) {
        let has_v1_suffix = url
            .path_segments()
            .and_then(|mut segs| segs.rfind(|s| !s.is_empty()))
            == Some("v1");

        let segments: Vec<&str> = if has_v1_suffix {
            subpath_without_v1
                .split('/')
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            std::iter::once("v1")
                .chain(subpath_without_v1.split('/').filter(|s| !s.is_empty()))
                .collect()
        };

        let mutated = match url.path_segments_mut() {
            Ok(mut path_segs) => {
                path_segs.pop_if_empty();
                path_segs.extend(&segments);
                true
            }
            Err(_) => false,
        };

        if mutated {
            return url.to_string();
        }
    }

    let clean_base = base_url.trim_end_matches('/');
    if clean_base.ends_with("/v1") {
        format!("{}/{}", clean_base, subpath_without_v1)
    } else {
        format!("{}/v1/{}", clean_base, subpath_without_v1)
    }
}

/// Tests whether the discovered local 9Router instance is keyless for both Search and Fetch.
///
/// Both endpoints must pass the keyless check. If either requires authentication or returns
/// ambiguous responses, API key authentication is assumed.
pub async fn detect_keyless(client: &reqwest::Client, base_url: &str) -> bool {
    let search_url = endpoint_url(base_url, "v1/search");
    let fetch_url = endpoint_url(base_url, "v1/web/fetch");

    let search_result = probe_endpoint_auth(client, &search_url).await;
    let fetch_result = probe_endpoint_auth(client, &fetch_url).await;

    search_result == AuthProbeResult::Keyless && fetch_result == AuthProbeResult::Keyless
}

/// Checks whether an executable binary exists on the provided PATH string.
///
/// Performs a passive inspection without launching or executing the application.
pub fn is_binary_on_path_in(binary_name: &str, path_env: Option<&std::ffi::OsStr>) -> bool {
    let path_val = match path_env {
        Some(p) => p,
        None => return false,
    };

    for dir in std::env::split_paths(path_val) {
        let candidate = dir.join(binary_name);
        if candidate.is_file() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = candidate.metadata() {
                    if meta.permissions().mode() & 0o111 != 0 {
                        return true;
                    }
                }
            }
            #[cfg(not(unix))]
            {
                return true;
            }
        }
    }

    false
}

/// Checks whether the `9router` executable is installed on `PATH`.
pub fn is_9router_installed() -> bool {
    is_binary_on_path_in("9router", std::env::var_os("PATH").as_deref())
}

/// Inspects process command-line arguments to determine if they correspond to an active 9Router launcher
/// and extracts an explicit custom port if present.
///
/// Requirements:
/// - Rejects `9router-mcp-web` and unrelated processes.
/// - For `node`/`bun`, requires the script target to be a 9Router launcher.
/// - Parses `-p <port>`, `--port <port>`, `-p=<port>`, and `--port=<port>`.
/// - Rejects non-loopback bind hosts (e.g. LAN/external IP).
/// - Rejects port 20128 because the default port was already checked in Step 1.
pub fn extract_custom_port_from_cmdline<T: AsRef<str>>(args: &[T]) -> Option<u16> {
    if args.is_empty() {
        return None;
    }

    let prog = args[0].as_ref();
    let prog_name = Path::new(prog)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(prog);

    if prog_name == "9router-mcp-web" || prog_name.starts_with("9router-mcp-web") {
        return None;
    }

    let is_launcher = if prog_name == "9router" {
        true
    } else if prog_name == "node" || prog_name == "bun" {
        let mut found_script = false;
        for arg in &args[1..] {
            let a = arg.as_ref();
            if a.starts_with('-') {
                continue;
            }
            let script_path = Path::new(a);
            let script_name = script_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(a);

            if script_name == "9router" {
                found_script = true;
                break;
            }
            if script_name == "cli.js" {
                let full_path = a.replace('\\', "/");
                if full_path.contains("9router") || full_path.contains(".9router") {
                    found_script = true;
                    break;
                }
            }
            // First non-flag argument was another script
            break;
        }
        found_script
    } else {
        false
    };

    if !is_launcher {
        return None;
    }

    let mut custom_port: Option<u16> = None;
    let mut bound_host: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_ref();
        if arg == "-p" || arg == "--port" {
            if i + 1 < args.len() {
                if let Ok(p) = args[i + 1].as_ref().trim().parse::<u16>() {
                    if p > 0 {
                        custom_port = Some(p);
                    }
                }
                i += 2;
                continue;
            }
        } else if let Some(val) = arg
            .strip_prefix("-p=")
            .or_else(|| arg.strip_prefix("--port="))
        {
            if let Ok(p) = val.trim().parse::<u16>() {
                if p > 0 {
                    custom_port = Some(p);
                }
            }
        } else if arg == "-H" || arg == "--host" {
            if i + 1 < args.len() {
                bound_host = Some(args[i + 1].as_ref().trim().to_string());
                i += 2;
                continue;
            }
        } else if let Some(val) = arg
            .strip_prefix("-H=")
            .or_else(|| arg.strip_prefix("--host="))
        {
            bound_host = Some(val.trim().to_string());
        }
        i += 1;
    }

    // Host verification: if process is explicitly bound only to a non-loopback host, do not auto-adopt
    if let Some(host) = bound_host {
        let h_lower = host.to_lowercase();
        let is_allowed_loopback_or_wildcard = h_lower == "0.0.0.0"
            || h_lower == "127.0.0.1"
            || h_lower == "localhost"
            || h_lower == "::1"
            || h_lower == "[::1]";

        if !is_allowed_loopback_or_wildcard {
            return None;
        }
    }

    // Only return a port if it differs from the default 20128 already tested in Step 1
    match custom_port {
        Some(p) if p != DEFAULT_LOCAL_PORT => Some(p),
        _ => None,
    }
}

/// Reads Linux `/proc` table safely and in a read-only manner to detect an active 9Router launcher with a custom port.
#[cfg(target_os = "linux")]
pub fn find_custom_port_from_active_processes() -> Option<u16> {
    let proc_dir = match std::fs::read_dir("/proc") {
        Ok(d) => d,
        Err(_) => return None,
    };

    for entry in proc_dir.flatten() {
        let file_name = entry.file_name();
        let name_str = match file_name.to_str() {
            Some(s) => s,
            None => continue,
        };

        if !name_str.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        let cmdline_path = entry.path().join("cmdline");
        let content = match std::fs::read(cmdline_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if content.is_empty() {
            continue;
        }

        let tokens: Vec<String> = content
            .split(|&b| b == 0)
            .filter(|slice| !slice.is_empty())
            .map(|slice| String::from_utf8_lossy(slice).to_string())
            .collect();

        if let Some(port) = extract_custom_port_from_cmdline(&tokens) {
            return Some(port);
        }
    }

    None
}

#[cfg(not(target_os = "linux"))]
pub fn find_custom_port_from_active_processes() -> Option<u16> {
    None
}

/// Executes the full 9Router local discovery algorithm:
/// 1. Try default endpoint `http://127.0.0.1:20128/api/health`.
/// 2. If default fails, check if `9router` binary is on PATH.
/// 3. If binary exists, check for an active running 9Router launcher with an explicit custom port.
/// 4. Stop if none found.
pub async fn discover_local_9router() -> Option<DiscoveredInstance> {
    let client = match discovery_http_client() {
        Ok(c) => c,
        Err(_) => return None,
    };

    // Step 1: Default local endpoint
    if check_health(&client, DEFAULT_LOCAL_URL).await {
        let is_keyless = detect_keyless(&client, DEFAULT_LOCAL_URL).await;
        return Some(DiscoveredInstance {
            base_url: DEFAULT_LOCAL_URL.to_string(),
            is_keyless,
        });
    }

    // Step 2: Check whether 9router binary exists on PATH
    if !is_9router_installed() {
        return None;
    }

    // Step 3: Check for an active running launcher with custom port
    if let Some(custom_port) = find_custom_port_from_active_processes() {
        let candidate_url = format!("http://127.0.0.1:{}", custom_port);
        if check_health(&client, &candidate_url).await {
            let is_keyless = detect_keyless(&client, &candidate_url).await;
            return Some(DiscoveredInstance {
                base_url: candidate_url,
                is_keyless,
            });
        }
    }

    // Step 4: Stop
    None
}

/// Testable variant of `discover_local_9router` allowing injected binary check and custom port resolver.
pub async fn discover_local_9router_with<F>(
    client: &reqwest::Client,
    default_url: &str,
    binary_installed: bool,
    custom_port_finder: F,
) -> Option<DiscoveredInstance>
where
    F: FnOnce() -> Option<u16>,
{
    // Step 1: Default local endpoint
    if check_health(client, default_url).await {
        let is_keyless = detect_keyless(client, default_url).await;
        return Some(DiscoveredInstance {
            base_url: default_url.to_string(),
            is_keyless,
        });
    }

    // Step 2: Check whether 9router binary exists on PATH
    if !binary_installed {
        return None;
    }

    // Step 3: Check for an active running launcher with custom port
    if let Some(custom_port) = custom_port_finder() {
        let candidate_url = format!("http://127.0.0.1:{}", custom_port);
        if check_health(client, &candidate_url).await {
            let is_keyless = detect_keyless(client, &candidate_url).await;
            return Some(DiscoveredInstance {
                base_url: candidate_url,
                is_keyless,
            });
        }
    }

    // Step 4: Stop
    None
}
