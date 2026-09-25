use std::path::PathBuf;

use crate::error::{AppError, Result};

pub const HELP_TEXT: &str = concat!(
    "9router-mcp-web ",
    env!("CARGO_PKG_VERSION"),
    "\n",
    "Model Context Protocol (MCP) server for 9Router Web Search and Web Fetch.\n",
    "\n",
    "Usage:\n",
    "  9router-mcp-web [OPTIONS]            Start MCP server (STDIO by default)\n",
    "  9router-mcp-web serve [OPTIONS]      Start MCP server (STDIO by default)\n",
    "  9router-mcp-web configure [OPTIONS]  Run interactive configuration wizard\n",
    "\n",
    "Options:\n",
    "  -c, --config <PATH>       Path to configuration file\n",
    "      --transport <stdio|http>\n",
    "                            Transport to use [default: stdio]\n",
    "      --port <PORT>         HTTP listen port (valid only with --transport http) [default: 20129]\n",
    "  -h, --help                Print help\n",
    "  -V, --version             Print version\n",
);

pub const VERSION_TEXT: &str = concat!("9router-mcp-web ", env!("CARGO_PKG_VERSION"));
pub const DEFAULT_HTTP_PORT: u16 = 20129;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transport {
    #[default]
    Stdio,
    Http,
}

impl Transport {
    pub fn as_str(&self) -> &'static str {
        match self {
            Transport::Stdio => "stdio",
            Transport::Http => "http",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliCommand {
    Serve,
    Configure,
    Help,
    Version,
}

impl CliCommand {
    pub fn as_str(&self) -> &'static str {
        match self {
            CliCommand::Serve => "serve",
            CliCommand::Configure => "configure",
            CliCommand::Help => "help",
            CliCommand::Version => "version",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    pub command: CliCommand,
    pub config_path: Option<PathBuf>,
    pub transport: Transport,
    pub port: Option<u16>,
}

impl Cli {
    /// Return the configured HTTP port or default (20129).
    pub fn effective_port(&self) -> u16 {
        self.port.unwrap_or(DEFAULT_HTTP_PORT)
    }

    /// Parse CLI arguments from a full argv slice (including argv[0] binary name).
    pub fn parse<T: AsRef<str>>(args: &[T]) -> Result<Self> {
        let tokens = if args.is_empty() { &[] } else { &args[1..] };
        Self::parse_tokens(tokens)
    }

    /// Parse CLI arguments from an iterator of arguments (including argv[0]).
    pub fn parse_from<I, T>(iter: I) -> Result<Self>
    where
        I: IntoIterator<Item = T>,
        T: AsRef<str>,
    {
        let args: Vec<String> = iter.into_iter().map(|s| s.as_ref().to_string()).collect();
        Self::parse(&args)
    }

    /// Parse CLI tokens excluding argv[0].
    pub fn parse_tokens<T: AsRef<str>>(tokens: &[T]) -> Result<Self> {
        let mut command: Option<CliCommand> = None;
        let mut config_path: Option<PathBuf> = None;
        let mut transport: Option<Transport> = None;
        let mut port: Option<u16> = None;
        let mut help_requested = false;
        let mut version_requested = false;

        let mut i = 0;
        while i < tokens.len() {
            let token = tokens[i].as_ref();

            if token == "--help" || token == "-h" {
                help_requested = true;
                i += 1;
            } else if token == "--version" || token == "-V" {
                version_requested = true;
                i += 1;
            } else if token == "--config" || token == "-c" {
                if config_path.is_some() {
                    return Err(AppError::Config(format!(
                        "Option '{}' was provided more than once",
                        token
                    )));
                }
                if i + 1 >= tokens.len() {
                    return Err(AppError::Config(format!(
                        "Flag '{}' requires a path argument",
                        token
                    )));
                }
                let next_tok = tokens[i + 1].as_ref();
                let trimmed = next_tok.trim();
                if trimmed.is_empty() {
                    return Err(AppError::Config(format!(
                        "Flag '{}' requires a non-empty path argument",
                        token
                    )));
                }
                if trimmed.starts_with('-') {
                    return Err(AppError::Config(format!(
                        "Flag '{}' requires a path argument, but found option '{}'",
                        token, next_tok
                    )));
                }
                config_path = Some(PathBuf::from(trimmed));
                i += 2;
            } else if let Some(stripped) = token.strip_prefix("--config=") {
                if config_path.is_some() {
                    return Err(AppError::Config(
                        "Option '--config' was provided more than once".to_string(),
                    ));
                }
                let trimmed = stripped.trim();
                if trimmed.is_empty() {
                    return Err(AppError::Config(
                        "Flag '--config=' requires a non-empty path argument".to_string(),
                    ));
                }
                if trimmed.starts_with('-') {
                    return Err(AppError::Config(format!(
                        "Flag '--config=' requires a valid path argument, but found option '{}'",
                        trimmed
                    )));
                }
                config_path = Some(PathBuf::from(trimmed));
                i += 1;
            } else if let Some(stripped) = token.strip_prefix("-c=") {
                if config_path.is_some() {
                    return Err(AppError::Config(
                        "Option '-c' was provided more than once".to_string(),
                    ));
                }
                let trimmed = stripped.trim();
                if trimmed.is_empty() {
                    return Err(AppError::Config(
                        "Flag '-c=' requires a non-empty path argument".to_string(),
                    ));
                }
                if trimmed.starts_with('-') {
                    return Err(AppError::Config(format!(
                        "Flag '-c=' requires a valid path argument, but found option '{}'",
                        trimmed
                    )));
                }
                config_path = Some(PathBuf::from(trimmed));
                i += 1;
            } else if token == "--transport" {
                if transport.is_some() {
                    return Err(AppError::Config(
                        "Option '--transport' was provided more than once".to_string(),
                    ));
                }
                if i + 1 >= tokens.len() {
                    return Err(AppError::Config(
                        "Flag '--transport' requires a value ('stdio' or 'http')".to_string(),
                    ));
                }
                let next_tok = tokens[i + 1].as_ref();
                let trimmed = next_tok.trim();
                if trimmed.is_empty() {
                    return Err(AppError::Config(
                        "Flag '--transport' requires a non-empty value ('stdio' or 'http')"
                            .to_string(),
                    ));
                }
                if trimmed.starts_with('-') {
                    return Err(AppError::Config(format!(
                        "Flag '--transport' requires a value, but found option '{}'",
                        next_tok
                    )));
                }
                transport = Some(parse_transport(trimmed)?);
                i += 2;
            } else if let Some(stripped) = token.strip_prefix("--transport=") {
                if transport.is_some() {
                    return Err(AppError::Config(
                        "Option '--transport' was provided more than once".to_string(),
                    ));
                }
                let trimmed = stripped.trim();
                if trimmed.is_empty() {
                    return Err(AppError::Config(
                        "Flag '--transport=' requires a non-empty value ('stdio' or 'http')"
                            .to_string(),
                    ));
                }
                if trimmed.starts_with('-') {
                    return Err(AppError::Config(format!(
                        "Flag '--transport=' requires a valid value, but found option '{}'",
                        trimmed
                    )));
                }
                transport = Some(parse_transport(trimmed)?);
                i += 1;
            } else if token == "--port" {
                if port.is_some() {
                    return Err(AppError::Config(
                        "Option '--port' was provided more than once".to_string(),
                    ));
                }
                if i + 1 >= tokens.len() {
                    return Err(AppError::Config(
                        "Flag '--port' requires a port argument".to_string(),
                    ));
                }
                let next_tok = tokens[i + 1].as_ref();
                let trimmed = next_tok.trim();
                if trimmed.is_empty() {
                    return Err(AppError::Config(
                        "Flag '--port' requires a non-empty port argument".to_string(),
                    ));
                }
                if trimmed.starts_with('-') {
                    return Err(AppError::Config(format!(
                        "Flag '--port' requires a port argument, but found option '{}'",
                        next_tok
                    )));
                }
                port = Some(parse_port(trimmed)?);
                i += 2;
            } else if let Some(stripped) = token.strip_prefix("--port=") {
                if port.is_some() {
                    return Err(AppError::Config(
                        "Option '--port' was provided more than once".to_string(),
                    ));
                }
                let trimmed = stripped.trim();
                if trimmed.is_empty() {
                    return Err(AppError::Config(
                        "Flag '--port=' requires a non-empty port argument".to_string(),
                    ));
                }
                if trimmed.starts_with('-') {
                    return Err(AppError::Config(format!(
                        "Flag '--port=' requires a valid port argument, but found option '{}'",
                        trimmed
                    )));
                }
                port = Some(parse_port(trimmed)?);
                i += 1;
            } else if token.starts_with('-') {
                return Err(AppError::Config(format!("Unknown option '{}'", token)));
            } else {
                let new_cmd = match token {
                    "configure" => CliCommand::Configure,
                    "serve" => CliCommand::Serve,
                    _ => {
                        return Err(AppError::Config(format!(
                            "Unknown command or argument '{}'",
                            token
                        )));
                    }
                };

                if let Some(existing) = &command {
                    if existing == &new_cmd {
                        return Err(AppError::Config(format!(
                            "Command '{}' specified more than once",
                            token
                        )));
                    } else {
                        return Err(AppError::Config(format!(
                            "Multiple commands specified: '{}' and '{}'",
                            existing.as_str(),
                            token
                        )));
                    }
                }
                command = Some(new_cmd);
                i += 1;
            }
        }

        if help_requested {
            return Ok(Self {
                command: CliCommand::Help,
                config_path,
                transport: transport.unwrap_or_default(),
                port,
            });
        }

        if version_requested {
            return Ok(Self {
                command: CliCommand::Version,
                config_path,
                transport: transport.unwrap_or_default(),
                port,
            });
        }

        let final_command = command.unwrap_or(CliCommand::Serve);
        if final_command == CliCommand::Configure {
            if transport.is_some() || port.is_some() {
                return Err(AppError::Config(
                    "Options '--transport' and '--port' cannot be used with the 'configure' command"
                        .to_string(),
                ));
            }
        } else if final_command == CliCommand::Serve {
            let effective_transport = transport.unwrap_or(Transport::Stdio);
            if port.is_some() && effective_transport != Transport::Http {
                return Err(AppError::Config(
                    "Option '--port' is only allowed with '--transport http'".to_string(),
                ));
            }
        }

        Ok(Self {
            command: final_command,
            config_path,
            transport: transport.unwrap_or(Transport::Stdio),
            port,
        })
    }
}

fn parse_transport(val: &str) -> Result<Transport> {
    match val {
        "stdio" => Ok(Transport::Stdio),
        "http" => Ok(Transport::Http),
        other => Err(AppError::Config(format!(
            "Invalid transport '{}'. Valid values are 'stdio' or 'http'",
            other
        ))),
    }
}

fn parse_port(val: &str) -> Result<u16> {
    let num: i64 = val.parse().map_err(|_| {
        AppError::Config(format!("Invalid port '{}': must be a valid integer", val))
    })?;
    if num == 0 {
        return Err(AppError::Config(
            "Port must be between 1 and 65535 (port 0 is not allowed)".to_string(),
        ));
    }
    if !(1..=65535).contains(&num) {
        return Err(AppError::Config(format!(
            "Invalid port '{}': port out of range (1-65535)",
            val
        )));
    }
    Ok(num as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_args_defaults_to_serve() {
        let cli = Cli::parse(&["9router-mcp-web"]).unwrap();
        assert_eq!(cli.command, CliCommand::Serve);
        assert_eq!(cli.config_path, None);

        let cli_empty = Cli::parse::<&str>(&[]).unwrap();
        assert_eq!(cli_empty.command, CliCommand::Serve);
        assert_eq!(cli_empty.config_path, None);
    }

    #[test]
    fn test_explicit_serve_command() {
        let cli = Cli::parse(&["9router-mcp-web", "serve"]).unwrap();
        assert_eq!(cli.command, CliCommand::Serve);
        assert_eq!(cli.config_path, None);

        let cli_config = Cli::parse(&["9router-mcp-web", "serve", "--config", "c.toml"]).unwrap();
        assert_eq!(cli_config.command, CliCommand::Serve);
        assert_eq!(cli_config.config_path, Some(PathBuf::from("c.toml")));

        let cli_config_first =
            Cli::parse(&["9router-mcp-web", "--config", "c.toml", "serve"]).unwrap();
        assert_eq!(cli_config_first.command, CliCommand::Serve);
        assert_eq!(cli_config_first.config_path, Some(PathBuf::from("c.toml")));
    }

    #[test]
    fn test_help_forms() {
        let cli_long = Cli::parse(&["9router-mcp-web", "--help"]).unwrap();
        assert_eq!(cli_long.command, CliCommand::Help);

        let cli_short = Cli::parse(&["9router-mcp-web", "-h"]).unwrap();
        assert_eq!(cli_short.command, CliCommand::Help);

        let cli_with_config =
            Cli::parse(&["9router-mcp-web", "--config", "cfg.toml", "--help"]).unwrap();
        assert_eq!(cli_with_config.command, CliCommand::Help);
        assert_eq!(cli_with_config.config_path, Some(PathBuf::from("cfg.toml")));

        let cli_with_cmd = Cli::parse(&["9router-mcp-web", "configure", "-h"]).unwrap();
        assert_eq!(cli_with_cmd.command, CliCommand::Help);
    }

    #[test]
    fn test_version_forms() {
        let cli_long = Cli::parse(&["9router-mcp-web", "--version"]).unwrap();
        assert_eq!(cli_long.command, CliCommand::Version);

        let cli_short = Cli::parse(&["9router-mcp-web", "-V"]).unwrap();
        assert_eq!(cli_short.command, CliCommand::Version);
    }

    #[test]
    fn test_configure_forms() {
        let cli = Cli::parse(&["9router-mcp-web", "configure"]).unwrap();
        assert_eq!(cli.command, CliCommand::Configure);
        assert_eq!(cli.config_path, None);

        let cli_config_after =
            Cli::parse(&["9router-mcp-web", "configure", "--config", "file.toml"]).unwrap();
        assert_eq!(cli_config_after.command, CliCommand::Configure);
        assert_eq!(
            cli_config_after.config_path,
            Some(PathBuf::from("file.toml"))
        );

        let cli_config_before =
            Cli::parse(&["9router-mcp-web", "--config", "file.toml", "configure"]).unwrap();
        assert_eq!(cli_config_before.command, CliCommand::Configure);
        assert_eq!(
            cli_config_before.config_path,
            Some(PathBuf::from("file.toml"))
        );

        let cli_config_eq =
            Cli::parse(&["9router-mcp-web", "configure", "--config=file.toml"]).unwrap();
        assert_eq!(cli_config_eq.command, CliCommand::Configure);
        assert_eq!(cli_config_eq.config_path, Some(PathBuf::from("file.toml")));

        let cli_short_config =
            Cli::parse(&["9router-mcp-web", "-c", "file.toml", "configure"]).unwrap();
        assert_eq!(cli_short_config.command, CliCommand::Configure);
        assert_eq!(
            cli_short_config.config_path,
            Some(PathBuf::from("file.toml"))
        );
    }

    #[test]
    fn test_config_without_command_defaults_to_serve() {
        let cli_space = Cli::parse(&["9router-mcp-web", "--config", "file.toml"]).unwrap();
        assert_eq!(cli_space.command, CliCommand::Serve);
        assert_eq!(cli_space.config_path, Some(PathBuf::from("file.toml")));

        let cli_eq = Cli::parse(&["9router-mcp-web", "--config=file.toml"]).unwrap();
        assert_eq!(cli_eq.command, CliCommand::Serve);
        assert_eq!(cli_eq.config_path, Some(PathBuf::from("file.toml")));

        let cli_short = Cli::parse(&["9router-mcp-web", "-c", "file.toml"]).unwrap();
        assert_eq!(cli_short.command, CliCommand::Serve);
        assert_eq!(cli_short.config_path, Some(PathBuf::from("file.toml")));

        let cli_short_eq = Cli::parse(&["9router-mcp-web", "-c=file.toml"]).unwrap();
        assert_eq!(cli_short_eq.command, CliCommand::Serve);
        assert_eq!(cli_short_eq.config_path, Some(PathBuf::from("file.toml")));
    }

    #[test]
    fn test_config_missing_or_empty_path_fails() {
        assert!(Cli::parse(&["9router-mcp-web", "--config"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "-c"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--config", ""]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--config", "   "]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "-c", ""]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "-c", "   "]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--config="]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--config=   "]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "-c="]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "-c=   "]).is_err());
    }

    #[test]
    fn test_config_followed_by_option_flag_fails() {
        // --config --help must not treat --help as a file path
        assert!(Cli::parse(&["9router-mcp-web", "--config", "--help"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "-c", "--help"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--config", "-h"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--config", "-c"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--config", "--version"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--config", "-V"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--config=--help"]).is_err());
    }

    #[test]
    fn test_unknown_option_or_argument_fails() {
        assert!(Cli::parse(&["9router-mcp-web", "--unknown"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "-x"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "unknown_cmd"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "configure", "extra"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "serve", "extra"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--config", "cfg.toml", "extra"]).is_err());
    }

    #[test]
    fn test_duplicate_options_or_commands_fail() {
        assert!(Cli::parse(&[
            "9router-mcp-web",
            "--config",
            "a.toml",
            "--config",
            "b.toml"
        ])
        .is_err());
        assert!(Cli::parse(&["9router-mcp-web", "-c", "a.toml", "--config", "b.toml"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--config=a.toml", "--config=b.toml"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "configure", "serve"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "serve", "configure"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "configure", "configure"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "serve", "serve"]).is_err());
    }

    #[test]
    fn test_transport_default_stdio() {
        let cli = Cli::parse(&["9router-mcp-web"]).unwrap();
        assert_eq!(cli.transport, Transport::Stdio);
        assert_eq!(cli.port, None);
        assert_eq!(cli.effective_port(), DEFAULT_HTTP_PORT);

        let cli_serve = Cli::parse(&["9router-mcp-web", "serve"]).unwrap();
        assert_eq!(cli_serve.transport, Transport::Stdio);
    }

    #[test]
    fn test_transport_explicit_stdio() {
        let cli = Cli::parse(&["9router-mcp-web", "--transport", "stdio"]).unwrap();
        assert_eq!(cli.transport, Transport::Stdio);

        let cli_serve = Cli::parse(&["9router-mcp-web", "serve", "--transport", "stdio"]).unwrap();
        assert_eq!(cli_serve.transport, Transport::Stdio);

        let cli_eq = Cli::parse(&["9router-mcp-web", "--transport=stdio"]).unwrap();
        assert_eq!(cli_eq.transport, Transport::Stdio);

        let cli_serve_eq = Cli::parse(&["9router-mcp-web", "serve", "--transport=stdio"]).unwrap();
        assert_eq!(cli_serve_eq.transport, Transport::Stdio);
    }

    #[test]
    fn test_transport_explicit_http() {
        let cli = Cli::parse(&["9router-mcp-web", "--transport", "http"]).unwrap();
        assert_eq!(cli.transport, Transport::Http);
        assert_eq!(cli.port, None);
        assert_eq!(cli.effective_port(), 20129);

        let cli_serve = Cli::parse(&["9router-mcp-web", "serve", "--transport", "http"]).unwrap();
        assert_eq!(cli_serve.transport, Transport::Http);

        let cli_eq = Cli::parse(&["9router-mcp-web", "--transport=http"]).unwrap();
        assert_eq!(cli_eq.transport, Transport::Http);

        let cli_serve_eq = Cli::parse(&["9router-mcp-web", "serve", "--transport=http"]).unwrap();
        assert_eq!(cli_serve_eq.transport, Transport::Http);
    }

    #[test]
    fn test_transport_http_with_port() {
        let cli =
            Cli::parse(&["9router-mcp-web", "--transport", "http", "--port", "3000"]).unwrap();
        assert_eq!(cli.transport, Transport::Http);
        assert_eq!(cli.port, Some(3000));
        assert_eq!(cli.effective_port(), 3000);

        let cli_reverse =
            Cli::parse(&["9router-mcp-web", "--port", "3000", "--transport", "http"]).unwrap();
        assert_eq!(cli_reverse.transport, Transport::Http);
        assert_eq!(cli_reverse.port, Some(3000));

        let cli_eq = Cli::parse(&["9router-mcp-web", "--transport=http", "--port=3000"]).unwrap();
        assert_eq!(cli_eq.transport, Transport::Http);
        assert_eq!(cli_eq.port, Some(3000));

        let cli_serve = Cli::parse(&[
            "9router-mcp-web",
            "serve",
            "--transport",
            "http",
            "--port",
            "8080",
        ])
        .unwrap();
        assert_eq!(cli_serve.transport, Transport::Http);
        assert_eq!(cli_serve.port, Some(8080));
    }

    #[test]
    fn test_port_without_http_rejected() {
        // Default transport is stdio, so --port alone must fail
        let err1 = Cli::parse(&["9router-mcp-web", "--port", "3000"]).unwrap_err();
        assert!(
            err1.to_string()
                .contains("only allowed with '--transport http'"),
            "Error was: {}",
            err1
        );

        let err2 = Cli::parse(&["9router-mcp-web", "--port=3000"]).unwrap_err();
        assert!(
            err2.to_string()
                .contains("only allowed with '--transport http'"),
            "Error was: {}",
            err2
        );

        // Explicit stdio with --port must fail
        let err3 =
            Cli::parse(&["9router-mcp-web", "--transport", "stdio", "--port", "3000"]).unwrap_err();
        assert!(
            err3.to_string()
                .contains("only allowed with '--transport http'"),
            "Error was: {}",
            err3
        );

        let err4 = Cli::parse(&[
            "9router-mcp-web",
            "serve",
            "--transport=stdio",
            "--port=3000",
        ])
        .unwrap_err();
        assert!(
            err4.to_string()
                .contains("only allowed with '--transport http'"),
            "Error was: {}",
            err4
        );
    }

    #[test]
    fn test_invalid_transport_fails() {
        assert!(Cli::parse(&["9router-mcp-web", "--transport", "invalid"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--transport", "sse"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--transport", "https"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--transport", "auto"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--transport="]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--transport"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--transport", "--help"]).is_err());
    }

    #[test]
    fn test_invalid_port_fails() {
        // Non-numeric
        assert!(Cli::parse(&["9router-mcp-web", "--transport", "http", "--port", "abc"]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--transport", "http", "--port="]).is_err());
        assert!(Cli::parse(&["9router-mcp-web", "--transport", "http", "--port"]).is_err());

        // Port 0 is rejected in CLI
        let err_zero =
            Cli::parse(&["9router-mcp-web", "--transport", "http", "--port", "0"]).unwrap_err();
        assert!(
            err_zero.to_string().contains("port 0 is not allowed"),
            "Error was: {}",
            err_zero
        );

        // Out of range (TCP port is 1-65535)
        let err_high =
            Cli::parse(&["9router-mcp-web", "--transport", "http", "--port", "65536"]).unwrap_err();
        assert!(
            err_high.to_string().contains("port out of range"),
            "Error was: {}",
            err_high
        );
    }

    #[test]
    fn test_transport_and_port_rejected_with_configure() {
        let err1 =
            Cli::parse(&["9router-mcp-web", "configure", "--transport", "http"]).unwrap_err();
        assert!(
            err1.to_string()
                .contains("cannot be used with the 'configure' command"),
            "Error: {}",
            err1
        );

        let err2 = Cli::parse(&["9router-mcp-web", "configure", "--port", "3000"]).unwrap_err();
        assert!(
            err2.to_string()
                .contains("cannot be used with the 'configure' command"),
            "Error: {}",
            err2
        );

        let err3 =
            Cli::parse(&["9router-mcp-web", "--transport", "http", "configure"]).unwrap_err();
        assert!(
            err3.to_string()
                .contains("cannot be used with the 'configure' command"),
            "Error: {}",
            err3
        );
    }

    #[test]
    fn test_duplicate_transport_and_port_fail() {
        assert!(Cli::parse(&[
            "9router-mcp-web",
            "--transport",
            "http",
            "--transport",
            "stdio"
        ])
        .is_err());
        assert!(Cli::parse(&[
            "9router-mcp-web",
            "--transport",
            "http",
            "--port",
            "3000",
            "--port",
            "4000"
        ])
        .is_err());
    }
}
