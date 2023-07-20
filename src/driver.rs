use crate::net::driver::run_server;

pub async fn main_inner() -> anyhow::Result<()> {
    // Initialize the logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).init();

    // Run the server
    run_server().await
}
