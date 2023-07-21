use tokio::net::{TcpListener, TcpStream};

use crate::net::protocol::{sb_handshake, sb_status};

use super::{limits::HARD_MAX_PACKET_LEN_INCL, protocol::PeerState, transport::RawPeerStream};

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
    let mut state = PeerState::Handshake;

    while let Some(packet) = peer_stream.read().await {
        let packet = packet?;
        log::info!("Received packet: {packet:#?}");

        match state {
            PeerState::Handshake => {
                let packet = sb_handshake::Packet::decode(&packet)?;

                match packet {
                    sb_handshake::Handshake(packet) => {
                        log::info!("Received handshake packet: {packet:#?}");

                        match packet.next_state.0 {
                            1 => state = PeerState::Status,
                            2 => state = PeerState::Login,
                            _ => anyhow::bail!("Invalid handshake target state."),
                        }
                    }
                }
            }
            PeerState::Status => {
                let packet = sb_status::Packet::decode(&packet)?;

                match packet {
                    sb_status::StatusRequest(packet) => {
                        log::info!("Received status request: {packet:#?}");
                        return Ok(true);
                    }
                    sb_status::PingRequest(packet) => {
                        log::info!("Received ping request: {packet:#?}");
                        return Ok(true);
                    }
                }
            }
            PeerState::Login => todo!(),
            PeerState::Play => todo!(),
        }
    }

    Ok(false)
}
