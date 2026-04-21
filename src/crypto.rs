use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Key, Nonce,
};
use hkdf::Hkdf;
use rand::Rng;
use sha2::Sha256;
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use x25519_dalek::{EphemeralSecret, PublicKey};

pub async fn perform_handshake(stream: &mut TcpStream) -> io::Result<[u8; 32]> {
    let role = negotiate_role(stream).await?;

    let my_secret = EphemeralSecret::random_from_rng(OsRng);
    let my_public = PublicKey::from(&my_secret);

    let peer_public_bytes: [u8; 32];

    if role == Role::Initiator {
        stream.write_all(my_public.as_bytes()).await?;
        stream.flush().await?;

        let mut buf = [0u8; 32];
        stream.read_exact(&mut buf).await?;
        peer_public_bytes = buf;
    } else {
        let mut buf = [0u8; 32];
        stream.read_exact(&mut buf).await?;
        peer_public_bytes = buf;

        stream.write_all(my_public.as_bytes()).await?;
        stream.flush().await?;
    }

    let peer_public = PublicKey::from(peer_public_bytes);

    let dh_output = my_secret.diffie_hellman(&peer_public);
    let symmetric_key = hkdf_derive(dh_output.as_bytes());

    Ok(symmetric_key)
}

pub fn encrypt_message(shared_secret: &[u8; 32], plaintext: &str) -> Vec<u8> {
    let key = Key::from_slice(shared_secret);
    let cipher = ChaCha20Poly1305::new(key);

    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);

    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .expect("Encryption failure — this should never happen with a valid key");

    let mut payload = nonce.to_vec();
    payload.extend_from_slice(&ciphertext);
    payload
}

pub fn decrypt_message(shared_secret: &[u8; 32], payload: &[u8]) -> Result<String, String> {
    if payload.len() < 28 {
        return Err(format!(
            "Payload too short ({} bytes) — possible truncation or tampering",
            payload.len()
        ));
    }

    let key = Key::from_slice(shared_secret);
    let cipher = ChaCha20Poly1305::new(key);

    let nonce = Nonce::from_slice(&payload[..12]);
    let ciphertext = &payload[12..];

    let decrypted_bytes = cipher.decrypt(nonce, ciphertext).map_err(|_| {
        "AEAD authentication failed — message was tampered with or key mismatch".to_string()
    })?;

    String::from_utf8(decrypted_bytes)
        .map_err(|_| "Decrypted payload is not valid UTF-8 — possible protocol error".to_string())
}

#[derive(PartialEq)]
enum Role {
    Initiator,
    Responder,
}

async fn negotiate_role(stream: &mut TcpStream) -> io::Result<Role> {
    loop {
        let my_byte: u8 = rand::thread_rng().gen();
        stream.write_all(&[my_byte]).await?;
        stream.flush().await?;

        let mut peer_byte = [0u8; 1];
        stream.read_exact(&mut peer_byte).await?;
        let peer_byte = peer_byte[0];

        match my_byte.cmp(&peer_byte) {
            std::cmp::Ordering::Greater => return Ok(Role::Initiator),
            std::cmp::Ordering::Less => return Ok(Role::Responder),
            std::cmp::Ordering::Equal => {
                continue;
            }
        }
    }
}

fn hkdf_derive(dh_bytes: &[u8]) -> [u8; 32] {
    let hkdf = Hkdf::<Sha256>::new(None, dh_bytes);
    let mut okm = [0u8; 32];
    hkdf.expand(b"nyx-p2p-chat-v1", &mut okm)
        .expect("HKDF expand failed — output length is valid so this cannot happen");
    okm
}
