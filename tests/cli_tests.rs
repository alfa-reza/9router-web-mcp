use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::tempdir;

fn bin_path() -> &'static str {
    env!("CARGO_BIN_EXE_9router-mcp-web")
}

#[test]
fn test_process_help_long() {
    let output = Command::new(bin_path())
        .arg("--help")
        .output()
        .expect("Failed to execute process");

    assert!(output.status.success(), "Expected exit status 0 for --help");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stdout.contains("9router-mcp-web"),
        "Stdout should contain header"
    );
    assert!(
        stdout.contains("Usage:"),
        "Stdout should contain Usage section"
    );
    assert!(
        stdout.contains("Options:"),
        "Stdout should contain Options section"
    );
    assert!(
        stdout.contains("-c, --config <PATH>"),
        "Stdout should document config option"
    );
    assert!(
        stdout.contains("-h, --help"),
        "Stdout should document help option"
    );
    assert!(
        stdout.contains("-V, --version"),
        "Stdout should document version option"
    );
    assert!(
        stderr.is_empty(),
        "Stderr should be empty for successful --help, got: {}",
        stderr
    );
}

#[test]
fn test_process_help_short() {
    let output = Command::new(bin_path())
        .arg("-h")
        .output()
        .expect("Failed to execute process");

    assert!(output.status.success(), "Expected exit status 0 for -h");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stdout.contains("Usage:"),
        "Stdout should contain Usage section"
    );
    assert!(
        stderr.is_empty(),
        "Stderr should be empty for successful -h, got: {}",
        stderr
    );
}

#[test]
fn test_process_version_long() {
    let output = Command::new(bin_path())
        .arg("--version")
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Expected exit status 0 for --version"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let expected = format!("9router-mcp-web {}\n", env!("CARGO_PKG_VERSION"));
    assert_eq!(stdout, expected, "Stdout must match version string");
    assert!(
        stderr.is_empty(),
        "Stderr should be empty for --version, got: {}",
        stderr
    );
}

#[test]
fn test_process_version_short() {
    let output = Command::new(bin_path())
        .arg("-V")
        .output()
        .expect("Failed to execute process");

    assert!(output.status.success(), "Expected exit status 0 for -V");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let expected = format!("9router-mcp-web {}\n", env!("CARGO_PKG_VERSION"));
    assert_eq!(stdout, expected, "Stdout must match version string");
    assert!(
        stderr.is_empty(),
        "Stderr should be empty for -V, got: {}",
        stderr
    );
}

#[test]
fn test_process_config_missing_path_fails() {
    // --config with no following argument
    let output = Command::new(bin_path())
        .arg("--config")
        .output()
        .expect("Failed to execute process");

    assert!(
        !output.status.success(),
        "Process must fail when --config lacks path"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Flag '--config' requires a path argument"),
        "Stderr: {}",
        stderr
    );

    // -c with no following argument
    let output = Command::new(bin_path())
        .arg("-c")
        .output()
        .expect("Failed to execute process");

    assert!(
        !output.status.success(),
        "Process must fail when -c lacks path"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Flag '-c' requires a path argument"),
        "Stderr: {}",
        stderr
    );

    // --config with empty value
    let output = Command::new(bin_path())
        .args(["--config", ""])
        .output()
        .expect("Failed to execute process");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("requires a non-empty path argument"),
        "Stderr: {}",
        stderr
    );

    // --config= with empty value
    let output = Command::new(bin_path())
        .arg("--config=")
        .output()
        .expect("Failed to execute process");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("requires a non-empty path argument"),
        "Stderr: {}",
        stderr
    );
}

#[test]
fn test_process_config_followed_by_help_fails_without_starting_server() {
    // binary --config --help must NOT treat --help as a file path and must fail
    let output = Command::new(bin_path())
        .args(["--config", "--help"])
        .output()
        .expect("Failed to execute process");

    assert!(!output.status.success(), "--config --help must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Flag '--config' requires a path argument"),
        "Expected missing path diagnostic, got: {}",
        stderr
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("Usage:"),
        "Stdout should not contain help when --config was malformed: {}",
        stdout
    );

    // binary -c --help
    let output = Command::new(bin_path())
        .args(["-c", "--help"])
        .output()
        .expect("Failed to execute process");

    assert!(!output.status.success(), "-c --help must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Flag '-c' requires a path argument"),
        "Stderr: {}",
        stderr
    );
}

#[test]
fn test_process_unknown_or_malformed_arguments_fail() {
    // Unknown option
    let output = Command::new(bin_path())
        .arg("--unknown-flag")
        .output()
        .expect("Failed to execute process");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Unknown option '--unknown-flag'"),
        "Stderr: {}",
        stderr
    );

    // Unknown short option
    let output = Command::new(bin_path())
        .arg("-z")
        .output()
        .expect("Failed to execute process");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown option '-z'"), "Stderr: {}", stderr);

    // Unknown positional argument
    let output = Command::new(bin_path())
        .arg("random_subcommand")
        .output()
        .expect("Failed to execute process");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Unknown command or argument 'random_subcommand'"),
        "Stderr: {}",
        stderr
    );

    // Multiple commands
    let output = Command::new(bin_path())
        .args(["configure", "serve"])
        .output()
        .expect("Failed to execute process");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Multiple commands specified"),
        "Stderr: {}",
        stderr
    );

    // Duplicate config flag
    let output = Command::new(bin_path())
        .args(["--config", "a.toml", "--config", "b.toml"])
        .output()
        .expect("Failed to execute process");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("more than once"), "Stderr: {}", stderr);
}

fn spawn_configure_child(args: &[&str]) -> std::process::Child {
    // Run under setsid so rpassword cannot attach to a controlling terminal and properly falls back to piped stdin
    Command::new("setsid")
        .arg(bin_path())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn process under setsid")
}

#[test]
fn test_process_configure_command_ordering_deterministic() {
    let dir = tempdir().unwrap();

    // 1. binary configure --config <path>
    let config_path1 = dir.path().join("config1.toml");
    let mut child1 =
        spawn_configure_child(&["configure", "--config", config_path1.to_str().unwrap()]);

    {
        let stdin = child1.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(b"\n\n\n\n\n")
            .expect("Failed to write to stdin");
    }

    let output1 = child1.wait_with_output().expect("Failed to wait on child");
    assert!(
        output1.status.success(),
        "configure --config failed: {:?}",
        output1
    );
    let stderr1 = String::from_utf8_lossy(&output1.stderr);
    assert!(
        stderr1.contains(&format!(
            "Config file destination: {}",
            config_path1.display()
        )),
        "Stderr did not contain expected destination: {}",
        stderr1
    );
    assert!(
        config_path1.exists(),
        "Config file must be created at config_path1"
    );

    // 2. binary --config <path> configure (reversed order)
    let config_path2 = dir.path().join("config2.toml");
    let mut child2 =
        spawn_configure_child(&["--config", config_path2.to_str().unwrap(), "configure"]);

    {
        let stdin = child2.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(b"\n\n\n\n\n")
            .expect("Failed to write to stdin");
    }

    let output2 = child2.wait_with_output().expect("Failed to wait on child");
    assert!(
        output2.status.success(),
        "--config configure failed: {:?}",
        output2
    );
    let stderr2 = String::from_utf8_lossy(&output2.stderr);
    assert!(
        stderr2.contains(&format!(
            "Config file destination: {}",
            config_path2.display()
        )),
        "Stderr did not contain expected destination: {}",
        stderr2
    );
    assert!(
        config_path2.exists(),
        "Config file must be created at config_path2"
    );

    // 3. binary --config=<path> configure
    let config_path3 = dir.path().join("config3.toml");
    let config_arg = format!("--config={}", config_path3.display());
    let mut child3 = spawn_configure_child(&[&config_arg, "configure"]);

    {
        let stdin = child3.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(b"\n\n\n\n\n")
            .expect("Failed to write to stdin");
    }

    let output3 = child3.wait_with_output().expect("Failed to wait on child");
    assert!(
        output3.status.success(),
        "--config=path configure failed: {:?}",
        output3
    );
    assert!(
        config_path3.exists(),
        "Config file must be created at config_path3"
    );
}

#[test]
fn test_process_serve_normal_behavior_not_regressed() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("serve_config.toml");
    let config_content = r#"
base_url = "http://127.0.0.1:20128"
search_combo = "search-combo"
fetch_combo = "fetch-combo"
timeout_secs = 30
"#;
    fs::write(&config_path, config_content).unwrap();

    // Launch binary with --config <path> (defaults to serve)
    let mut child = Command::new(bin_path())
        .args(["--config", config_path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start server child process");

    {
        let stdin = child.stdin.as_mut().expect("Failed to open child stdin");
        // Send MCP initialize request over stdio
        let init_req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test-client","version":"1.0"}}}"#;
        writeln!(stdin, "{}", init_req).expect("Failed to write initialize request");
        // Close stdin to trigger graceful shutdown
    }

    let output = child
        .wait_with_output()
        .expect("Failed to wait for server output");
    assert!(
        output.status.success(),
        "Server process did not exit successfully"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Stdout must contain valid JSON-RPC initialize response
    assert!(
        stdout.contains("\"id\":1") && stdout.contains("\"serverInfo\""),
        "Stdout missing initialize response: {}",
        stdout
    );

    // Stderr must contain the startup log from tracing
    assert!(
        stderr.contains("Starting 9router-mcp-web STDIO server"),
        "Stderr missing startup log: {}",
        stderr
    );
}

#[test]
fn test_process_explicit_serve_command_works() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("serve_config2.toml");
    let config_content = r#"
base_url = "http://127.0.0.1:20128"
"#;
    fs::write(&config_path, config_content).unwrap();

    // Launch binary with `serve --config <path>`
    let mut child = Command::new(bin_path())
        .args(["serve", "--config", config_path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start server child process");

    {
        let stdin = child.stdin.as_mut().expect("Failed to open child stdin");
        let init_req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test-client","version":"1.0"}}}"#;
        writeln!(stdin, "{}", init_req).expect("Failed to write initialize request");
    }

    let output = child
        .wait_with_output()
        .expect("Failed to wait for server output");
    assert!(
        output.status.success(),
        "Server process did not exit successfully"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"id\":1") && stdout.contains("\"serverInfo\""),
        "Stdout missing initialize response: {}",
        stdout
    );
}

#[test]
fn test_process_help_documents_transport_and_port() {
    let output = Command::new(bin_path())
        .arg("--help")
        .output()
        .expect("Failed to execute process");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--transport <stdio|http>"),
        "Stdout should document --transport: {}",
        stdout
    );
    assert!(
        stdout.contains("--port <PORT>"),
        "Stdout should document --port: {}",
        stdout
    );
}

#[test]
fn test_process_cli_behavior_matrix_rejections() {
    // 1. --port 3000 without http (defaults to stdio) -> error
    let output = Command::new(bin_path())
        .args(["--port", "3000"])
        .output()
        .expect("Failed to execute process");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("only allowed with '--transport http'"),
        "Stderr: {}",
        stderr
    );

    // 2. --transport stdio --port 3000 -> error
    let output = Command::new(bin_path())
        .args(["--transport", "stdio", "--port", "3000"])
        .output()
        .expect("Failed to execute process");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("only allowed with '--transport http'"),
        "Stderr: {}",
        stderr
    );

    // 3. --transport invalid -> error
    let output = Command::new(bin_path())
        .args(["--transport", "invalid"])
        .output()
        .expect("Failed to execute process");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Invalid transport 'invalid'"),
        "Stderr: {}",
        stderr
    );

    // 4. configure --transport http -> error
    let output = Command::new(bin_path())
        .args(["configure", "--transport", "http"])
        .output()
        .expect("Failed to execute process");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with the 'configure' command"),
        "Stderr: {}",
        stderr
    );

    // 5. configure --port 3000 -> error
    let output = Command::new(bin_path())
        .args(["configure", "--port", "3000"])
        .output()
        .expect("Failed to execute process");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with the 'configure' command"),
        "Stderr: {}",
        stderr
    );

    // 6. port 0 rejected
    let output = Command::new(bin_path())
        .args(["--transport", "http", "--port", "0"])
        .output()
        .expect("Failed to execute process");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("port 0 is not allowed"),
        "Stderr: {}",
        stderr
    );

    // 7. port out of range rejected
    let output = Command::new(bin_path())
        .args(["--transport", "http", "--port", "70000"])
        .output()
        .expect("Failed to execute process");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("port out of range"), "Stderr: {}", stderr);

    // 8. non-numeric port rejected
    let output = Command::new(bin_path())
        .args(["--transport", "http", "--port", "abc"])
        .output()
        .expect("Failed to execute process");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("must be a valid integer"),
        "Stderr: {}",
        stderr
    );

    // 9. duplicate transport rejected
    let output = Command::new(bin_path())
        .args(["--transport", "http", "--transport", "stdio"])
        .output()
        .expect("Failed to execute process");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Option '--transport' was provided more than once"),
        "Stderr: {}",
        stderr
    );

    // 10. duplicate port rejected
    let output = Command::new(bin_path())
        .args(["--transport", "http", "--port", "3000", "--port", "4000"])
        .output()
        .expect("Failed to execute process");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Option '--port' was provided more than once"),
        "Stderr: {}",
        stderr
    );
}

#[test]
fn test_process_transport_explicit_stdio_runs_normally() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("stdio_config.toml");
    let config_content = r#"
base_url = "http://127.0.0.1:20128"
search_combo = "search-combo"
fetch_combo = "fetch-combo"
"#;
    fs::write(&config_path, config_content).unwrap();

    // 1. Launch with `--transport stdio`
    let mut child1 = Command::new(bin_path())
        .args([
            "--transport",
            "stdio",
            "--config",
            config_path.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start child process");

    {
        let stdin = child1.stdin.as_mut().expect("Failed to open child stdin");
        let init_req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test-client","version":"1.0"}}}"#;
        writeln!(stdin, "{}", init_req).expect("Failed to write initialize request");
    }

    let output1 = child1
        .wait_with_output()
        .expect("Failed to wait for server output");
    assert!(output1.status.success());
    let stdout1 = String::from_utf8_lossy(&output1.stdout);
    assert!(stdout1.contains("\"id\":1") && stdout1.contains("\"serverInfo\""));

    // 2. Launch with `serve --transport=stdio`
    let mut child2 = Command::new(bin_path())
        .args([
            "serve",
            "--transport=stdio",
            "--config",
            config_path.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start child process");

    {
        let stdin = child2.stdin.as_mut().expect("Failed to open child stdin");
        let init_req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test-client","version":"1.0"}}}"#;
        writeln!(stdin, "{}", init_req).expect("Failed to write initialize request");
    }

    let output2 = child2
        .wait_with_output()
        .expect("Failed to wait for server output");
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("\"id\":1") && stdout2.contains("\"serverInfo\""));
}

#[test]
fn test_process_http_port_conflict_fails_with_clean_error() {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("Failed to bind ephemeral port");
    let occupied_port = listener.local_addr().unwrap().port();

    let output = Command::new(bin_path())
        .args(["--transport", "http", "--port", &occupied_port.to_string()])
        .output()
        .expect("Failed to execute process");

    assert!(
        !output.status.success(),
        "Process must fail when port is already occupied"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to bind HTTP listener on 127.0.0.1:"),
        "Expected listener bind failure, got stderr: {}",
        stderr
    );
    assert!(
        stderr.contains("Address already in use") || stderr.contains("address already in use"),
        "Expected address in use indication, got stderr: {}",
        stderr
    );

    drop(listener);
}

#[cfg(unix)]
#[test]
fn test_process_http_graceful_shutdown_sigterm() {
    use std::io::{BufRead, BufReader};

    // Find an ephemeral free port
    let ephemeral_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("Failed to bind");
    let free_port = ephemeral_listener.local_addr().unwrap().port();
    drop(ephemeral_listener);

    let mut child = Command::new(bin_path())
        .args(["--transport", "http", "--port", &free_port.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn HTTP server process");

    let pid = child.id();
    let stderr_pipe = child.stderr.take().expect("Failed to capture stderr");
    let mut reader = BufReader::new(stderr_pipe);

    // Wait until startup log is observed
    let mut started = false;
    let mut captured_stderr = String::new();
    let mut line = String::new();
    while let Ok(n) = reader.read_line(&mut line) {
        if n == 0 {
            break;
        }
        captured_stderr.push_str(&line);
        if line.contains("Starting 9router-mcp-web Streamable HTTP server") {
            started = true;
            break;
        }
        line.clear();
    }
    assert!(
        started,
        "Server should have logged startup line, got: {}",
        captured_stderr
    );

    // Send SIGTERM to the child process
    let kill_res = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .output()
        .expect("Failed to execute kill -TERM");
    assert!(kill_res.status.success(), "kill command should succeed");

    // Read remaining stderr
    while let Ok(n) = reader.read_line(&mut line) {
        if n == 0 {
            break;
        }
        captured_stderr.push_str(&line);
        line.clear();
    }

    let status = child.wait().expect("Failed to wait on child process");
    assert!(
        status.success(),
        "Server should exit cleanly with status 0, was: {:?}",
        status
    );
    assert!(
        captured_stderr.contains("Received SIGTERM, initiating graceful shutdown"),
        "Stderr missing SIGTERM log: {}",
        captured_stderr
    );
    assert!(
        captured_stderr.contains("MCP HTTP server stopped gracefully"),
        "Stderr missing stopped gracefully log: {}",
        captured_stderr
    );
}
