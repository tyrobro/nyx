use clap::Parser;
use futures::StreamExt;
use libp2p::{
    identity,
    kad::{self, Quorum, Record, RecordKey},
    mdns, noise,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, PeerId, SwarmBuilder,
};
use std::io::Write;
use std::net::UdpSocket;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};

mod crypto;

#[derive(NetworkBehaviour)]
struct NyxBehaviour {
    kademlia: kad::Behaviour<kad::store::MemoryStore>,
    mdns: mdns::tokio::Behaviour,
}

#[derive(Parser)]
#[command(name = "nyx")]
#[command(about = "Anonymous P2P terminal communication", long_about = None)]
struct Cli {}

fn get_local_ip() -> String {
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

const BOOTSTRAP_ADDR: &str =
    "/dnsaddr/bootstrap.libp2p.io/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN";
const BOOTSTRAP_ID: &str = "QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN";

#[tokio::main]
async fn main() {
    let _cli = Cli::parse();

    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    let p2p_listener = tokio::net::TcpListener::bind("0.0.0.0:0")
        .await
        .expect("Failed to bind chat listener");
    let chat_port = p2p_listener.local_addr().unwrap().port();
    let local_ip = get_local_ip();

    println!("========================================");
    println!("  Nyx Decentralized Node Initialized    ");
    println!("========================================");
    println!("Your Global Peer ID: {}", local_peer_id);
    println!("Detected local IP  : {}", local_ip);
    println!("Booting libp2p Swarm Engine with mDNS & Kademlia...");

    let is_busy = Arc::new(AtomicBool::new(false));
    let is_busy_listener = is_busy.clone();
    let (conn_tx, mut conn_rx) = tokio::sync::mpsc::channel(1);

    tokio::spawn(async move {
        loop {
            if let Ok((mut socket, peer_addr)) = p2p_listener.accept().await {
                use tokio::io::AsyncWriteExt;
                if is_busy_listener.load(Ordering::SeqCst) {
                    let _ = socket.write_all(&[0x01]).await;
                    let _ = socket.shutdown().await;
                } else {
                    let _ = socket.write_all(&[0x00]).await;
                    let _ = conn_tx.send((socket, peer_addr)).await;
                }
            }
        }
    });

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
            let mut kademlia = kad::Behaviour::new(peer_id, kad::store::MemoryStore::new(peer_id));
            kademlia.set_mode(Some(kad::Mode::Server));
            let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id).unwrap();
            NyxBehaviour { kademlia, mdns }
        })
        .unwrap()
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    swarm
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .unwrap();

    let bootaddr = BOOTSTRAP_ADDR.parse().unwrap();
    let bootid = PeerId::from_str(BOOTSTRAP_ID).unwrap();
    swarm
        .behaviour_mut()
        .kademlia
        .add_address(&bootid, bootaddr);
    swarm.behaviour_mut().kademlia.bootstrap().unwrap();

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
    println!("\n[COMMANDS]");
    println!("Type '/connect <PeerID>' to dial a peer.");
    println!("Or leave this terminal open to wait for incoming connections.");
    print!("nyx> ");
    std::io::stdout().flush().unwrap();

    let mut stdin = BufReader::new(tokio::io::stdin()).lines();
    let mut hunting_id: Option<RecordKey> = None;

    loop {
        tokio::select! {
            line = stdin.next_line(), if !is_busy.load(Ordering::SeqCst) => {
                if let Ok(Some(cmd)) = line {
                    let cmd = cmd.trim();
                    if cmd.starts_with("/connect ") {
                        let id_str = cmd.trim_start_matches("/connect ").trim();
                        match PeerId::from_str(id_str) {
                            Ok(target_peer_id) => {
                                if target_peer_id == local_peer_id {
                                    println!("[Error]: You cannot dial your own Peer ID.");
                                    print!("nyx> ");
                                    std::io::stdout().flush().unwrap();
                                    continue;
                                }

                                println!("[System]: Searching Local & Global DHT for Peer [{}]...", id_str);
                                let target_key = RecordKey::new(&target_peer_id.to_bytes());
                                hunting_id = Some(target_key.clone());
                                swarm.behaviour_mut().kademlia.get_record(target_key);
                            }
                            Err(_) => {
                                println!("[Error]: Invalid Peer ID format.");
                                print!("nyx> ");
                                std::io::stdout().flush().unwrap();
                            }
                        }
                    } else if cmd == "exit" {
                        std::process::exit(0);
                    } else if !cmd.is_empty() {
                        println!("[Error]: Unknown command. Use '/connect <ID>' or 'exit'.");
                        print!("nyx> ");
                        std::io::stdout().flush().unwrap();
                    } else {
                        print!("nyx> ");
                        std::io::stdout().flush().unwrap();
                    }
                }
            }

            event = swarm.select_next_some() => match event {
                SwarmEvent::Behaviour(NyxBehaviourEvent::Mdns(mdns::Event::Discovered(peers))) => {
                    for (peer_id, multiaddr) in peers {
                        swarm.behaviour_mut().kademlia.add_address(&peer_id, multiaddr);
                    }
                }
                SwarmEvent::Behaviour(NyxBehaviourEvent::Kademlia(kad::Event::OutboundQueryProgressed {
                    result: kad::QueryResult::PutRecord(Ok(_)), ..
                })) => {
                    println!("\n[System]: Successfully anchored to the Global DHT!");
                    print!("nyx> ");
                    std::io::stdout().flush().unwrap();
                }
                SwarmEvent::Behaviour(NyxBehaviourEvent::Kademlia(kad::Event::OutboundQueryProgressed {
                    result: kad::QueryResult::GetRecord(Ok(kad::GetRecordOk::FoundRecord(record_info))), ..
                })) => {
                    if let Some(ref active_hunt) = hunting_id {
                        if record_info.record.key == *active_hunt {
                            let target_ip = String::from_utf8_lossy(&record_info.record.value).to_string();
                            println!("[System]: Success! Found Peer at network address: {}", target_ip);
                            println!("[System]: Establishing secure direct tunnel...");

                            use tokio::io::AsyncReadExt;
                            match tokio::net::TcpStream::connect(&target_ip).await {
                                Ok(mut p2p_stream) => {
                                    let mut status = [0u8; 1];
                                    if p2p_stream.read_exact(&mut status).await.is_ok() {
                                        if status[0] == 0x01 {
                                            println!("[System]: Connection rejected. Peer is currently busy.");
                                            hunting_id = None;
                                            print!("nyx> ");
                                            std::io::stdout().flush().unwrap();
                                        } else if status[0] == 0x00 {
                                            is_busy.store(true, Ordering::SeqCst);
                                            println!("\n\n>>> DIRECT P2P CONNECTION ESTABLISHED <<<");

                                            let is_busy_clone = is_busy.clone();
                                            tokio::spawn(async move {
                                                handle_p2p_chat(p2p_stream).await;
                                                is_busy_clone.store(false, Ordering::SeqCst);
                                                print!("\n[System]: Tunnel closed. Returned to global node.\nnyx> ");
                                                std::io::stdout().flush().unwrap();
                                            });
                                            hunting_id = None;
                                        } else {
                                            println!("[Error]: Unrecognized status byte from peer.");
                                            hunting_id = None;
                                            print!("nyx> ");
                                            std::io::stdout().flush().unwrap();
                                        }
                                    } else {
                                        println!("[Error]: Failed to read handshake status from peer.");
                                        hunting_id = None;
                                        print!("nyx> ");
                                        std::io::stdout().flush().unwrap();
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[Error]: Found peer at {}, but TCP connection failed: {}", target_ip, e);
                                    hunting_id = None;
                                    print!("nyx> ");
                                    std::io::stdout().flush().unwrap();
                                }
                            }
                        }
                    }
                }
                SwarmEvent::Behaviour(NyxBehaviourEvent::Kademlia(kad::Event::OutboundQueryProgressed {
                    result: kad::QueryResult::GetRecord(Err(_)), ..
                })) => {
                    if hunting_id.is_some() {
                        println!("[System]: DHT search missed. Peer might be offline, or propagation is pending.");
                        hunting_id = None;
                        print!("nyx> ");
                        std::io::stdout().flush().unwrap();
                    }
                }
                _ => {}
            },

            incoming = conn_rx.recv() => {
                if let Some((socket, peer_addr)) = incoming {
                    is_busy.store(true, Ordering::SeqCst);
                    println!("\n\n>>> DIRECT P2P CONNECTION ESTABLISHED FROM {} <<<", peer_addr);

                    let is_busy_clone = is_busy.clone();
                    tokio::spawn(async move {
                        handle_p2p_chat(socket).await;
                        is_busy_clone.store(false, Ordering::SeqCst);
                        print!("\n[System]: Tunnel closed. Returned to global node.\nnyx> ");
                        std::io::stdout().flush().unwrap();
                    });
                }
            }
        }
    }
}

use rustyline_async::{Readline, ReadlineEvent};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

    loop {
        tokio::select! {
            len_result = reader.read_u32() => {
                let length = match len_result {
                    Ok(l) => l as usize,
                    Err(_) => {
                        writeln!(stdout, "\nPeer disconnected abruptly.").unwrap();
                        break;
                    }
                };

                let mut payload = vec![0u8; length];
                if reader.read_exact(&mut payload).await.is_err() {
                    writeln!(stdout, "\nRead error — peer may have disconnected.").unwrap();
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
                            break;
                        }
                        writeln!(stdout, "[Peer]: {}", text).unwrap();
                    }
                    Err(e) => {
                        writeln!(stdout, "\n[SECURITY ALERT] Decryption failed: {}", e).unwrap();
                        break;
                    }
                }
            }

            readline_result = rl.readline() => {
                match readline_result {
                    Ok(ReadlineEvent::Line(line)) => {
                        let command = line.trim();
                        rl.add_history_entry(line.clone());

                        if command == "nyx exit" {
                            let payload = crate::crypto::encrypt_message(
                                &shared_secret,
                                "/nyx_internal_drop_connection",
                            );
                            let _ = writer.write_u32(payload.len() as u32).await;
                            let _ = writer.write_all(&payload).await;
                            let _ = writer.flush().await;

                            writeln!(stdout, "Session ended. Disconnecting cleanly...").unwrap();
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            break;
                        } else if !command.is_empty() {
                            let payload = crate::crypto::encrypt_message(&shared_secret, command);
                            if writer.write_u32(payload.len() as u32).await.is_err()
                                || writer.write_all(&payload).await.is_err()
                            {
                                writeln!(
                                    stdout,
                                    "\nFailed to send message. Peer disconnected."
                                )
                                .unwrap();
                                break;
                            }
                        }
                    }
                    Ok(ReadlineEvent::Eof) | Ok(ReadlineEvent::Interrupted) => {
                        let payload = crate::crypto::encrypt_message(
                            &shared_secret,
                            "/nyx_internal_drop_connection",
                        );
                        let _ = writer.write_u32(payload.len() as u32).await;
                        let _ = writer.write_all(&payload).await;
                        let _ = writer.flush().await;
                        break;
                    }
                    Err(_) => break,
                }
            }
        }
    }

    let _ = rl.flush();
    drop(rl);
    drop(stdout);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}
