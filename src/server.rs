use std::io;
use tokio::net::TcpListener;

pub async fn start_server(addr: &str) -> io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    println!("Nyx Coordination Server listening on {}", addr);

    loop {
        let (socket, remote_addr) = listener.accept().await?;

        println!("New peer connected: {}", remote_addr);

        tokio::spawn(async move {
            let _ = socket;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpStream;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn test_server_binds_to_port() {
        tokio::spawn(async {
            start_server("127.0.0.1:8080").await.unwrap();
        });

        let mut connected = false;

        for _ in 0..10 {
            if TcpStream::connect("127.0.0.1:8080").await.is_ok() {
                connected = true;
                break;
            }
        }

        sleep(Duration::from_millis(50)).await;

        assert!(
            connected,
            "Failed to connect to the server. Is it listening?"
        );
    }
}
