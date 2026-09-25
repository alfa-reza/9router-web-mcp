use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

pub use crate::cli::DEFAULT_HTTP_PORT;
use crate::error::{AppError, Result};
use crate::server::NineRouterMcpServer;

pub const BIND_HOST: &str = "127.0.0.1";

/// Binds a TCP listener to loopback (127.0.0.1) on the specified port.
/// Returns a descriptive `AppError` on bind failure without panicking.
pub async fn bind_http_listener(port: u16) -> Result<TcpListener> {
    let bind_addr = format!("{}:{}", BIND_HOST, port);
    TcpListener::bind(&bind_addr).await.map_err(|e| {
        AppError::ServerRuntime(format!(
            "failed to bind HTTP listener on {}: {}",
            bind_addr, e
        ))
    })
}

/// Creates the Axum router with official `rmcp` `StreamableHttpService` mounted at `/mcp`.
///
/// HTTP security baseline:
/// - Loopback-only Host validation (localhost, 127.0.0.1, ::1) enforced by `rmcp` default config.
/// - Origin validation enforced with empty allowlist (requests without Origin are allowed;
///   requests with any present Origin are rejected with 403 Forbidden).
/// - Default request body size protection retained.
pub fn create_mcp_router(
    server: NineRouterMcpServer,
    cancellation_token: CancellationToken,
) -> axum::Router {
    let mcp_config = StreamableHttpServerConfig::default()
        .enforce_origin_validation()
        .with_cancellation_token(cancellation_token.child_token());

    let service: StreamableHttpService<NineRouterMcpServer, LocalSessionManager> =
        StreamableHttpService::new(move || Ok(server.clone()), Default::default(), mcp_config);

    axum::Router::new().nest_service("/mcp", service)
}

/// Serves the MCP HTTP service on the given TCP listener until `cancellation_token` is cancelled.
pub async fn serve_with_listener(
    listener: TcpListener,
    server: NineRouterMcpServer,
    cancellation_token: CancellationToken,
) -> Result<()> {
    let local_addr: SocketAddr = listener
        .local_addr()
        .map_err(|e| AppError::ServerRuntime(format!("failed to get listener address: {}", e)))?;

    tracing::info!(
        endpoint = %format!("http://{}/mcp", local_addr),
        "Starting 9router-mcp-web Streamable HTTP server"
    );

    let router = create_mcp_router(server, cancellation_token.clone());

    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            cancellation_token.cancelled_owned().await;
        })
        .await
        .map_err(|e| AppError::ServerRuntime(format!("HTTP server runtime error: {}", e)))?;

    tracing::info!("MCP HTTP server stopped gracefully");
    Ok(())
}

/// High-level function to bind to 127.0.0.1:<port> and serve HTTP with graceful shutdown on OS signals.
pub async fn serve_http(server: NineRouterMcpServer, port: u16) -> Result<()> {
    let listener = bind_http_listener(port).await?;
    let cancellation_token = CancellationToken::new();
    let shutdown_token = cancellation_token.clone();
    tokio::spawn(async move {
        shutdown_signal(shutdown_token).await;
    });
    serve_with_listener(listener, server, cancellation_token).await
}

/// Handles OS termination signals (SIGINT / Ctrl+C, and SIGTERM on Unix) to trigger graceful shutdown.
pub async fn shutdown_signal(token: CancellationToken) {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::warn!("failed to install Ctrl+C signal handler: {}", e);
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let mut terminate_stream =
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(stream) => Some(stream),
            Err(e) => {
                tracing::warn!("failed to install SIGTERM signal handler: {}", e);
                None
            }
        };

    let terminate = async {
        #[cfg(unix)]
        {
            if let Some(stream) = &mut terminate_stream {
                stream.recv().await;
            } else {
                std::future::pending::<()>().await;
            }
        }
        #[cfg(not(unix))]
        std::future::pending::<()>().await;
    };

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C / SIGINT, initiating graceful shutdown");
        }
        _ = terminate => {
            tracing::info!("Received SIGTERM, initiating graceful shutdown");
        }
        _ = token.cancelled() => {}
    }
    token.cancel();
}
