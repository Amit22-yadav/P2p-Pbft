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
        info!("Connecting to peer at {}", address);
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

        info!("Connected to peer {} at {}", peer_id, address);

        // Create peer handle
        let (tx, rx) = mpsc::channel(100);
        let peer_handle = PeerHandle {
            info: PeerInfo {
                node_id: peer_id.clone(),
                address: address.to_string(),
                connected: true,
            },
            sender: tx,
        };

        // Store peer
        self.peers.write().insert(peer_id.clone(), peer_handle);

        // Spawn reader and writer tasks
        let message_tx = self.message_tx.clone();
        let peers = self.peers.clone();
        let peer_id_clone = peer_id.clone();

        // Writer task
        tokio::spawn(async move {
            peer_writer(write_half, rx).await;
        });

        // Reader task
        tokio::spawn(async move {
            if let Err(e) = peer_reader(read_half, peer_id_clone.clone(), message_tx).await {
                debug!("Peer {} reader ended: {}", peer_id_clone, e);
            }
            peers.write().remove(&peer_id_clone);
            info!("Peer {} disconnected", peer_id_clone);
        });

        // Try to connect to discovered peers
        for (_, addr) in known_peers {
            if !self.peers.read().contains_key(&peer_id) {
                let self_clone = self.clone_handle();
                tokio::spawn(async move {
                    let _ = self_clone.connect_to_peer(&addr).await;
                });
            }
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
        info!("Connecting to peer at {}", address);
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

        info!("Connected to peer {} at {}", peer_id, address);

        // Create peer handle
        let (tx, rx) = mpsc::channel(100);
        let peer_handle = PeerHandle {
            info: PeerInfo {
                node_id: peer_id.clone(),
                address: address.to_string(),
                connected: true,
            },
            sender: tx,
        };

        // Store peer
        self.peers.write().insert(peer_id.clone(), peer_handle);

        // Spawn reader and writer tasks
        let message_tx = self.message_tx.clone();
        let peers = self.peers.clone();
        let peer_id_clone = peer_id.clone();

        // Writer task
        tokio::spawn(async move {
            peer_writer(write_half, rx).await;
        });

        // Reader task
        tokio::spawn(async move {
            if let Err(e) = peer_reader(read_half, peer_id_clone.clone(), message_tx).await {
                debug!("Peer {} reader ended: {}", peer_id_clone, e);
            }
            peers.write().remove(&peer_id_clone);
            info!("Peer {} disconnected", peer_id_clone);
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
    our_listen_port: u16,
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

    info!("Handshake completed with peer {}", peer_id);

    // Create peer handle
    let (tx, rx) = mpsc::channel(100);
    let peer_address = address
        .split(':')
        .next()
        .map(|host| format!("{}:{}", host, peer_port))
        .unwrap_or(address);

    let peer_handle = PeerHandle {
        info: PeerInfo {
            node_id: peer_id.clone(),
            address: peer_address,
            connected: true,
        },
        sender: tx,
    };

    // Store peer
    peers.write().insert(peer_id.clone(), peer_handle);

    // Spawn writer task
    tokio::spawn(async move {
        peer_writer(write_half, rx).await;
    });

    // Reader task (runs in current task)
    let peer_id_clone = peer_id.clone();
    if let Err(e) = peer_reader(read_half, peer_id.clone(), message_tx).await {
        debug!("Peer {} reader ended: {}", peer_id, e);
    }
    peers.write().remove(&peer_id_clone);
    info!("Peer {} disconnected", peer_id_clone);

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

/// Receive a message from a stream
async fn receive_message<R: AsyncReadExt + Unpin>(
    reader: &mut R,
) -> Result<NetworkMessage, NetworkError> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;

    let mut data = vec![0u8; len];
    reader.read_exact(&mut data).await?;

    let msg = NetworkMessage::deserialize(&data)?;
    Ok(msg)
}
