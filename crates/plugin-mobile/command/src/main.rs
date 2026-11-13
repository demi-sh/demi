use std::sync::Arc;

#[tokio::main]
async fn main() {
    // A panic leaves its report on standard error
    // (`crates/xtask/src/native/build.rs`).
    jush_shared_cli::install_panic_hook();
    jush_command_sdk::Launch::from_process();
    // Events go to standard error as lines of the log, which the runner keeps
    // as they are.
    jush_command_sdk::log_to_stderr("mobile");
    let resident = Arc::new(jush_mobile::JushMobile::new());
    if let Err(error) = jush_command_sdk::serve_stdio(resident).await {
        tracing::error!(event = "mobile_resident.exited", %error, "the resident ended with an error");
        std::process::exit(1);
    }
    std::process::exit(0);
}
