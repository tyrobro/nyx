use clap::{Parser, Subcommand};
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use std::io::{self, Write};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
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

            tokio::spawn(async move {
                if let Ok((socket, peer_addr)) = p2p_listener.accept().await {
                    println!(
                        "\n\n>>> DIRECT P2P CONNECTION ESTABLISHED FROM {} <<<",
                        peer_addr
                    );
                    handle_p2p_chat(socket).await;
                }
            });

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
                    eprintln!("Running in offline mode. Peers will not be able to find you.");
                }
            }

            tokio::signal::ctrl_c().await.unwrap();
            println!("Shutting down node.");
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

async fn handle_p2p_chat(mut stream: tokio::net::TcpStream) {
    println!("Peforming secure cryptographic handshake...");

    let _shared_secret = match crate::crypto::perform_handshake(&mut stream).await {
        Ok(secret) => secret,
        Err(e) => {
            eprintln!("Cryptographic handshake failed: {}", e);
            return;
        }
    };

    println!("Handshake successful! End-to-end secure tunnel established.");
    println!("(Notice: Messages are not yet encrypted. We will use the secret in the next step!)");

    let (reader, mut writer) = tokio::io::split(stream);

    let mut network_reader = BufReader::new(reader);
    let mut stdin_reader = BufReader::new(tokio::io::stdin());

    let mut network_line = String::new();
    let mut stdin_line = String::new();

    println!("Press Enter to send your message. Type 'nyx exit' to quit.");
    print!("> ");
    io::stdout().flush().unwrap();

    loop {
        tokio::select! {
            result = network_reader.read_line(&mut network_line) => {
                if result.unwrap_or(0) == 0 {
                    println!("\n Peer Disconnected.");
                    break;
                }

                print!("\r[Peer]: {}", network_line);
                print!("> ");
                io::stdout().flush().unwrap();
                network_line.clear();
            }

            result = stdin_reader.read_line(&mut stdin_line) =>{
                if result.unwrap_or(0) == 0 {
                    break;
                }

                let command = stdin_line.trim();

                if command == "nyx exit" {
                    println!("Session ended.");
                    break;
                } else if !command.is_empty(){
                    if writer.write_all(stdin_line.as_bytes()).await.is_err() {
                        println!("\nFailed to send message. Peer may have disconnected.");
                        break;
                    }
                }

                print!("> ");
                io::stdout().flush().unwrap();
                stdin_line.clear();
            }
        }
    }
}
