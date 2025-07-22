use std::net::Ipv4Addr;

use tokio::net::{TcpListener, TcpStream};
use tracing::Instrument;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive("INFO".parse().unwrap())
                .from_env_lossy(),
        )
        .init();

    let listener = TcpListener::bind(("127.0.0.1".parse::<Ipv4Addr>().unwrap(), 8080)).await?;

    loop {
        let (conn, remote_addr) = listener.accept().await?;

        tokio::spawn(process_conn(conn).instrument(tracing::info_span!(
            "connection thread",
            remote_addr = format!("{remote_addr:?}"),
        )));
    }
}

async fn process_conn(conn: TcpStream) {
    if let Err(err) = process_conn_inner(conn).await {
        tracing::error!("{err:?}");
    }
}

async fn process_conn_inner(conn: TcpStream) -> anyhow::Result<()> {
    tracing::info!("Socket connected!");

    Ok(())
}
