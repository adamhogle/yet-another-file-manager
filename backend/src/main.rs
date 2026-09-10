use std::env;
use std::path::{Path, PathBuf};
use std::process;

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

fn resolve_config_path(args: impl Iterator<Item = String>, cwd: &Path) -> Result<PathBuf, String> {
    let values: Vec<String> = args.collect();
    if values.len() > 1 {
        return Err("Usage: backend [config-path]".to_string());
    }

    if let Some(path) = values.first() {
        return Ok(PathBuf::from(path));
    }

    for file_name in ["config.yaml", "config.json"] {
        let candidate = cwd.join(file_name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err(
        "Configuration path argument is required unless ./config.yaml or ./config.json exists in the current working directory."
            .to_string(),
    )
}

#[tokio::main]
async fn main() {
    if let Err(message) = run().await {
        eprintln!("{message}");
        process::exit(1);
    }
}

fn init_tracing() {
    // Defaults to info-level app logs with tower-http's own request logging visible;
    // RUST_LOG replaces the whole directive set when set.
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=debug"));
    tracing_subscriber::fmt()
        .compact()
        .with_env_filter(env_filter)
        .init();
}

async fn run() -> Result<(), String> {
    init_tracing();

    let cwd =
        env::current_dir().map_err(|_| "Could not determine current directory".to_string())?;
    let config_path = resolve_config_path(env::args().skip(1), &cwd)?;
    backend::initialize_app_config(&config_path)?;

    // In release builds the test-only env var is ignored; warn so a stray flag
    // is visible instead of silently dropped.
    let disable_auth_env = env::var("YAFM_DISABLE_AUTH").ok();
    if !cfg!(debug_assertions) && disable_auth_env.is_some() {
        tracing::warn!("YAFM_DISABLE_AUTH is a test-only flag and is ignored in release builds.");
    }

    // The only way to disable auth: the debug-gated test backdoor. The env var
    // is honored only by development builds (cfg!(debug_assertions)) and skips
    // the oidc requirement entirely; the release image always enforces auth.
    // The check runs BEFORE initialize_auth so the flag is honored before the
    // oidc requirement is evaluated.
    // YAFM_DISABLE_AUTH is release-ignored (cfg!(debug_assertions) is build-time):
    // release builds always run initialize_auth(), which fails closed without
    // an `oidc` block — pinned by `initialize_auth_requires_the_oidc_block` in
    // src/auth.rs and verified manually against a --release binary.
    if cfg!(debug_assertions) && disable_auth_env.as_deref() == Some("1") {
        backend::auth::initialize_auth_disabled_for_tests();
    } else {
        backend::auth::initialize_auth()?;
    }

    let binding = backend::get_server_binding()?;
    let listen_address = format!("{}:{}", binding.address, binding.port);

    let listener = TcpListener::bind(&listen_address)
        .await
        .map_err(|_| format!("failed to bind backend listener on {listen_address}"))?;

    // Report the actual bound address so listenPort: 0 resolves to the assigned port.
    let bound_address = listener
        .local_addr()
        .map_err(|_| "could not determine the bound address of the backend listener".to_string())?;

    tracing::info!("backend listening on http://{bound_address}");

    axum::serve(listener, backend::app_router())
        .await
        .map_err(|_| "backend server crashed".to_string())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::resolve_config_path;

    #[test]
    fn resolves_explicit_config_path_from_cli_argument() {
        let cwd = TempDir::new().expect("temp dir");
        let resolved = resolve_config_path(
            ["../config/yafm.config.yaml".to_string()].into_iter(),
            cwd.path(),
        )
        .expect("resolve explicit path");

        assert_eq!(resolved, PathBuf::from("../config/yafm.config.yaml"));
    }

    #[test]
    fn falls_back_to_config_yaml_in_cwd() {
        let cwd = TempDir::new().expect("temp dir");
        let config_path = cwd.path().join("config.yaml");
        fs::write(&config_path, "sharedRoot: /tmp\n").expect("write config");

        let resolved =
            resolve_config_path(std::iter::empty(), cwd.path()).expect("resolve default config");

        assert_eq!(resolved, config_path);
    }

    #[test]
    fn requires_config_argument_when_no_cwd_defaults_exist() {
        let cwd = TempDir::new().expect("temp dir");
        let error = resolve_config_path(std::iter::empty(), cwd.path())
            .expect_err("missing config should fail");

        assert!(error.contains("Configuration path argument is required"));
    }
}
