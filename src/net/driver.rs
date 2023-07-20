use tokio::net::{TcpListener, TcpStream};

use super::transport::{RawPeerStream, HARD_MAX_PACKET_LEN_INCL};

pub async fn run_server() -> anyhow::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:8080").await?;

    log::info!("Server is listening.");

    loop {
        let (peer_stream, remote_ip) = listener.accept().await?;
        log::info!("Got connection from {remote_ip:?}");

        tokio::spawn(async move {
            match run_peer_listener(peer_stream).await {
                Ok(()) => {
                    log::info!("Lost connection to {remote_ip:?}")
                }
                Err(err) => {
                    log::error!("Error occurred while communicating with {remote_ip:?}: {err}");
                }
            }
        });
    }
}

async fn run_peer_listener(peer_stream: TcpStream) -> anyhow::Result<()> {
    let mut peer_stream = RawPeerStream::new(peer_stream, HARD_MAX_PACKET_LEN_INCL);

    while let Some(packet) = peer_stream.read().await {
        let packet = packet?;

        log::info!("Received packet: {packet:#?}");
    }

    Ok(())
}
