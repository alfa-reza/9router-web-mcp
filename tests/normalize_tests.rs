use ninerouter_mcp_web::error::AppError;
use ninerouter_mcp_web::normalize::validate_and_normalize_url;

#[test]
fn test_valid_github_blob_urls_rewritten() {
    // Standard main branch
    let url = "https://github.com/owner/repo/blob/main/README.md";
    assert_eq!(
        validate_and_normalize_url(url).unwrap(),
        "https://raw.githubusercontent.com/owner/repo/main/README.md"
    );

    // Commit hash ref
    let url_commit =
        "https://github.com/owner/repo/blob/d238e3fd0cd45bcce32a95ba09f7a3ece736e6e2/src/main.rs";
    assert_eq!(
        validate_and_normalize_url(url_commit).unwrap(),
        "https://raw.githubusercontent.com/owner/repo/d238e3fd0cd45bcce32a95ba09f7a3ece736e6e2/src/main.rs"
    );

    // Nested directories and multiple dots
    let url_nested = "https://github.com/owner/repo/blob/v1.2.3/path/to/deep/config.test.json";
    assert_eq!(
        validate_and_normalize_url(url_nested).unwrap(),
        "https://raw.githubusercontent.com/owner/repo/v1.2.3/path/to/deep/config.test.json"
    );

    // Stripping query and fragment
    let url_query = "https://github.com/owner/repo/blob/main/src/lib.rs?plain=1#L10-L20";
    assert_eq!(
        validate_and_normalize_url(url_query).unwrap(),
        "https://raw.githubusercontent.com/owner/repo/main/src/lib.rs"
    );
}

#[test]
fn test_github_urls_not_rewritten() {
    // Repository root
    let root = "https://github.com/owner/repo";
    assert_eq!(validate_and_normalize_url(root).unwrap(), root);

    // Tree URL
    let tree = "https://github.com/owner/repo/tree/main/src";
    assert_eq!(validate_and_normalize_url(tree).unwrap(), tree);

    // Issue URL
    let issue = "https://github.com/owner/repo/issues/42";
    assert_eq!(validate_and_normalize_url(issue).unwrap(), issue);

    // Pull request URL
    let pr = "https://github.com/owner/repo/pull/100";
    assert_eq!(validate_and_normalize_url(pr).unwrap(), pr);

    // Releases URL
    let release = "https://github.com/owner/repo/releases/tag/v1.0.0";
    assert_eq!(validate_and_normalize_url(release).unwrap(), release);

    // Already raw URL
    let already_raw = "https://raw.githubusercontent.com/owner/repo/main/README.md";
    assert_eq!(
        validate_and_normalize_url(already_raw).unwrap(),
        already_raw
    );
}

#[test]
fn test_non_github_urls_not_rewritten() {
    let regular_url = "https://example.com/some/path/blob/test.html";
    assert_eq!(
        validate_and_normalize_url(regular_url).unwrap(),
        regular_url
    );
}

#[test]
fn test_invalid_urls_rejected() {
    // Empty
    assert!(matches!(
        validate_and_normalize_url(""),
        Err(AppError::InvalidUrl(_))
    ));
    assert!(matches!(
        validate_and_normalize_url("   "),
        Err(AppError::InvalidUrl(_))
    ));

    // Local file
    assert!(matches!(
        validate_and_normalize_url("file:///etc/passwd"),
        Err(AppError::InvalidUrl(_))
    ));

    // Custom scheme
    assert!(matches!(
        validate_and_normalize_url("gopher://example.com"),
        Err(AppError::InvalidUrl(_))
    ));
    assert!(matches!(
        validate_and_normalize_url("ftp://example.com/file"),
        Err(AppError::InvalidUrl(_))
    ));

    // Malformed
    assert!(matches!(
        validate_and_normalize_url("not a url"),
        Err(AppError::InvalidUrl(_))
    ));
}
