# **Task 001: Project Setup and Configuration**

## **1. Objective**

The goal of this task is to set up the basic project structure and define the core data types for handling configuration. We will ensure that our application can correctly parse and validate a `pgtls.toml` file.

## **2. Implementation Steps**

### **2.1. Create `src/config.rs`**


Create a new module `src/config.rs` to house all configuration-related structures.

### **2.2. Define Data Structures**

In `src/config.rs`, define the Rust structs that map to the TOML configuration specified in `specs/004-configuration.md`. Use `serde` for deserialization.

```rust
// src/config.rs
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(rename = "proxy")]
    pub proxies: Vec<Proxy>,
}

#[derive(Debug, Deserialize)]
pub struct Proxy {
    pub listener: Listener,
    pub backend: Backend,
}

#[derive(Debug, Deserialize)]
pub struct Listener {
    pub bind_address: String,
    pub server_cert: String,
    pub server_key: String,
    #[serde(default)]
    pub mtls: bool,
    pub client_ca: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Backend {
    pub address: String,
    #[serde(default = "default_tls_enabled")]
    pub tls_enabled: bool,
    pub root_ca: Option<String>,
    pub client_cert: Option<String>,
    pub client_key: Option<String>,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_tls_enabled() -> bool {
    true
}
```

### **2.3. Add a `load` function**

Create a function `Config::load(path: &str) -> anyhow::Result<Self>` that reads the file at the given path, parses the TOML, and returns a `Config` instance. This function should also perform the validation logic specified in the configuration spec.

### **2.4. Update `src/lib.rs`**

Modify `src/lib.rs` to include the new `config` module.

```rust
// src/lib.rs
pub mod config;
```

## **3. Testing Strategy (TDD)**

Our testing will focus on ensuring the configuration is parsed and validated correctly.

### **3.1. Create Test Cases**

In `src/config.rs`, inside a `#[cfg(test)]` module, create several test cases:

1.  **`test_load_full_config`**:
    *   Create a temporary TOML file with a complete and valid configuration, including two `[[proxy]]` entries (one with backend TLS, one without, one with mTLS).
    *   Call `Config::load()` with the path to this file.
    *   Assert that the resulting `Config` struct has the correct values.

2.  **`test_load_minimal_config`**:
    *   Create a temporary TOML file with the minimum required fields for a single proxy route.
    *   Assert that the parsing is successful and that default values for `log_level`, `mtls`, and `tls_enabled` are correctly applied.

3.  **`test_validation_errors`**:
    *   Create several tests that check for validation failures:
        *   A proxy with `listener.mtls = true` but no `listener.client_ca`.
        *   A proxy with `backend.tls_enabled = true` but no `backend.root_ca`.
        *   A proxy with `backend.client_cert` but no `backend.client_key`.
    *   Assert that `Config::load()` returns an `Err` for each of these cases.

4.  **`test_file_not_found`**:
    *   Call `Config::load()` with a non-existent file path.
    *   Assert that it returns an `Err`.

This TDD approach ensures that our core configuration logic is robust and reliable before we build any networking code on top of it.
