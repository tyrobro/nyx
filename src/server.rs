use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

pub type Registry = Arc<Mutex<HashMap<String, String>>>;

pub async fn start_server(addr: &str, registry: Registry) -> io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    println!("Nyx Coordination Server listening on {}", addr);

    loop {
        let (mut socket, remote_addr) = listener.accept().await?;
        let registry_ref = registry.clone();

        tokio::spawn(async move {
            let mut buffer = [0; 1024];

            let registered_id: String;

            match socket.read(&mut buffer).await {
                Ok(n) if n > 0 => {
                    let message = String::from_utf8_lossy(&buffer[..n]).trim().to_string();

                    if message.starts_with("REGISTER:") {
                        let parts: Vec<&str> = message.split(":").collect();
                        if parts.len() == 3 {
                            let id = parts[1].to_string();
                            let port = parts[2].to_string();

                            registered_id = id.clone();

                            let target_addr = format!("{}:{}", remote_addr.ip(), port);

                            let mut map = registry_ref.lock().await;
                            map.insert(id.clone(), target_addr.clone());
                            println!("Registered peer ID [{}] to target [{}]", id, target_addr);
                        } else {
                            println!("Invalid register protocol from {}", remote_addr);
                            return;
                        }
                    } else if message.starts_with("QUERY:") {
                        let id = message.strip_prefix("QUERY:").unwrap().to_string();
                        let map = registry_ref.lock().await;

                        if let Some(peer_addr) = map.get(&id) {
                            let response = format!("FOUND:{}\n", peer_addr);
                            let _ = socket.write_all(response.as_bytes()).await;
                            println!("Answered query for [{}]: Found", id);
                        } else {
                            let _ = socket.write_all(b"NOT_FOUND\n").await;
                            println!("Answered query for [{}]: Not Found", id);
                        }
                        return;
                    } else {
                        println!("Unknown protocol message from {}", remote_addr);
                        return;
                    }
                }
                _ => return,
            }

            loop {
                match socket.read(&mut buffer).await {
                    Ok(0) => break,
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }

            let mut map = registry_ref.lock().await;
            map.remove(&registered_id);
            println!(
                "Peer [{}] disconnected. Wiped from registry.",
                registered_id
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpStream;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn test_server_registers_and_cleans_up_peer() {
        let registry: Registry = Arc::new(Mutex::new(HashMap::new()));
        let server_registry = registry.clone();

        tokio::spawn(async move {
            start_server("127.0.0.1:8080", server_registry)
                .await
                .unwrap();
        });

        let mut stream = None;
        for _ in 0..10 {
            if let Ok(s) = TcpStream::connect("127.0.0.1:8080").await {
                stream = Some(s);
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
        let mut stream = stream.expect("Failed to connect to server");

        stream
            .write_all(b"REGISTER:MOCK 8F3A 1234:54321")
            .await
            .unwrap();
        sleep(Duration::from_millis(50)).await;

        {
            let map = registry.lock().await;
            assert!(
                map.contains_key("MOCK 8F3A 1234"),
                "Server failed to register the ID"
            );
        }

        drop(stream);
        sleep(Duration::from_millis(50)).await;

        {
            let map = registry.lock().await;
            assert!(
                !map.contains_key("MOCK 8F3A 1234"),
                "Server failed to wipe the ID after disconnect!"
            );
        }
    }
}
