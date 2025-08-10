use anyhow::Result;
use clap::Parser;
use std::process;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

mod cert_manager;
mod config;
mod protocol;
mod proxy;

use config::Config;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the configuration file
    config: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = Config::load(&args.config)?;

    // Setup logging
    let filter = EnvFilter::try_new(&config.log_level).unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(fmt::layer().json()) // Use JSON formatting
        .with(filter)
        .init();

    if config.proxies.is_empty() {
        tracing::error!("No proxy configurations found in {}", args.config);
        process::exit(1);
    }

    tracing::info!(
        "Starting pgtls proxy with {} route(s)",
        config.proxies.len()
    );

    // Start all proxy tasks
    let mut tasks = Vec::new();

    for proxy_config in config.proxies {
        tracing::info!(
            "Starting proxy for listener: {}",
            proxy_config.listener.bind_address
        );
        let task = tokio::spawn(proxy::run_proxy(proxy_config));
        tasks.push(task);
    }

    // Wait for shutdown signal
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received Ctrl+C, shutting down.");
        }
        // On Unix, we can also listen for SIGTERM
        result = setup_sigterm_handler() => {
            if let Err(e) = result {
                tracing::error!("Error setting up signal handler: {}", e);
            }
        }
        // If any proxy task completes (likely due to error), shut down
        result = futures::future::select_all(tasks.iter_mut()) => {
            match result.0 {
                Ok(_) => tracing::info!("Proxy task completed, shutting down."),
                Err(e) => tracing::error!("Proxy task failed: {}, shutting down.", e),
            }
        }
    }

    tracing::info!("Shutdown complete.");
    Ok(())
}

#[cfg(unix)]
async fn setup_sigterm_handler() -> Result<(), std::io::Error> {
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    sigterm.recv().await;
    tracing::info!("Received SIGTERM, shutting down.");
    Ok(())
}

#[cfg(not(unix))]
async fn setup_sigterm_handler() -> Result<(), std::io::Error> {
    futures::future::pending::<Result<(), std::io::Error>>().await
}
