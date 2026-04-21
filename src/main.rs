use clap::{Parser, Subcommand};
use futures::StreamExt;
use libp2p::{
    identity,
    kad::{self, Quorum, Record, RecordKey},
    mdns,
    noise,
    // NetworkBehaviour here is the *trait*; the derive macro of the same name
    // is unlocked by adding "macros" to libp2p's features in Cargo.toml.
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp,
    yamux,
    PeerId,
    SwarmBuilder,
};
use std::io::Write;
use std::net::UdpSocket;
use std::str::FromStr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
mod crypto;

#[derive(NetworkBehaviour)]
struct NyxBehaviour {
    kademlia: kad::Behaviour<kad::store::MemoryStore>,
    mdns: mdns::tokio::Behaviour,
}

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

/// Returns the machine's best-guess outbound LAN IP address.
///
/// Technique: open a UDP socket connected to a public address (no packets
/// are actually sent) and read back which local interface the OS selected.
/// Falls back to "127.0.0.1" if the probe fails (e.g. no network).
fn get_local_ip() -> String {
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?; // no packets sent — just routing table probe
            s.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

/// Shared bootstrap configuration — used by both Start and Connect commands
/// so that both nodes join the same global DHT.
const BOOTSTRAP_ADDR: &str =
    "/dnsaddr/bootstrap.libp2p.io/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN";
const BOOTSTRAP_ID: &str = "QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN";

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match &cli.command {
        // ------------------------------------------------------------------ //
        // START — become the listener / receiver                              //
        // ------------------------------------------------------------------ //
        Commands::Start => {
            let local_key = identity::Keypair::generate_ed25519();
            let local_peer_id = PeerId::from(local_key.public());

            // Bind the raw TCP chat listener on a random OS-assigned port
            let p2p_listener = tokio::net::TcpListener::bind("0.0.0.0:0")
                .await
                .expect("Failed to bind chat listener");
            let chat_port = p2p_listener.local_addr().unwrap().port();

            // FIX: Detect our actual LAN IP instead of hard-coding 127.0.0.1.
            // Without this fix, remote peers (and even mDNS peers on a
            // different subnet) can never reach us because 127.0.0.1 is
            // loopback-only.
            let local_ip = get_local_ip();

            println!("Nyx Decentralized Node Initialized.");
            println!("Your Global Peer ID: {}", local_peer_id);
            println!("Detected local IP  : {}", local_ip);
            println!("Booting libp2p Swarm Engine with mDNS & Kademlia...");

            let mut swarm = SwarmBuilder::with_existing_identity(local_key)
                .with_tokio()
                .with_tcp(
                    tcp::Config::default(),
                    noise::Config::new,
                    yamux::Config::default,
                )
                .unwrap()
                .with_behaviour(|key| {
                    let peer_id = PeerId::from(key.public());
                    let mut kademlia =
                        kad::Behaviour::new(peer_id, kad::store::MemoryStore::new(peer_id));
                    // Server mode: this node will store and serve DHT records
                    kademlia.set_mode(Some(kad::Mode::Server));
                    let mdns =
                        mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id).unwrap();
                    NyxBehaviour { kademlia, mdns }
                })
                .unwrap()
                .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
                .build();

            swarm
                .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
                .unwrap();

            // Join the global Kademlia DHT via the public libp2p bootstrap node
            let bootaddr = BOOTSTRAP_ADDR.parse().unwrap();
            let bootid = PeerId::from_str(BOOTSTRAP_ID).unwrap();
            swarm
                .behaviour_mut()
                .kademlia
                .add_address(&bootid, bootaddr);
            swarm.behaviour_mut().kademlia.bootstrap().unwrap();

            // Publish our reachable address (real LAN IP + chat port) to the DHT
            // so that the Connect peer can look us up by our Peer ID.
            let record_key = RecordKey::new(&local_peer_id.to_bytes());
            let record_value = format!("{}:{}", local_ip, chat_port).into_bytes();
            let record = Record {
                key: record_key,
                value: record_value,
                publisher: None,
                expires: None,
            };
            let _ = swarm
                .behaviour_mut()
                .kademlia
                .put_record(record, Quorum::One);

            println!(
                "Registering Chat Port [{}] to the local/global mesh...",
                chat_port
            );
            println!("Waiting for a peer to connect. Press Ctrl+C to abort.");

            loop {
                tokio::select! {
                    event = swarm.select_next_some() => match event {
                        // mDNS: when a peer announces itself on the LAN, add
                        // its address to Kademlia so routing works locally too
                        SwarmEvent::Behaviour(NyxBehaviourEvent::Mdns(
                            mdns::Event::Discovered(peers),
                        )) => {
                            for (peer_id, multiaddr) in peers {
                                swarm
                                    .behaviour_mut()
                                    .kademlia
                                    .add_address(&peer_id, multiaddr);
                            }
                        }
                        SwarmEvent::Behaviour(NyxBehaviourEvent::Kademlia(
                            kad::Event::OutboundQueryProgressed {
                                result: kad::QueryResult::PutRecord(Ok(_)),
                                ..
                            },
                        )) => {
                            println!("Successfully anchored to the DHT!");
                        }
                        _ => {}
                    },

                    // When the Connect peer opens a raw TCP connection to our
                    // chat port, hand the socket off to the chat handler
                    incoming = p2p_listener.accept() => {
                        if let Ok((socket, peer_addr)) = incoming {
                            println!(
                                "\n\n>>> DIRECT P2P CONNECTION ESTABLISHED FROM {} <<<",
                                peer_addr
                            );
                            handle_p2p_chat(socket).await;
                            break;
                        }
                    }
                }
            }
        }

        // ------------------------------------------------------------------ //
        // CONNECT — look up the Start peer and dial them                      //
        // ------------------------------------------------------------------ //
        Commands::Connect { id } => {
            println!("Booting Swarm Engine with mDNS & Kademlia...");

            let local_key = identity::Keypair::generate_ed25519();
            let mut swarm = SwarmBuilder::with_existing_identity(local_key)
                .with_tokio()
                .with_tcp(
                    tcp::Config::default(),
                    noise::Config::new,
                    yamux::Config::default,
                )
                .unwrap()
                .with_behaviour(|key: &identity::Keypair| {
                    let peer_id = PeerId::from(key.public());
                    // FIX: Connect node also needs a bootstrap entry so it
                    // can actually reach the global DHT when no mDNS peer
                    // happens to be sitting on the same LAN.
                    let mut kademlia =
                        kad::Behaviour::new(peer_id, kad::store::MemoryStore::new(peer_id));
                    let bootaddr: libp2p::Multiaddr = BOOTSTRAP_ADDR.parse().unwrap();
                    let bootid = PeerId::from_str(BOOTSTRAP_ID).unwrap();
                    kademlia.add_address(&bootid, bootaddr);
                    let mdns =
                        mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id).unwrap();
                    NyxBehaviour { kademlia, mdns }
                })
                .unwrap()
                .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
                .build();

            swarm
                .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
                .unwrap();

            // FIX: Bootstrap *before* querying so the routing table is
            // populated. Without this the Connect node only knows about
            // the single bootstrap peer and DHT lookups frequently fail.
            swarm.behaviour_mut().kademlia.bootstrap().unwrap();

            let target_peer_id = PeerId::from_str(id).expect("Invalid Peer ID format.");
            let record_key = RecordKey::new(&target_peer_id.to_bytes());

            println!("Searching Local & Global DHT for Peer [{}]...", id);

            let mut target_ip = String::new();

            'search: loop {
                swarm
                    .behaviour_mut()
                    .kademlia
                    .get_record(record_key.clone());

                let mut found = false;

                loop {
                    match swarm.select_next_some().await {
                        SwarmEvent::Behaviour(NyxBehaviourEvent::Mdns(
                            mdns::Event::Discovered(peers),
                        )) => {
                            for (peer_id, multiaddr) in peers {
                                swarm
                                    .behaviour_mut()
                                    .kademlia
                                    .add_address(&peer_id, multiaddr);
                            }
                        }

                        SwarmEvent::Behaviour(NyxBehaviourEvent::Kademlia(
                            kad::Event::OutboundQueryProgressed {
                                result:
                                    kad::QueryResult::GetRecord(Ok(kad::GetRecordOk::FoundRecord(
                                        record_info,
                                    ))),
                                ..
                            },
                        )) => {
                            target_ip =
                                String::from_utf8_lossy(&record_info.record.value).to_string();
                            found = true;
                            break;
                        }

                        SwarmEvent::Behaviour(NyxBehaviourEvent::Kademlia(
                            kad::Event::OutboundQueryProgressed {
                                result: kad::QueryResult::GetRecord(Err(_)),
                                ..
                            },
                        )) => {
                            break; // inner loop — will retry in outer loop
                        }

                        _ => {}
                    }
                }

                if found {
                    println!("Success! Found Peer at network address: {}", target_ip);
                    break 'search;
                } else {
                    println!("DHT propagation pending. Retrying search...");
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            }

            println!("Establishing secure direct tunnel...");
            match tokio::net::TcpStream::connect(&target_ip).await {
                Ok(p2p_stream) => {
                    println!("\n\n>>> DIRECT P2P CONNECTION ESTABLISHED <<<");
                    handle_p2p_chat(p2p_stream).await;
                }
                Err(e) => {
                    eprintln!(
                        "Fatal: Found peer at {}, but TCP connection failed: {}",
                        target_ip, e
                    );
                    eprintln!(
                        "Hint: ensure both machines are on the same LAN, or that \
                               the Start peer has forwarded port {} on their router.",
                        target_ip.split(':').last().unwrap_or("?")
                    );
                }
            }
        }
    }
}

// -------------------------------------------------------------------------- //
// Chat session                                                                //
// -------------------------------------------------------------------------- //

use rustyline_async::{Readline, ReadlineEvent};

/// Runs the interactive encrypted chat session over an established TCP stream.
///
/// Wire format for every message:
///   [ 4-byte big-endian length ][ 12-byte nonce ][ ciphertext + 16-byte tag ]
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

    // Spawn the readline loop in its own task so it doesn't block the
    // network receiver in our main select! loop below.
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
            // ---- Incoming message from peer --------------------------------
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
                    writeln!(stdout, "\nRead error — peer may have disconnected.").unwrap();
                    stdout.flush().unwrap();
                    break;
                }

                match crate::crypto::decrypt_message(&shared_secret, &payload) {
                    Ok(text) => {
                        if text == "/nyx_internal_drop_connection" {
                            writeln!(
                                stdout,
                                "\n[System]: Peer has securely closed the connection."
                            )
                            .unwrap();
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

            // ---- Outgoing message from keyboard ----------------------------
            keyboard_result = rx.recv() => {
                if let Some(line) = keyboard_result {
                    let command = line.trim();

                    if command == "nyx exit" {
                        // Send encrypted teardown signal before closing
                        let payload = crate::crypto::encrypt_message(
                            &shared_secret,
                            "/nyx_internal_drop_connection",
                        );
                        let _ = writer.write_u32(payload.len() as u32).await;
                        let _ = writer.write_all(&payload).await;
                        let _ = writer.flush().await;

                        writeln!(stdout, "Session ended. Disconnecting cleanly...").unwrap();
                        stdout.flush().unwrap();

                        // Brief pause so the peer can receive the teardown signal
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        break;
                    } else if !command.is_empty() {
                        let payload =
                            crate::crypto::encrypt_message(&shared_secret, command);
                        if writer.write_u32(payload.len() as u32).await.is_err()
                            || writer.write_all(&payload).await.is_err()
                        {
                            writeln!(
                                stdout,
                                "\nFailed to send message. Peer disconnected."
                            )
                            .unwrap();
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
