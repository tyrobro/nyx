use clap::{Parser, Subcommand};
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use std::{
    io::{self, Write},
    net::TcpStream,
};
use tokio::io::AsyncWriteExt;

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
                .join(" ");

            println!("Your ID: {}", formatted_id);
            println!("Keep your private key secure and do not share it.");
            println!("Connecting to network...");

            let mut _keep_alive_stream = None;

            match tokio::net::TcpStream::connect("127.0.0.1:8080").await {
                Ok(mut stream) => {
                    let payload = format!("REGISTER:{}", formatted_id);
                    if let Err(e) = stream.write_all(payload.as_bytes()).await {
                        eprintln!("Failed to register with server: {}", e);
                    } else {
                        println!("Successfully registered online.");
                        _keep_alive_stream = Some(stream);
                    }
                }

                Err(e) => {
                    eprintln!("Warning: Could not connect to coordination server ({}).", e);
                    eprintln!("Running in offline mode. You are not visible to your peers.");
                }
            }
            loop {
                print!("> ");
                io::stdout().flush().unwrap();

                let mut input = String::new();
                if io::stdin().read_line(&mut input).is_err() {
                    println!("Error reading input");
                    break;
                }

                let command = input.trim();

                match command {
                    "nyx exit" => {
                        println!("Session ended.");
                        break;
                    }
                    "" => continue,
                    _ => {
                        println!("(Message not sent - P2P not implemented yet)");
                    }
                }
            }
        }

        Commands::Connect { id } => {
            println!("Connecting to {}...", id);
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
