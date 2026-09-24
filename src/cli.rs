use std::path::PathBuf;

use crate::error::{AppError, Result};

pub const HELP_TEXT: &str = concat!(
    "9router-mcp-web ",
    env!("CARGO_PKG_VERSION"),
    "\n",
    "Model Context Protocol (MCP) server for 9Router Web Search and Web Fetch.\n",
    "\n",
    "Usage:\n",
    "  9router-mcp-web [OPTIONS]            Start MCP STDIO server\n",
    "  9router-mcp-web serve [OPTIONS]      Start MCP STDIO server\n",
    "  9router-mcp-web configure [OPTIONS]  Run interactive configuration wizard\n",
    "\n",
    "Options:\n",
    "  -c, --config <PATH>  Path to configuration file\n",
    "  -h, --help           Print help\n",
    "  -V, --version        Print version\n",
);

pub const VERSION_TEXT: &str = concat!("9router-mcp-web ", env!("CARGO_PKG_VERSION"));

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
}

impl Cli {
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

        let final_command = if help_requested {
            CliCommand::Help
        } else if version_requested {
            CliCommand::Version
        } else {
            command.unwrap_or(CliCommand::Serve)
        };

        Ok(Self {
            command: final_command,
            config_path,
        })
    }
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
}
