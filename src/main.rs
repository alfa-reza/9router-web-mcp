use ninerouter_mcp_web::config::{self, parse_config_arg, Config};
use ninerouter_mcp_web::error::{AppError, Result};
use ninerouter_mcp_web::server::NineRouterMcpServer;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    // Configure tracing strictly to stderr so stdout remains reserved exclusively for MCP JSON-RPC
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let config_path_override = parse_config_arg(&args)?;

    if args.len() > 1 && args[1] == "configure" {
        return config::run_interactive_configure(config_path_override.as_deref());
    }

    if args.len() > 1 && (args[1] == "--version" || args[1] == "-V") {
        println!("9router-mcp-web {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args.len() > 1 && (args[1] == "--help" || args[1] == "-h") {
        eprintln!("9router-mcp-web {}", env!("CARGO_PKG_VERSION"));
        eprintln!("Model Context Protocol (MCP) server for 9Router Web Search and Web Fetch.");
        eprintln!();
        eprintln!("Usage:");
        eprintln!("  9router-mcp-web [OPTIONS]            Start MCP STDIO server");
        eprintln!("  9router-mcp-web configure [OPTIONS]  Run interactive configuration wizard");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  -c, --config <PATH>  Path to configuration file");
        eprintln!("  -h, --help           Print help");
        eprintln!("  -V, --version        Print version");
        return Ok(());
    }

    // Resolve configuration (env overrides > file > defaults)
    let config = Config::resolve(config_path_override.as_deref())?;

    if let Some(warning) = config.check_plain_http_warning() {
        tracing::warn!("{}", warning);
    }

    tracing::info!(
        base_url = %config.base_url,
        search_combo = %config.search_combo,
        fetch_combo = %config.fetch_combo,
        api_key = %config.masked_api_key(),
        "Starting 9router-mcp-web STDIO server"
    );

    let (stdin, stdout) = rmcp::transport::io::stdio();
    let server = NineRouterMcpServer::new(&config)?;

    let running = rmcp::service::serve_server(server, (stdin, stdout))
        .await
        .map_err(|e| AppError::Config(format!("Failed to start MCP server: {}", e)))?;

    match running.waiting().await {
        Ok(rmcp::service::QuitReason::Cancelled | rmcp::service::QuitReason::Closed) => {
            tracing::info!("MCP server stopped gracefully");
        }
        Ok(rmcp::service::QuitReason::JoinError(e)) => {
            return Err(AppError::NetworkUnreachable(format!(
                "MCP server runtime task panicked or failed: {}",
                e
            )));
        }
        Ok(_) => {
            tracing::info!("MCP server stopped");
        }
        Err(e) => {
            return Err(AppError::NetworkUnreachable(format!(
                "MCP service task join failed: {}",
                e
            )));
        }
    }

    Ok(())
}
