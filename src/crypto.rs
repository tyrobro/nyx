use rand::rngs::OsRng;
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use x25519_dalek::{EphemeralSecret, PublicKey};

pub async fn perform_handshake(stream: &mut TcpStream) -> io::Result<[u8; 32]> {
    let my_secret = EphemeralSecret::random_from_rng(OsRng);
    let my_public = PublicKey::from(&my_secret);

    stream.write_all(my_public.as_bytes()).await?;

    let mut peer_public_bytes = [0u8; 32];
    stream.read_exact(&mut peer_public_bytes).await?;
    let peer_public = PublicKey::from(peer_public_bytes);

    let shared_secret = my_secret.diffie_hellman(&peer_public);

    Ok(shared_secret.to_bytes())
}
