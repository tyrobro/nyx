# Nyx: Decentralized Anonymous P2P Terminal

Nyx is a military-grade, end-to-end encrypted (E2EE) terminal chat application built in Rust. It utilizes a completely decentralized architecture, relying on the global Kademlia Distributed Hash Table (DHT) and local mDNS to connect peers without *any* centralized coordination servers.

Welcome to the ghost protocol.

## 🚀 Features

* **True Decentralization:** Powered by `libp2p`, Nyx has no central servers. It weaves itself into the global IPFS mesh network and routes traffic peer-to-peer.
* **Dual-Discovery Engine:**
  * **Global:** Uses the Kademlia DHT to anchor and locate peers across the public internet.
  * **Local:** Uses mDNS (Multicast DNS) to instantly discover and connect to peers on the same local network, bypassing strict public DHT spam filters.
* **Cryptographic Handshake:** Employs Elliptic Curve Diffie-Hellman (`x25519-dalek`) to generate an ephemeral shared secret over an insecure connection.
* **AEAD Encryption:** All text messages are encrypted and authenticated in transit using `ChaCha20-Poly1305`. Tampering with network packets instantly severs the connection.
* **Asynchronous UI:** Built with `rustyline-async` and `tokio::select!`. Incoming network packets cleanly slide above your typing prompt, completely eliminating terminal text-tearing.
* **Graceful Teardown:** Internal cryptographic signals handle clean disconnects, avoiding dirty OS-level socket drops.

## 🛠️ Tech Stack

* **Runtime:** `tokio` (Asynchronous I/O)
* **Networking:** `libp2p` (TCP, Noise, Yamux, Kademlia, mDNS)
* **Cryptography:** `x25519-dalek`, `chacha20poly1305`
* **CLI/UI:** `clap`, `rustyline-async`

## 📦 Installation & Setup

You must have [Rust and Cargo](https://rustup.rs/) installed on your machine.

1. Clone the repository:
   ```bash
   git clone [https://github.com/yourusername/nyx.git](https://github.com/yourusername/nyx.git)
   cd nyx