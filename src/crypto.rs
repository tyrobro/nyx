use chacha20poly1305::{
    ChaCha20Poly1305, Key, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
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

pub fn encrypt_message(shared_secret: &[u8; 32], plaintext: &str) -> Vec<u8> {
    let key = Key::from_slice(shared_secret);
    let cipher = ChaCha20Poly1305::new(key);

    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);

    let mut ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .expect("Encryption failure!");

    let mut payload = nonce.to_vec();
    payload.append(&mut ciphertext);

    payload
}

pub fn decrypt_message(shared_secret: &[u8; 32], payload: &[u8]) -> Result<String, &'static str> {
    if payload.len() < 12 {
        return Err("Payload too short");
    }

    let key = Key::from_slice(shared_secret);
    let cipher = ChaCha20Poly1305::new(key);

    let nonce = Nonce::from_slice(&payload[..12]);
    let ciphertext = &payload[12..];

    match cipher.decrypt(nonce, ciphertext) {
        Ok(decrypted_bytes) => match String::from_utf8(decrypted_bytes) {
            Ok(text) => Ok(text),
            Err(_) => Err("Decrypted data is not valid UTF-8"),
        },
        Err(_) => Err("Decryption failed! The data was tampered with or the wrong key was used."),
    }
}
