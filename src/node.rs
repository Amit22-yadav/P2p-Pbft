use crate::crypto::KeyPair;
use crate::message::{NetworkMessage, NodeId, Request};
use crate::network::Network;
use crate::pbft::{OutgoingMessage, PbftConfig, PbftConsensus, PbftState};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// A complete PBFT node combining networking and consensus
pub struct Node {
    pub node_id: NodeId,
    network: Network,
    consensus: Option<PbftConsensus>,
    outgoing_rx: mpsc::Receiver<OutgoingMessage>,
    committed_rx: mpsc::Receiver<Request>,
}

impl Node {
    /// Create a new node
    pub fn new(listen_port: u16, seed: Option<[u8; 32]>) -> Self {
        // Generate or derive keypair
        let keypair = match seed {
            Some(s) => KeyPair::from_seed(&s),
            None => KeyPair::generate(),
        };

        let node_id = keypair.public_key_hex();
        info!("Node ID: {}", node_id);

        let network = Network::new(node_id.clone(), listen_port);

        // Create channels for outgoing messages and committed requests
        let (outgoing_tx, outgoing_rx) = mpsc::channel(1000);
        let (committed_tx, committed_rx) = mpsc::channel(1000);

        // Create placeholder consensus (will be initialized when we know all replicas)
        let config = PbftConfig::new(1); // Will be updated
        let state = PbftState::new(node_id.clone(), keypair, vec![node_id.clone()], config);
        let consensus = PbftConsensus::new(state, outgoing_tx, committed_tx);

        Self {
            node_id,
            network,
            consensus: Some(consensus),
            outgoing_rx,
            committed_rx,
        }
    }

    /// Initialize consensus with known replicas
    pub fn initialize_consensus(&mut self, replicas: Vec<NodeId>, keypair: KeyPair) {
        let n = replicas.len();
        let config = PbftConfig::new(n);

        info!(
            "Initializing PBFT with {} replicas, f={}, quorum={}",
            n,
            config.f,
            config.quorum()
        );

        let (outgoing_tx, outgoing_rx) = mpsc::channel(1000);
        let (committed_tx, committed_rx) = mpsc::channel(1000);

        let state = PbftState::new(self.node_id.clone(), keypair, replicas, config);
        let consensus = PbftConsensus::new(state, outgoing_tx, committed_tx);

        self.consensus = Some(consensus);
        self.outgoing_rx = outgoing_rx;
        self.committed_rx = committed_rx;
    }

    /// Get the node ID
    pub fn id(&self) -> &NodeId {
        &self.node_id
    }

    /// Start the node
    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Start network listener
        self.network.start_listener().await?;

        // Take message receiver from network
        let mut network_rx = self
            .network
            .take_message_receiver()
            .expect("Message receiver already taken");

        let consensus = self.consensus.take().expect("Consensus not initialized");

        // Spawn task to handle outgoing messages
        let network_peers = self.network.peers.clone();
        let mut outgoing_rx = std::mem::replace(&mut self.outgoing_rx, mpsc::channel(1).1);

        tokio::spawn(async move {
            while let Some(outgoing) = outgoing_rx.recv().await {
                match outgoing {
                    OutgoingMessage::Broadcast(msg) => {
                        // Send to all peers
                        let peers: Vec<_> = network_peers.read().values().cloned().collect();
                        for peer in peers {
                            if let Err(e) = peer.send(msg.clone()).await {
                                warn!("Failed to send to {}: {}", peer.info.node_id, e);
                            }
                        }
                    }
                    OutgoingMessage::SendTo { target, message } => {
                        // Send to specific peer only
                        let peer = network_peers.read().get(&target).cloned();
                        if let Some(peer) = peer {
                            if let Err(e) = peer.send(message).await {
                                warn!("Failed to send to {}: {}", target, e);
                            }
                        } else {
                            warn!("Target peer {} not found for targeted send", target);
                        }
                    }
                }
            }
        });

        // Spawn task to handle committed requests
        let mut committed_rx = std::mem::replace(&mut self.committed_rx, mpsc::channel(1).1);
        tokio::spawn(async move {
            while let Some(request) = committed_rx.recv().await {
                info!(
                    "Request committed: client={}, op_len={}",
                    request.client_id,
                    request.operation.len()
                );
                // Here you would execute the actual application logic
            }
        });

        // Spawn timeout checker
        let consensus_for_timeout = consensus.state().clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                let state = consensus_for_timeout.read().await;
                if !state.is_primary()
                    && state.last_primary_activity.elapsed() > state.config.view_change_timeout
                {
                    drop(state);
                    // Would trigger view change here
                    warn!("Primary timeout detected");
                }
            }
        });

        // Main message processing loop
        info!("Node started, processing messages...");
        while let Some((peer_id, msg)) = network_rx.recv().await {
            match msg {
                NetworkMessage::Consensus(consensus_msg) => {
                    if let Err(e) = consensus.process_message(consensus_msg).await {
                        warn!("Error processing consensus message from {}: {}", peer_id, e);
                    }
                }
                NetworkMessage::Ping { node_id, timestamp } => {
                    let pong = NetworkMessage::Pong {
                        node_id: self.node_id.clone(),
                        timestamp,
                    };
                    if let Err(e) = self.network.send_to_peer(&node_id, pong).await {
                        warn!("Failed to send pong to {}: {}", node_id, e);
                    }
                }
                NetworkMessage::Pong { .. } => {
                    // Heartbeat received
                }
                NetworkMessage::Handshake { .. } | NetworkMessage::HandshakeAck { .. } => {
                    // Handled by network layer
                }
            }
        }

        Ok(())
    }

    /// Connect to a peer
    pub async fn connect(&self, address: &str) -> Result<NodeId, Box<dyn std::error::Error>> {
        Ok(self.network.connect_to_peer(address).await?)
    }

    /// Submit a request (as a client)
    pub async fn submit_request(&self, operation: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
        let request = Request::new(self.node_id.clone(), operation);

        if let Some(ref consensus) = self.consensus {
            consensus.handle_request(request).await?;
        }

        Ok(())
    }

    /// Get number of connected peers
    pub fn peer_count(&self) -> usize {
        self.network.peer_count()
    }

    /// Get list of connected peers
    pub fn peers(&self) -> Vec<crate::network::PeerInfo> {
        self.network.get_peers()
    }
}

/// Builder for creating a Node with configuration
pub struct NodeBuilder {
    listen_port: u16,
    seed: Option<[u8; 32]>,
    bootstrap_peers: Vec<String>,
}

impl NodeBuilder {
    pub fn new(listen_port: u16) -> Self {
        Self {
            listen_port,
            seed: None,
            bootstrap_peers: Vec::new(),
        }
    }

    pub fn with_seed(mut self, seed: [u8; 32]) -> Self {
        self.seed = Some(seed);
        self
    }

    pub fn with_bootstrap_peer(mut self, address: String) -> Self {
        self.bootstrap_peers.push(address);
        self
    }

    pub fn with_bootstrap_peers(mut self, addresses: Vec<String>) -> Self {
        self.bootstrap_peers.extend(addresses);
        self
    }

    pub fn build(self) -> Node {
        Node::new(self.listen_port, self.seed)
    }

    pub async fn build_and_connect(self) -> Result<Node, Box<dyn std::error::Error>> {
        let node = Node::new(self.listen_port, self.seed);

        for peer in &self.bootstrap_peers {
            match node.connect(peer).await {
                Ok(peer_id) => info!("Connected to bootstrap peer {} at {}", peer_id, peer),
                Err(e) => error!("Failed to connect to bootstrap peer {}: {}", peer, e),
            }
        }

        Ok(node)
    }
}
