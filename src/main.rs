use ninerouter_mcp_web::cli::{self, Cli, CliCommand, Transport};
use ninerouter_mcp_web::config::{self, Config};
use ninerouter_mcp_web::error::{AppError, Result};
use ninerouter_mcp_web::http;
use ninerouter_mcp_web::server::NineRouterMcpServer;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("Error: {}", err);
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let cli = Cli::parse(&args)?;

    match cli.command {
        CliCommand::Help => {
            print!("{}", cli::HELP_TEXT);
            Ok(())
        }
        CliCommand::Version => {
            println!("{}", cli::VERSION_TEXT);
            Ok(())
        }
        CliCommand::Configure => {
            config::run_interactive_configure(cli.config_path.as_deref()).await
        }
        CliCommand::Serve => {
            // Configure tracing strictly to stderr so stdout remains reserved exclusively for MCP JSON-RPC
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
                )
                .init();

            // Resolve configuration (env overrides > file > local discovery)
            let config = Config::resolve(cli.config_path.as_deref()).await?;

            if let Some(warning) = config.check_plain_http_warning() {
                tracing::warn!("{}", warning);
            }

            let server = NineRouterMcpServer::new(&config)?;

            match cli.transport {
                Transport::Stdio => {
                    tracing::info!(
                        base_url = %config.base_url,
                        search_combo = %config.search_combo,
                        fetch_combo = %config.fetch_combo,
                        api_key = %config.masked_api_key(),
                        "Starting 9router-mcp-web STDIO server"
                    );

                    let (stdin, stdout) = rmcp::transport::io::stdio();
                    let running = rmcp::service::serve_server(server, (stdin, stdout))
                        .await
                        .map_err(|e| {
                            AppError::ServerRuntime(format!("Failed to start MCP server: {}", e))
                        })?;

                    match running.waiting().await {
                        Ok(
                            rmcp::service::QuitReason::Cancelled
                            | rmcp::service::QuitReason::Closed,
                        ) => {
                            tracing::info!("MCP server stopped gracefully");
                        }
                        Ok(rmcp::service::QuitReason::JoinError(e)) => {
                            return Err(AppError::ServerRuntime(format!(
                                "MCP server runtime task panicked or failed: {}",
                                e
                            )));
                        }
                        Ok(_) => {
                            tracing::info!("MCP server stopped");
                        }
                        Err(e) => {
                            return Err(AppError::ServerRuntime(format!(
                                "MCP service task join failed: {}",
                                e
                            )));
                        }
                    }

                    Ok(())
                }
                Transport::Http => {
                    let port = cli.effective_port();
                    http::serve_http(server, port).await
                }
            }
        }
    }
}
