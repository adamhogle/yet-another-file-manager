//! The server binding accessor through the process-global config: an
//! uninitialized application config fails closed, and an initialized one
//! reports the configured address and port. Runs in its own test binary as
//! one test so no other test has initialized the config first (the config
//! OnceCell is first-call-wins and cannot be reset).

use std::fs;
use tempfile::TempDir;

#[tokio::test]
async fn the_server_binding_reports_the_configured_address_and_port() {
    // One test per binary: no other test has initialized the config, so the
    // accessor fails closed with the not-initialized message.
    let error = backend::get_server_binding().expect_err("an uninitialized config must fail");
    assert_eq!(error, "Application configuration was not initialized");
    // Without an initialized config there is no access state either.
    assert!(backend::get_access_state_config().is_none());

    let temp = TempDir::new().expect("temp dir");
    let shared_root = temp.path().join("share");
    fs::create_dir_all(&shared_root).expect("create shared root");
    let config_path = temp.path().join("yafm.config.yaml");
    fs::write(
        &config_path,
        format!(
            "sharedRoot: {}\nlistenAddress: 127.0.0.1\nlistenPort: 9999\n",
            shared_root.display()
        ),
    )
    .expect("write config");

    backend::initialize_app_config(&config_path).expect("initialize app config");

    let binding = backend::get_server_binding().expect("the initialized binding");
    assert_eq!(binding.address, "127.0.0.1");
    assert_eq!(binding.port, 9999);
    // The canonical shared root is readable for the access-check mode and
    // the check walk.
    assert_eq!(
        backend::get_shared_root().expect("the shared root"),
        shared_root
            .canonicalize()
            .expect("canonical root")
            .as_path()
    );
    // This config has no `access` block: the accessor reports None, the
    // missing-block startup validation lives in the access initializer.
    assert!(backend::get_access_state_config().is_none());
}
