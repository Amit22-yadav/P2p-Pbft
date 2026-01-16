use crate::message::{NetworkMessage, NodeId};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),
    #[error("Connection failed to {0}")]
    ConnectionFailed(String),
    #[error("Peer not found: {0}")]
    PeerNotFound(NodeId),
    #[error("Channel closed")]
    ChannelClosed,
}

/// Information about a connected peer
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub node_id: NodeId,
    pub address: String,
    pub connected: bool,
}

/// Handle for sending messages to a peer
#[derive(Clone)]
pub struct PeerHandle {
    pub info: PeerInfo,
    sender: mpsc::Sender<NetworkMessage>,
    /// Unique connection ID to identify this specific connection
    connection_id: u64,
}

use std::sync::atomic::{AtomicU64, Ordering};
static CONNECTION_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_connection_id() -> u64 {
    CONNECTION_COUNTER.fetch_add(1, Ordering::SeqCst)
}

impl PeerHandle {
    pub async fn send(&self, msg: NetworkMessage) -> Result<(), NetworkError> {
        self.sender
            .send(msg)
            .await
            .map_err(|_| NetworkError::ChannelClosed)
    }
}

/// P2P Network manager
pub struct Network {
    pub node_id: NodeId,
    pub listen_port: u16,
    pub peers: Arc<RwLock<HashMap<NodeId, PeerHandle>>>,
    message_tx: mpsc::Sender<(NodeId, NetworkMessage)>,
    message_rx: Option<mpsc::Receiver<(NodeId, NetworkMessage)>>,
}

impl Network {
    pub fn new(node_id: NodeId, listen_port: u16) -> Self {
        let (message_tx, message_rx) = mpsc::channel(1000);
        Self {
            node_id,
            listen_port,
            peers: Arc::new(RwLock::new(HashMap::new())),
            message_tx,
            message_rx: Some(message_rx),
        }
    }

    /// Take the message receiver (can only be called once)
    pub fn take_message_receiver(&mut self) -> Option<mpsc::Receiver<(NodeId, NetworkMessage)>> {
        self.message_rx.take()
    }

    /// Get list of connected peers
    pub fn get_peers(&self) -> Vec<PeerInfo> {
        self.peers.read().values().map(|p| p.info.clone()).collect()
    }

    /// Get number of connected peers
    pub fn peer_count(&self) -> usize {
        self.peers.read().len()
    }

    /// Start listening for incoming connections
    pub async fn start_listener(&self) -> Result<(), NetworkError> {
        let addr = format!("0.0.0.0:{}", self.listen_port);
        let listener = TcpListener::bind(&addr).await?;
        info!("Listening on {}", addr);

        let peers = self.peers.clone();
        let message_tx = self.message_tx.clone();
        let node_id = self.node_id.clone();
        let listen_port = self.listen_port;

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        info!("Incoming connection from {}", addr);
                        let peers = peers.clone();
                        let message_tx = message_tx.clone();
                        let node_id = node_id.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_incoming_connection(
                                stream,
                                addr.to_string(),
                                peers,
                                message_tx,
                                node_id,
                                listen_port,
                            )
                            .await
                            {
                                error!("Error handling connection from {}: {}", addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept connection: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// Connect to a peer
    pub async fn connect_to_peer(&self, address: &str) -> Result<NodeId, NetworkError> {
        debug!("Connecting to peer at {}", address);
        let stream = TcpStream::connect(address)
            .await
            .map_err(|_| NetworkError::ConnectionFailed(address.to_string()))?;

        let (read_half, mut write_half) = stream.into_split();

        // Send handshake
        let handshake = NetworkMessage::Handshake {
            node_id: self.node_id.clone(),
            listen_port: self.listen_port,
        };
        send_message(&mut write_half, &handshake).await?;

        // Wait for handshake ack
        let mut read_half = tokio::io::BufReader::new(read_half);
        let response = receive_message(&mut read_half).await?;

        let (peer_id, known_peers) = match response {
            NetworkMessage::HandshakeAck {
                node_id,
                known_peers,
            } => (node_id, known_peers),
            _ => {
                return Err(NetworkError::ConnectionFailed(
                    "Invalid handshake response".to_string(),
                ))
            }
        };

        // Check if we already have this peer (prevents duplicate connections)
        if self.peers.read().contains_key(&peer_id) {
            debug!("Already connected to peer {}, dropping duplicate connection", &peer_id[..16]);
            return Ok(peer_id);
        }

        // Create peer handle
        let (tx, rx) = mpsc::channel(100);
        let conn_id = next_connection_id();
        let peer_handle = PeerHandle {
            info: PeerInfo {
                node_id: peer_id.clone(),
                address: address.to_string(),
                connected: true,
            },
            sender: tx,
            connection_id: conn_id,
        };

        // Store peer (use entry API to avoid race condition)
        {
            let mut peers = self.peers.write();
            if peers.contains_key(&peer_id) {
                debug!("Already connected to peer {} (race), dropping duplicate", &peer_id[..16]);
                return Ok(peer_id);
            }
            peers.insert(peer_id.clone(), peer_handle);
        }

        // Log connection with peer count
        let peer_count = self.peers.read().len();
        info!("✓ Peer connected: {}... at {} [Total peers: {}]", &peer_id[..16], address, peer_count);

        // Spawn reader and writer tasks
        let message_tx = self.message_tx.clone();
        let peers = self.peers.clone();
        let peer_id_clone = peer_id.clone();

        // Writer task
        tokio::spawn(async move {
            peer_writer(write_half, rx).await;
        });

        // Reader task - only remove peer if connection_id matches
        tokio::spawn(async move {
            if let Err(e) = peer_reader(read_half, peer_id_clone.clone(), message_tx).await {
                debug!("Peer {} reader ended: {}", peer_id_clone, e);
            }
            // Only remove if this connection is still the active one
            let mut peers_guard = peers.write();
            if let Some(handle) = peers_guard.get(&peer_id_clone) {
                if handle.connection_id == conn_id {
                    peers_guard.remove(&peer_id_clone);
                    let remaining = peers_guard.len();
                    drop(peers_guard);
                    info!("✗ Peer disconnected: {}... [Remaining peers: {}]", &peer_id_clone[..16], remaining);
                }
            }
        });

        // Try to connect to discovered peers to form a mesh network
        for (discovered_peer_id, addr) in known_peers {
            // Skip if we already have this peer or if it's ourselves
            if discovered_peer_id == self.node_id {
                continue;
            }
            if self.peers.read().contains_key(&discovered_peer_id) {
                continue;
            }
            info!("Discovered new peer {} at {}, connecting...", &discovered_peer_id[..16], addr);
            let self_clone = self.clone_handle();
            tokio::spawn(async move {
                if let Err(e) = self_clone.connect_to_peer(&addr).await {
                    debug!("Failed to connect to discovered peer: {}", e);
                }
            });
        }

        Ok(peer_id)
    }

    /// Send a message to a specific peer
    pub async fn send_to_peer(
        &self,
        peer_id: &NodeId,
        msg: NetworkMessage,
    ) -> Result<(), NetworkError> {
        let peers = self.peers.read();
        let peer = peers
            .get(peer_id)
            .ok_or_else(|| NetworkError::PeerNotFound(peer_id.clone()))?;
        peer.send(msg).await
    }

    /// Broadcast a message to all peers
    pub async fn broadcast(&self, msg: NetworkMessage) {
        let peers: Vec<PeerHandle> = self.peers.read().values().cloned().collect();
        for peer in peers {
            if let Err(e) = peer.send(msg.clone()).await {
                warn!("Failed to send to peer {}: {}", peer.info.node_id, e);
            }
        }
    }

    /// Clone just the handle parts for spawning tasks
    fn clone_handle(&self) -> NetworkHandle {
        NetworkHandle {
            node_id: self.node_id.clone(),
            listen_port: self.listen_port,
            peers: self.peers.clone(),
            message_tx: self.message_tx.clone(),
        }
    }
}

/// Lightweight handle for network operations
#[derive(Clone)]
pub struct NetworkHandle {
    pub node_id: NodeId,
    pub listen_port: u16,
    peers: Arc<RwLock<HashMap<NodeId, PeerHandle>>>,
    message_tx: mpsc::Sender<(NodeId, NetworkMessage)>,
}

impl NetworkHandle {
    pub async fn connect_to_peer(&self, address: &str) -> Result<NodeId, NetworkError> {
        debug!("Connecting to peer at {}", address);
        let stream = TcpStream::connect(address)
            .await
            .map_err(|_| NetworkError::ConnectionFailed(address.to_string()))?;

        let (read_half, mut write_half) = stream.into_split();

        // Send handshake
        let handshake = NetworkMessage::Handshake {
            node_id: self.node_id.clone(),
            listen_port: self.listen_port,
        };
        send_message(&mut write_half, &handshake).await?;

        // Wait for handshake ack
        let mut read_half = tokio::io::BufReader::new(read_half);
        let response = receive_message(&mut read_half).await?;

        let (peer_id, _known_peers) = match response {
            NetworkMessage::HandshakeAck {
                node_id,
                known_peers,
            } => (node_id, known_peers),
            _ => {
                return Err(NetworkError::ConnectionFailed(
                    "Invalid handshake response".to_string(),
                ))
            }
        };

        // Check if we already have this peer
        if self.peers.read().contains_key(&peer_id) {
            debug!("Already connected to peer {}, dropping duplicate", &peer_id[..16]);
            return Ok(peer_id);
        }

        // Create peer handle
        let (tx, rx) = mpsc::channel(100);
        let conn_id = next_connection_id();
        let peer_handle = PeerHandle {
            info: PeerInfo {
                node_id: peer_id.clone(),
                address: address.to_string(),
                connected: true,
            },
            sender: tx,
            connection_id: conn_id,
        };

        // Store peer (check again to avoid race)
        {
            let mut peers_guard = self.peers.write();
            if peers_guard.contains_key(&peer_id) {
                debug!("Already connected to peer {} (race), dropping duplicate", &peer_id[..16]);
                return Ok(peer_id);
            }
            peers_guard.insert(peer_id.clone(), peer_handle);
        }

        info!("✓ Peer connected: {}... at {} [Total peers: {}]", &peer_id[..16], address, self.peers.read().len());

        // Spawn reader and writer tasks
        let message_tx = self.message_tx.clone();
        let peers = self.peers.clone();
        let peer_id_clone = peer_id.clone();

        // Writer task
        tokio::spawn(async move {
            peer_writer(write_half, rx).await;
        });

        // Reader task - only remove if connection_id matches
        tokio::spawn(async move {
            if let Err(e) = peer_reader(read_half, peer_id_clone.clone(), message_tx).await {
                debug!("Peer {} reader ended: {}", peer_id_clone, e);
            }
            // Only remove if this connection is still the active one
            let mut peers_guard = peers.write();
            if let Some(handle) = peers_guard.get(&peer_id_clone) {
                if handle.connection_id == conn_id {
                    peers_guard.remove(&peer_id_clone);
                    let remaining = peers_guard.len();
                    drop(peers_guard);
                    info!("✗ Peer disconnected: {}... [Remaining peers: {}]", &peer_id_clone[..16], remaining);
                }
            }
        });

        Ok(peer_id)
    }

    pub async fn broadcast(&self, msg: NetworkMessage) {
        let peers: Vec<PeerHandle> = self.peers.read().values().cloned().collect();
        for peer in peers {
            if let Err(e) = peer.send(msg.clone()).await {
                warn!("Failed to send to peer {}: {}", peer.info.node_id, e);
            }
        }
    }

    pub fn get_peers(&self) -> Vec<PeerInfo> {
        self.peers.read().values().map(|p| p.info.clone()).collect()
    }
}

/// Handle an incoming connection
async fn handle_incoming_connection(
    stream: TcpStream,
    address: String,
    peers: Arc<RwLock<HashMap<NodeId, PeerHandle>>>,
    message_tx: mpsc::Sender<(NodeId, NetworkMessage)>,
    our_node_id: NodeId,
    _our_listen_port: u16,
) -> Result<(), NetworkError> {
    let (read_half, mut write_half) = stream.into_split();
    let mut read_half = tokio::io::BufReader::new(read_half);

    // Receive handshake
    let msg = receive_message(&mut read_half).await?;

    let (peer_id, peer_port) = match msg {
        NetworkMessage::Handshake {
            node_id,
            listen_port,
        } => (node_id, listen_port),
        _ => {
            return Err(NetworkError::ConnectionFailed(
                "Expected handshake".to_string(),
            ))
        }
    };

    // Check if we already have this peer (prevents duplicate connections)
    // Use deterministic tie-breaking: lower node ID wins to keep connection
    let already_connected = peers.read().contains_key(&peer_id);
    if already_connected {
        debug!("Already connected to peer {}, dropping incoming duplicate", &peer_id[..16]);
        // Still send ack so the other side completes handshake cleanly
        let known_peers: Vec<(NodeId, String)> = peers
            .read()
            .values()
            .map(|p| (p.info.node_id.clone(), p.info.address.clone()))
            .collect();
        let ack = NetworkMessage::HandshakeAck {
            node_id: our_node_id,
            known_peers,
        };
        let _ = send_message(&mut write_half, &ack).await;
        return Ok(());
    }

    // Send handshake ack with known peers
    let known_peers: Vec<(NodeId, String)> = peers
        .read()
        .values()
        .map(|p| (p.info.node_id.clone(), p.info.address.clone()))
        .collect();

    let ack = NetworkMessage::HandshakeAck {
        node_id: our_node_id,
        known_peers,
    };
    send_message(&mut write_half, &ack).await?;

    // Create peer handle
    let (tx, rx) = mpsc::channel(100);
    let conn_id = next_connection_id();
    let peer_address = address
        .split(':')
        .next()
        .map(|host| format!("{}:{}", host, peer_port))
        .unwrap_or(address);

    let peer_handle = PeerHandle {
        info: PeerInfo {
            node_id: peer_id.clone(),
            address: peer_address.clone(),
            connected: true,
        },
        sender: tx,
        connection_id: conn_id,
    };

    // Store peer (check again to avoid race condition)
    {
        let mut peers_guard = peers.write();
        if peers_guard.contains_key(&peer_id) {
            debug!("Already connected to peer {} (race), dropping incoming duplicate", &peer_id[..16]);
            return Ok(());
        }
        peers_guard.insert(peer_id.clone(), peer_handle);
    }

    // Log connection with peer count
    let peer_count = peers.read().len();
    info!("✓ Peer connected: {}... at {} [Total peers: {}]", &peer_id[..16], peer_address, peer_count);

    // Spawn writer task
    tokio::spawn(async move {
        peer_writer(write_half, rx).await;
    });

    // Reader task (runs in current task)
    let peer_id_clone = peer_id.clone();
    if let Err(e) = peer_reader(read_half, peer_id.clone(), message_tx).await {
        debug!("Peer {} reader ended: {}", peer_id, e);
    }
    // Only remove if this connection is still the active one
    let mut peers_guard = peers.write();
    if let Some(handle) = peers_guard.get(&peer_id_clone) {
        if handle.connection_id == conn_id {
            peers_guard.remove(&peer_id_clone);
            let remaining = peers_guard.len();
            drop(peers_guard);
            info!("✗ Peer disconnected: {}... [Remaining peers: {}]", &peer_id_clone[..16], remaining);
        }
    }

    Ok(())
}

/// Read messages from a peer
async fn peer_reader<R: AsyncReadExt + Unpin>(
    mut reader: R,
    peer_id: NodeId,
    message_tx: mpsc::Sender<(NodeId, NetworkMessage)>,
) -> Result<(), NetworkError> {
    loop {
        let msg = receive_message(&mut reader).await?;
        if message_tx.send((peer_id.clone(), msg)).await.is_err() {
            break;
        }
    }
    Ok(())
}

/// Write messages to a peer
async fn peer_writer<W: AsyncWriteExt + Unpin>(
    mut writer: W,
    mut rx: mpsc::Receiver<NetworkMessage>,
) {
    while let Some(msg) = rx.recv().await {
        if let Err(e) = send_message(&mut writer, &msg).await {
            error!("Failed to send message: {}", e);
            break;
        }
    }
}

/// Send a message over a stream
async fn send_message<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    msg: &NetworkMessage,
) -> Result<(), NetworkError> {
    let data = msg.serialize()?;
    let len = data.len() as u32;
    writer.write_all(&len.to_le_bytes()).await?;
    writer.write_all(&data).await?;
    writer.flush().await?;
    Ok(())
}

/// Maximum message size (16 MB) to prevent OOM attacks
const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;

/// Receive a message from a stream
async fn receive_message<R: AsyncReadExt + Unpin>(
    reader: &mut R,
) -> Result<NetworkMessage, NetworkError> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;

    // Check message size to prevent OOM attacks
    if len > MAX_MESSAGE_SIZE {
        return Err(NetworkError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Message size {} exceeds maximum {}", len, MAX_MESSAGE_SIZE),
        )));
    }

    let mut data = vec![0u8; len];
    reader.read_exact(&mut data).await?;

    let msg = NetworkMessage::deserialize(&data)?;
    Ok(msg)
}
