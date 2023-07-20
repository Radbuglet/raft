mod driver;
mod net;
mod util;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    driver::main_inner().await
}
