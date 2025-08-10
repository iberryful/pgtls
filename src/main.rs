use anyhow::Result;
use pgtls::config::Config;
use std::env;
use std::process;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} <config-file>", args[0]);
        process::exit(1);
    }

    let config_path = &args[1];
    let config = Config::load(config_path)?;

    if config.proxies.is_empty() {
        eprintln!("No proxy configurations found in {config_path}");
        process::exit(1);
    }

    // Run all proxies concurrently
    let mut tasks = Vec::new();

    for proxy_config in config.proxies {
        let task = tokio::spawn(async move {
            if let Err(e) = pgtls::proxy::run_proxy(proxy_config).await {
                eprintln!("Proxy error: {e}");
            }
        });
        tasks.push(task);
    }

    // Wait for all proxy tasks to complete
    for task in tasks {
        task.await?;
    }

    Ok(())
}
