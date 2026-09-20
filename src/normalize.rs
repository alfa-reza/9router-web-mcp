use crate::error::{AppError, Result};
use url::Url;

/// Validates an input URL and normalizes GitHub `/blob/` URLs to their raw equivalent.
///
/// Rules:
/// - Must be an absolute URL with scheme `http` or `https`.
/// - Local filesystem paths (`file://`), custom schemes, or malformed URLs are rejected.
/// - Only URLs strictly matching `https://github.com/{owner}/{repo}/blob/{ref}/{path...}`
///   are rewritten to `https://raw.githubusercontent.com/{owner}/{repo}/{ref}/{path...}`.
/// - UI query parameters and hash fragments on GitHub `/blob/` URLs are stripped.
/// - Other GitHub URLs (tree, issues, pull, releases, repository root) and already-raw
///   URLs are NOT rewritten.
pub fn validate_and_normalize_url(raw_url: &str) -> Result<String> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidUrl("URL cannot be empty".to_string()));
    }

    let parsed =
        Url::parse(trimmed).map_err(|e| AppError::InvalidUrl(format!("Malformed URL: {}", e)))?;

    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(AppError::InvalidUrl(format!(
            "Unsupported URL scheme '{}'. Only http and https are allowed",
            scheme
        )));
    }

    if parsed.host_str().is_none() {
        return Err(AppError::InvalidUrl("URL must include a host".to_string()));
    }

    // Check for GitHub /blob/ rewrite
    if let Some(host) = parsed.host_str() {
        if host.eq_ignore_ascii_case("github.com") {
            if let Some(rewritten) = try_rewrite_github_blob(&parsed) {
                return Ok(rewritten);
            }
        }
    }

    Ok(trimmed.to_string())
}

fn try_rewrite_github_blob(url: &Url) -> Option<String> {
    let segments: Vec<&str> = url.path_segments()?.collect();
    // Path segments for /owner/repo/blob/ref/path...
    // [0: owner, 1: repo, 2: "blob", 3: ref, 4..: path]
    if segments.len() >= 5 && segments[2] == "blob" {
        let owner = segments[0];
        let repo = segments[1];
        let git_ref = segments[3];
        let file_path = segments[4..].join("/");

        if !owner.is_empty() && !repo.is_empty() && !git_ref.is_empty() && !file_path.is_empty() {
            return Some(format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}",
                owner, repo, git_ref, file_path
            ));
        }
    }
    None
}
