use clap::{Parser, Subcommand};
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use std::io::{self, Write};

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
}

fn main() {
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
                    "exit" => {
                        println!("Session ended.");
                        break;
                    }
                    "" => continue,
                    _ => {
                        println!("(Messafe not sent - P2P not implemented yet)");
                    }
                }
            }
        }

        Commands::Connect { id } => {
            println!("Connecting to {}...", id);
        }
    }
}
