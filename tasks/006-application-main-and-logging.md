# **Task 006: Application Assembly and Logging**

## **1. Objective**

This final task involves assembling all the previously built components into a complete, runnable application. We will implement the `main` function, set up the command-line interface, and integrate the structured logging specified in the design phase.

## **2. Implementation Steps**

### **2.1. Create `src/main.rs`**

If it doesn't exist, create the `src/main.rs` file. This will be the entry point for the `pgtls` binary.

### **2.2. Implement CLI Parsing**

Using the `clap` crate, define the command-line arguments as specified in `specs/006-cli.md`. The primary goal is to get the path to the configuration file.

```rust
// src/main.rs
use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the configuration file
    #[arg(short, long)]
    config: String,
}

fn main() {
    let args = Args::parse();
    // ...
}
```

### **2.3. Set Up Logging**

Integrate the `tracing` and `tracing-subscriber` crates. The log level should be determined by the `log_level` setting in the loaded configuration file.

```rust
// src/main.rs
// ...
use pgtls::config::Config;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = Config::load(&args.config)?;

    // Setup logging
    let filter = EnvFilter::try_new(&config.log_level)
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(fmt::layer().json()) // Use JSON formatting
        .with(filter)
        .init();

    // ... rest of main
    Ok(())
}
```

### **2.4. Implement the Main Application Loop**

The `main` function will be responsible for:
1.  Parsing CLI arguments.
2.  Loading and validating the configuration file.
3.  Setting up the logger.
4.  Iterating through the `[[proxy]]` routes defined in the config.
5.  For each route, spawning an asynchronous task that runs the `proxy::run_proxy` function.
6.  Implementing a graceful shutdown mechanism. The application should listen for `SIGINT` (Ctrl+C) and `SIGTERM` signals and exit cleanly.

```rust
// src/main.rs
// ...

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ... args, config, logging setup ...

    let mut tasks = vec![];

    for proxy_config in config.proxies {
        tracing::info!(
            "Starting proxy for listener: {}",
            proxy_config.listener.bind_address
        );
        let task = tokio::spawn(pgtls::proxy::run_proxy(proxy_config));
        tasks.push(task);
    }

    // Wait for a shutdown signal
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received Ctrl+C, shutting down.");
        }
        // On Unix, we can also listen for SIGTERM
        #[cfg(unix)]
        _ = async {
            let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
            sigterm.recv().await;
            tracing::info!("Received SIGTERM, shutting down.");
            Ok::<(), std::io::Error>(())
        } => {}
    }

    // Here you would add logic to gracefully shut down the tasks if needed,
    // for now, we just exit.

    Ok(())
}
```

### **2.5. Implement Real Certificate Loading**

Replace the stubbed certificate loading functions (`create_stub_server_config`, `create_stub_client_config`) in `src/proxy.rs` with real implementations that load certificates and keys from the paths specified in the `config::Proxy` struct. This will involve using `rustls` helpers to parse PEM files and create the `ServerConfig` and `ClientConfig` objects.

## **3. Testing Strategy**

Most of the logic here is tested via the integration tests in Task 005. However, we can add a few specific tests.

1.  **Manual Testing**:
    *   Run the binary with a valid config file and observe the log output.
    *   Check that it binds to the correct ports.
    *   Send a `SIGINT` (Ctrl+C) and `SIGTERM` and assert that the application exits gracefully with the correct log message.
    *   Run the binary with an invalid config path and assert that it exits with a clear error message.
2.  **Integration Test for Logging**:
    *   The integration tests from Task 005 can be augmented to capture the stdout/stderr of the `pgtls` child process.
    *   The test can then assert that the captured output contains valid JSON log lines and that the log messages correspond to the actions being performed (e.g., a log entry for a new connection).
