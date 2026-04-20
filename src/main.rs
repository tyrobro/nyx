use clap::{Parser, Subcommand};
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use std::io::Write;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
mod crypto;
mod server;

#[derive(Parser)]
#[command(name = "nyx")]
#[command(about = "Anonymous P2P terminal communication", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Start,
    Connect { id: String },
    Host,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Start => {
            let mut csprng = OsRng;
            let signing_key: SigningKey = SigningKey::generate(&mut csprng);
            let verifying_key = signing_key.verifying_key();

            let pub_hex: String = verifying_key
                .as_bytes()
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect();

            let formatted_id = pub_hex
                .as_bytes()
                .chunks(4)
                .map(|chunk| std::str::from_utf8(chunk).unwrap())
                .collect::<Vec<&str>>()
                .join("-");

            println!("Your ID: {}", formatted_id);
            println!("Keep your private key secure and do not share it.\n");

            let p2p_listener = tokio::net::TcpListener::bind("0.0.0.0:0")
                .await
                .expect("Failed to bind a local port for P2P connection");

            let local_p2p_port = p2p_listener.local_addr().unwrap().port();
            println!(
                "(Listening for direct P2P connections on port {})",
                local_p2p_port
            );

            println!("Connecting to network...");
            let mut _keep_alive_stream = None;

            match tokio::net::TcpStream::connect("127.0.0.1:8080").await {
                Ok(mut stream) => {
                    let payload = format!("REGISTER:{}:{}", formatted_id, local_p2p_port);
                    if let Err(e) = stream.write_all(payload.as_bytes()).await {
                        eprintln!("Failed to register with server: {}", e);
                    } else {
                        println!("Successfully registered online.");
                        _keep_alive_stream = Some(stream);
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Could not connect to coordination server ({}).", e);
                }
            }

            println!("Waiting for a peer to connect. Press Ctrl+C to abort.");

            if let Ok((socket, peer_addr)) = p2p_listener.accept().await {
                println!(
                    "\n\n>>> DIRECT P2P CONNECTION ESTABLISHED FROM {} <<<",
                    peer_addr
                );

                handle_p2p_chat(socket).await;
            }
        }

        Commands::Connect { id } => {
            println!("Looking for [{}] on the coordination server...", id);

            match tokio::net::TcpStream::connect("127.0.0.1:8080").await {
                Ok(mut stream) => {
                    let payload = format!("QUERY:{}", id);
                    if let Err(e) = stream.write_all(payload.as_bytes()).await {
                        eprintln!("Failed to query server: {}", e);
                        return;
                    }

                    let mut buffer = [0; 1024];
                    match stream.read(&mut buffer).await {
                        Ok(n) if n > 0 => {
                            let response = String::from_utf8_lossy(&buffer[..n]).trim().to_string();

                            if response.starts_with("FOUND:") {
                                let target_ip =
                                    response.strip_prefix("FOUND:").unwrap().to_string();
                                println!("Success! Peer found at network address: {}", target_ip);

                                println!("Establishing direct P2P connection...");

                                match tokio::net::TcpStream::connect(&target_ip).await {
                                    Ok(p2p_stream) => {
                                        println!(
                                            "\n\n>>> DIRECT P2P CONNECTION ESTABLISHED WITH {} <<<",
                                            id
                                        );

                                        handle_p2p_chat(p2p_stream).await;
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "Failed to establish direct P2P connection: {}",
                                            e
                                        );
                                        eprintln!(
                                            "The peer might have a firewall blocking incoming connections."
                                        );
                                    }
                                }
                            } else if response == "NOT_FOUND" {
                                eprintln!("Error: Peer [{}] not found.", id);
                            } else {
                                eprintln!(
                                    "Error: Received unknown response from server: {}",
                                    response
                                );
                            }
                        }
                        _ => eprintln!("Error: Server closed the connection before answering."),
                    }
                }
                Err(e) => {
                    eprintln!("Fatal: Could not connect to coordination server: {}", e);
                }
            }
        }

        Commands::Host => {
            println!("This node is now a Nyx Local Server");
            use std::collections::HashMap;
            use std::sync::Arc;
            use tokio::sync::Mutex;

            let registry = Arc::new(Mutex::new(HashMap::new()));

            if let Err(e) = crate::server::start_server("0.0.0.0:8080", registry).await {
                eprintln!("Server error: {}", e);
            }
        }
    }
}

use rustyline_async::{Readline, ReadlineEvent};

async fn handle_p2p_chat(mut stream: tokio::net::TcpStream) {
    println!("Performing secure cryptographic handshake...");

    let shared_secret = match crate::crypto::perform_handshake(&mut stream).await {
        Ok(secret) => secret,
        Err(e) => {
            eprintln!("Cryptographic handshake failed: {}", e);
            return;
        }
    };

    println!("Handshake successful! End-to-end secure tunnel established.\n");

    let (mut reader, mut writer) = tokio::io::split(stream);

    let (mut rl, mut stdout) =
        Readline::new("> ".to_owned()).expect("Failed to initialize asynchronous terminal UI");

    writeln!(
        stdout,
        "Type a message and press Enter to send. Type 'nyx exit' to quit."
    )
    .unwrap();
    stdout.flush().unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(100);

    tokio::spawn(async move {
        loop {
            match rl.readline().await {
                Ok(ReadlineEvent::Line(line)) => {
                    rl.add_history_entry(line.clone());
                    if tx.send(line).await.is_err() {
                        break;
                    }
                }
                Ok(ReadlineEvent::Eof) | Ok(ReadlineEvent::Interrupted) => {
                    let _ = tx.send("nyx exit".to_string()).await;
                    break;
                }
                Err(_) => break,
            }
        }
        rl.flush().unwrap();
    });

    loop {
        tokio::select! {
            len_result = reader.read_u32() => {
                let length = match len_result {
                    Ok(l) => l as usize,
                    Err(_) => {
                        writeln!(stdout, "\nPeer disconnected abruptly.").unwrap();
                        stdout.flush().unwrap();
                        break;
                    }
                };

                let mut payload = vec![0u8; length];
                if reader.read_exact(&mut payload).await.is_err() {
                    break;
                }

                match crate::crypto::decrypt_message(&shared_secret, &payload) {
                    Ok(text) => {
                        if text == "/nyx_internal_drop_connection" {
                            writeln!(stdout, "\n[System]: Peer has securely closed the connection.").unwrap();
                            stdout.flush().unwrap();
                            break;
                        }
                        writeln!(stdout, "[Peer]: {}", text).unwrap();
                        stdout.flush().unwrap();
                    }
                    Err(e) => {
                        writeln!(stdout, "\n[SECURITY ALERT] Decryption failed: {}", e).unwrap();
                        stdout.flush().unwrap();
                        break;
                    }
                }
            }

            keyboard_result = rx.recv() => {
                if let Some(line) = keyboard_result {
                    let command = line.trim();

                    if command == "nyx exit" {
                        let payload = crate::crypto::encrypt_message(&shared_secret, "/nyx_internal_drop_connection");
                        let _ = writer.write_u32(payload.len() as u32).await;
                        let _ = writer.write_all(&payload).await;
                        let _ = writer.flush().await;

                        writeln!(stdout, "Session ended. Disconnecting cleanly...").unwrap();
                        stdout.flush().unwrap();

                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        break;
                    } else if !command.is_empty() {
                        let payload = crate::crypto::encrypt_message(&shared_secret, command);
                        if writer.write_u32(payload.len() as u32).await.is_err() ||
                           writer.write_all(&payload).await.is_err() {
                            writeln!(stdout, "\nFailed to send message. Peer disconnected.").unwrap();
                            stdout.flush().unwrap();
                            break;
                        }
                    }
                }
            }
        }
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    std::process::exit(0);
}
