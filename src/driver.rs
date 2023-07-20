pub async fn main_inner() -> anyhow::Result<()> {
    // Initialize the logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    Ok(())
}
