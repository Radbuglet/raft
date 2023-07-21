use tokio::net::{TcpListener, TcpStream};

use crate::{
    net::{primitives::Codec, protocol::handshake::SbHandshake},
    util::codec::ByteReadCursor,
};

use super::{limits::HARD_MAX_PACKET_LEN_INCL, transport::RawPeerStream};

pub async fn run_server() -> anyhow::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:8080").await?;

    log::info!("Server is listening.");

    loop {
        let (peer_stream, remote_ip) = listener.accept().await?;
        log::info!("Got connection from {remote_ip:?}");

        tokio::spawn(async move {
            match run_peer_listener(peer_stream).await {
                Ok(true) => {
                    log::info!("Closed connection to {remote_ip:?}")
                }
                Ok(false) => {
                    log::info!("Lost connection to {remote_ip:?}")
                }
                Err(err) => {
                    log::error!("Error occurred while communicating with {remote_ip:?}: {err}");
                }
            }
        });
    }
}

async fn run_peer_listener(peer_stream: TcpStream) -> anyhow::Result<bool> {
    let mut peer_stream = RawPeerStream::new(peer_stream, HARD_MAX_PACKET_LEN_INCL);

    while let Some(packet) = peer_stream.read().await {
        let packet = packet?;
        log::info!("Received packet: {packet:#?}");

        let packet = SbHandshake::decode(&packet.body, &mut ByteReadCursor::new(&packet.body))?;
        log::info!("Received handshake packet: {packet:#?}");

        return Ok(true);
    }

    Ok(false)
}
