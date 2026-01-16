use clap::{Parser, Subcommand};
use p2p_pbft::blockchain::{Blockchain, BlockchainConfig};
use p2p_pbft::crypto::KeyPair;
use p2p_pbft::message::{ConsensusMessage, NetworkMessage, Request};
use p2p_pbft::network::Network;
use p2p_pbft::pbft::{OutgoingMessage, PbftConfig, PbftConsensus, PbftState};
use p2p_pbft::rpc::RpcServer;
use p2p_pbft::types::{address_to_hex, public_key_to_address};
use std::io::{self, BufRead, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser)]
#[command(name = "p2p-pbft")]
#[command(about = "P2P Blockchain with PBFT Consensus", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a blockchain node (new blockchain mode)
    Blockchain {
        /// Port to listen on for P2P
        #[arg(short, long, default_value = "9000")]
        port: u16,

        /// JSON-RPC port
        #[arg(long, default_value = "8545")]
        rpc_port: u16,

        /// Data directory
        #[arg(short, long, default_value = "./data")]
        data_dir: PathBuf,

        /// Chain ID
        #[arg(long, default_value = "pbft-chain")]
        chain_id: String,

        /// Seed for deterministic key generation (hex string, 32 bytes)
        #[arg(short, long)]
        seed: Option<String>,

        /// Bootstrap peer addresses
        #[arg(short, long)]
        bootstrap: Vec<String>,

        /// Total number of validator nodes
        #[arg(short, long, default_value = "4")]
        nodes: usize,

        /// Node index (0-based) for deterministic setup
        #[arg(short, long)]
        index: Option<usize>,

        /// Initial balance for the node's address
        #[arg(long, default_value = "1000000")]
        initial_balance: u64,
    },

    /// Start a PBFT node (legacy mode)
    Start {
        /// Port to listen on
        #[arg(short, long, default_value = "8000")]
        port: u16,

        /// Seed for deterministic key generation (hex string, 32 bytes)
        #[arg(short, long)]
        seed: Option<String>,

        /// Bootstrap peer addresses (can be specified multiple times)
        #[arg(short, long)]
        bootstrap: Vec<String>,

        /// Total number of nodes in the network (for PBFT configuration)
        #[arg(short, long, default_value = "4")]
        nodes: usize,

        /// Node index (0-based) for deterministic setup
        #[arg(short, long)]
        index: Option<usize>,
    },

    /// Generate a new keypair
    Keygen {
        /// Optional seed (hex string, 32 bytes)
        #[arg(short, long)]
        seed: Option<String>,
    },

    /// Initialize a new blockchain (creates genesis block)
    Init {
        /// Data directory
        #[arg(short, long, default_value = "./data")]
        data_dir: PathBuf,

        /// Chain ID
        #[arg(long, default_value = "pbft-chain")]
        chain_id: String,

        /// Number of validators
        #[arg(short, long, default_value = "4")]
        nodes: usize,

        /// Initial balance per validator
        #[arg(long, default_value = "1000000")]
        initial_balance: u64,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .compact()
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let cli = Cli::parse();

    match cli.command {
        Commands::Blockchain {
            port,
            rpc_port,
            data_dir,
            chain_id,
            seed,
            bootstrap,
            nodes,
            index,
            initial_balance,
        } => {
            start_blockchain_node(
                port,
                rpc_port,
                data_dir,
                chain_id,
                seed,
                bootstrap,
                nodes,
                index,
                initial_balance,
            )
            .await?;
        }
        Commands::Start {
            port,
            seed,
            bootstrap,
            nodes,
            index,
        } => {
            start_node(port, seed, bootstrap, nodes, index).await?;
        }
        Commands::Keygen { seed } => {
            keygen(seed)?;
        }
        Commands::Init {
            data_dir,
            chain_id,
            nodes,
            initial_balance,
        } => {
            init_blockchain(data_dir, chain_id, nodes, initial_balance)?;
        }
    }

    Ok(())
}

async fn start_node(
    port: u16,
    seed: Option<String>,
    bootstrap: Vec<String>,
    num_nodes: usize,
    index: Option<usize>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Parse or generate seed
    let keypair = if let Some(seed_hex) = seed {
        let seed_bytes: [u8; 32] = hex::decode(&seed_hex)?
            .try_into()
            .map_err(|_| "Seed must be 32 bytes")?;
        KeyPair::from_seed(&seed_bytes)
    } else if let Some(idx) = index {
        // Generate deterministic seed from index
        let mut seed = [0u8; 32];
        seed[0] = idx as u8;
        KeyPair::from_seed(&seed)
    } else {
        KeyPair::generate()
    };

    let node_id = keypair.public_key_hex();
    info!("Starting node with ID: {}", node_id);
    info!("Listening on port: {}", port);

    // Create network
    let mut network = Network::new(node_id.clone(), port);

    // Start listening
    network.start_listener().await?;

    // Connect to bootstrap peers
    for peer_addr in &bootstrap {
        info!("Connecting to bootstrap peer: {}", peer_addr);
        match network.connect_to_peer(peer_addr).await {
            Ok(peer_id) => info!("Connected to peer: {}", peer_id),
            Err(e) => warn!("Failed to connect to {}: {}", peer_addr, e),
        }
    }

    // Generate replica IDs for deterministic setup
    let mut replicas: Vec<String> = Vec::new();
    if let Some(_idx) = index {
        for i in 0..num_nodes {
            let mut seed = [0u8; 32];
            seed[0] = i as u8;
            let kp = KeyPair::from_seed(&seed);
            replicas.push(kp.public_key_hex());
        }
        replicas.sort();
        info!("Replica set ({} nodes):", replicas.len());
        for (i, r) in replicas.iter().enumerate() {
            info!("  {}: {}...", i, &r[..16]);
        }
    } else {
        replicas.push(node_id.clone());
    }

    // Create PBFT consensus
    let config = PbftConfig::new(num_nodes);
    info!(
        "PBFT config: n={}, f={}, quorum={}",
        config.n,
        config.f,
        config.quorum()
    );

    let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<OutgoingMessage>(1000);
    let (committed_tx, mut committed_rx) = mpsc::channel(1000);

    let state = PbftState::new(node_id.clone(), keypair, replicas, config);
    let consensus = PbftConsensus::new(state, outgoing_tx, committed_tx);

    // Take message receiver
    let mut network_rx = network
        .take_message_receiver()
        .expect("Message receiver already taken");

    // Spawn outgoing message handler
    let network_clone = network.peers.clone();
    tokio::spawn(async move {
        while let Some(outgoing) = outgoing_rx.recv().await {
            match outgoing {
                OutgoingMessage::Broadcast(msg) => {
                    // Send to all peers
                    let peers: Vec<_> = network_clone.read().values().cloned().collect();
                    let msg_type = match &msg {
                        NetworkMessage::Consensus(ConsensusMessage::PrePrepare(_)) => "PrePrepare",
                        NetworkMessage::Consensus(ConsensusMessage::Prepare(_)) => "Prepare",
                        NetworkMessage::Consensus(ConsensusMessage::Commit(_)) => "Commit",
                        _ => "Other",
                    };
                    info!("Broadcasting {} to {} peers", msg_type, peers.len());
                    for peer in peers {
                        if let Err(e) = peer.send(msg.clone()).await {
                            warn!("Failed to send to {}: {}", peer.info.node_id, e);
                        }
                    }
                }
                OutgoingMessage::SendTo { target, message } => {
                    // Send to specific peer only
                    let peer = network_clone.read().get(&target).cloned();
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

    // Spawn committed request handler
    tokio::spawn(async move {
        while let Some(request) = committed_rx.recv().await {
            info!(
                "✓ Request COMMITTED: client={}, data={:?}",
                request.client_id,
                String::from_utf8_lossy(&request.operation)
            );
        }
    });

    // Create channel for CLI to submit requests
    let (cli_request_tx, mut cli_request_rx) = mpsc::channel::<Request>(100);

    // Spawn view change timeout checker
    let consensus_state = consensus.state().clone();
    let network_peers_for_status = network.peers.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            let state = consensus_state.read().await;
            let is_primary = state.is_primary();
            let view = state.view;
            let seq = state.sequence;
            let total_replicas = state.replicas.len();
            drop(state);

            let connected_peers = network_peers_for_status.read().len();
            let expected_peers = total_replicas.saturating_sub(1); // exclude self
            let connection_status = if connected_peers >= expected_peers {
                "✓ fully connected"
            } else {
                "⚠ partial"
            };

            info!(
                "Status: view={}, seq={}, primary={}, peers={}/{} ({})",
                view, seq, is_primary, connected_peers, expected_peers, connection_status
            );
        }
    });

    // Spawn CLI handler
    let consensus_for_cli = consensus.state().clone();
    let node_id_for_cli = node_id.clone();
    tokio::spawn(async move {
        let stdin = io::stdin();
        let mut stdout = io::stdout();

        loop {
            print!("> ");
            stdout.flush().unwrap();

            let mut line = String::new();
            if stdin.lock().read_line(&mut line).is_err() {
                break;
            }

            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.splitn(2, ' ').collect();
            match parts[0] {
                "submit" | "s" => {
                    if parts.len() < 2 {
                        println!("Usage: submit <data>");
                        continue;
                    }
                    let data = parts[1].as_bytes().to_vec();
                    let request = Request::new(node_id_for_cli.clone(), data);

                    // Check if primary (in a block to ensure lock is dropped)
                    let is_primary = {
                        let state = consensus_for_cli.read().await;
                        state.is_primary()
                    };

                    if is_primary {
                        println!("Submitting request as primary...");
                    } else {
                        println!("Forwarding request to primary...");
                    }

                    // Send request through channel to main loop
                    if let Err(e) = cli_request_tx.send(request).await {
                        println!("Failed to submit request: {}", e);
                    } else {
                        println!("Request submitted: {:?}", parts[1]);
                    }
                }
                "status" | "st" => {
                    let state = consensus_for_cli.read().await;
                    println!("═══════════════════════════════════════");
                    println!("  Node Status");
                    println!("═══════════════════════════════════════");
                    println!("  Node ID:     {}...", &state.node_id[..16]);
                    println!("  View:        {}", state.view);
                    println!("  Sequence:    {}", state.sequence);
                    println!("  Role:        {}", if state.is_primary() { "PRIMARY" } else { "BACKUP" });
                    println!("  Primary:     {}...", &state.primary()[..16]);
                    println!("───────────────────────────────────────");
                    println!("  Replicas:    {}", state.replicas.len());
                    println!("  Log entries: {}", state.log.len());
                    println!("  Executed:    {}", state.executed.len());
                    println!("═══════════════════════════════════════");
                }
                "peers" | "p" => {
                    let state = consensus_for_cli.read().await;
                    println!("Replicas ({}):", state.replicas.len());
                    for (i, r) in state.replicas.iter().enumerate() {
                        let marker = if r == &state.node_id { " (self)" } else { "" };
                        let primary = if r == state.primary() {
                            " [PRIMARY]"
                        } else {
                            ""
                        };
                        println!("  {}: {}...{}{}", i, &r[..16], marker, primary);
                    }
                }
                "help" | "h" => {
                    println!("Commands:");
                    println!("  submit <data>  - Submit a request");
                    println!("  status         - Show node status");
                    println!("  peers          - Show replica list");
                    println!("  help           - Show this help");
                    println!("  quit           - Exit");
                }
                "quit" | "q" | "exit" => {
                    println!("Goodbye!");
                    std::process::exit(0);
                }
                _ => {
                    println!("Unknown command. Type 'help' for available commands.");
                }
            }
        }
    });

    // Main message processing loop
    info!("Node started. Type 'help' for commands.");
    loop {
        tokio::select! {
            // Handle CLI requests
            Some(request) = cli_request_rx.recv() => {
                info!("Processing CLI request: {:?}", String::from_utf8_lossy(&request.operation));
                if let Err(e) = consensus.handle_request(request).await {
                    warn!("Error handling request: {}", e);
                }
            }

            // Handle network messages
            Some((peer_id, msg)) = network_rx.recv() => {
                match msg {
                    NetworkMessage::Consensus(consensus_msg) => {
                        let msg_type = match &consensus_msg {
                            ConsensusMessage::Request(_) => "Request",
                            ConsensusMessage::PrePrepare(_) => "PrePrepare",
                            ConsensusMessage::Prepare(_) => "Prepare",
                            ConsensusMessage::Commit(_) => "Commit",
                            ConsensusMessage::Reply(_) => "Reply",
                            ConsensusMessage::ViewChange(_) => "ViewChange",
                            ConsensusMessage::NewView(_) => "NewView",
                            ConsensusMessage::Checkpoint(_) => "Checkpoint",
                            ConsensusMessage::BlockPrePrepare(_) => "BlockPrePrepare",
                            ConsensusMessage::BlockPrepare(_) => "BlockPrepare",
                            ConsensusMessage::BlockCommit(_) => "BlockCommit",
                            ConsensusMessage::BlockCommitted(_) => "BlockCommitted",
                            ConsensusMessage::TransactionBroadcast(_) => "TransactionBroadcast",
                        };
                        info!("Received {} from {}...", msg_type, &peer_id[..16]);

                        if let Err(e) = consensus.process_message(consensus_msg).await {
                            warn!("Error processing message from {}: {}", &peer_id[..16], e);
                        }
                    }
                    NetworkMessage::Ping { timestamp, .. } => {
                        let pong = NetworkMessage::Pong {
                            node_id: node_id.clone(),
                            timestamp,
                        };
                        if let Err(e) = network.send_to_peer(&peer_id, pong).await {
                            warn!("Failed to send pong: {}", e);
                        }
                    }
                    NetworkMessage::Pong { .. } => {}
                    _ => {}
                }
            }

            // Exit if both channels are closed
            else => break,
        }
    }

    Ok(())
}

fn keygen(seed: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let keypair = if let Some(seed_hex) = seed {
        let seed_bytes: [u8; 32] = hex::decode(&seed_hex)?
            .try_into()
            .map_err(|_| "Seed must be 32 bytes")?;
        KeyPair::from_seed(&seed_bytes)
    } else {
        KeyPair::generate()
    };

    let address = public_key_to_address(&keypair.public_key_bytes());
    println!("Public Key (Node ID): {}", keypair.public_key_hex());
    println!("Address:              {}", address_to_hex(&address));
    println!("\nThis is your node's identity in the network.");

    Ok(())
}

// ==================== Blockchain Functions ====================

fn init_blockchain(
    data_dir: PathBuf,
    chain_id: String,
    num_nodes: usize,
    initial_balance: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Initializing blockchain...");
    println!("  Data dir: {:?}", data_dir);
    println!("  Chain ID: {}", chain_id);
    println!("  Validators: {}", num_nodes);

    // Generate validator keys and addresses
    let mut validators = Vec::new();
    let mut initial_accounts = Vec::new();

    for i in 0..num_nodes {
        let mut seed = [0u8; 32];
        seed[0] = i as u8;
        let kp = KeyPair::from_seed(&seed);
        let node_id = kp.public_key_hex();
        let address = public_key_to_address(&kp.public_key_bytes());

        validators.push(node_id.clone());
        initial_accounts.push((address, initial_balance));

        println!(
            "  Validator {}: {}... ({})",
            i,
            &node_id[..16],
            address_to_hex(&address)
        );
    }

    // Create blockchain config
    let config = BlockchainConfig::new(data_dir, chain_id)
        .with_validators(validators)
        .with_initial_accounts(initial_accounts);

    // Use first validator's key for initialization
    let mut seed = [0u8; 32];
    seed[0] = 0;
    let keypair = KeyPair::from_seed(&seed);

    // Create blockchain and initialize genesis
    let blockchain = Blockchain::new(config, keypair)?;
    match blockchain.init_genesis()? {
        Some(genesis) => {
            println!("\nGenesis block created!");
            println!("  Height: {}", genesis.height());
            println!("  Hash: 0x{}", hex::encode(&genesis.hash()[..8]));
        }
        None => {
            println!("\nGenesis block already exists.");
        }
    }

    println!("\nBlockchain initialized successfully!");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn start_blockchain_node(
    port: u16,
    rpc_port: u16,
    data_dir: PathBuf,
    chain_id: String,
    seed: Option<String>,
    bootstrap: Vec<String>,
    num_nodes: usize,
    index: Option<usize>,
    initial_balance: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    // Parse or generate keypair
    let keypair = if let Some(seed_hex) = seed {
        let seed_bytes: [u8; 32] = hex::decode(&seed_hex)?
            .try_into()
            .map_err(|_| "Seed must be 32 bytes")?;
        KeyPair::from_seed(&seed_bytes)
    } else if let Some(idx) = index {
        let mut seed = [0u8; 32];
        seed[0] = idx as u8;
        KeyPair::from_seed(&seed)
    } else {
        KeyPair::generate()
    };

    let node_id = keypair.public_key_hex();
    let address = public_key_to_address(&keypair.public_key_bytes());

    info!("Starting blockchain node");
    info!("  Node ID: {}...", &node_id[..16]);
    info!("  Address: {}", address_to_hex(&address));
    info!("  P2P Port: {}", port);
    info!("  RPC Port: {}", rpc_port);
    info!("  Data Dir: {:?}", data_dir);

    // Generate validator set
    let mut validators = Vec::new();
    let mut initial_accounts = Vec::new();

    if index.is_some() {
        for i in 0..num_nodes {
            let mut seed = [0u8; 32];
            seed[0] = i as u8;
            let kp = KeyPair::from_seed(&seed);
            validators.push(kp.public_key_hex());
            let addr = public_key_to_address(&kp.public_key_bytes());
            initial_accounts.push((addr, initial_balance));
        }
        validators.sort();
    } else {
        validators.push(node_id.clone());
        initial_accounts.push((address, initial_balance));
    }

    info!("Validators ({}):", validators.len());
    for (i, v) in validators.iter().enumerate() {
        info!("  {}: {}...", i, &v[..16]);
    }

    // Create blockchain
    let config = BlockchainConfig::new(data_dir, chain_id)
        .with_validators(validators.clone())
        .with_initial_accounts(initial_accounts)
        .with_block_interval(Duration::from_secs(5));

    let mut blockchain = Blockchain::new(config, keypair.clone())?;

    // Initialize genesis if needed
    if let Some(genesis) = blockchain.init_genesis()? {
        info!("Genesis block created: hash=0x{}", hex::encode(&genesis.hash()[..8]));
    }

    // Create network
    let mut network = Network::new(node_id.clone(), port);
    network.start_listener().await?;

    // Connect to bootstrap peers
    for peer_addr in &bootstrap {
        info!("Connecting to bootstrap peer: {}", peer_addr);
        match network.connect_to_peer(peer_addr).await {
            Ok(peer_id) => info!("Connected to peer: {}...", &peer_id[..16]),
            Err(e) => warn!("Failed to connect to {}: {}", peer_addr, e),
        }
    }

    // Initialize consensus
    let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<OutgoingMessage>(1000);
    let (committed_tx, _committed_rx) = mpsc::channel(1000);

    blockchain.init_consensus(validators.clone(), outgoing_tx, committed_tx);

    let blockchain = Arc::new(RwLock::new(blockchain));

    // Start RPC server
    let rpc_addr: SocketAddr = format!("0.0.0.0:{}", rpc_port).parse()?;
    let rpc_blockchain = blockchain.clone();
    tokio::spawn(async move {
        let server = RpcServer::new(rpc_blockchain, rpc_addr);
        server.start().await;
    });

    // Take receivers
    let mut network_rx = network
        .take_message_receiver()
        .expect("Message receiver already taken");

    // Spawn outgoing message handler
    let network_peers = network.peers.clone();
    tokio::spawn(async move {
        while let Some(outgoing) = outgoing_rx.recv().await {
            match outgoing {
                OutgoingMessage::Broadcast(msg) => {
                    let peers: Vec<_> = network_peers.read().values().cloned().collect();
                    for peer in peers {
                        let _ = peer.send(msg.clone()).await;
                    }
                }
                OutgoingMessage::SendTo { target, message } => {
                    let peer = network_peers.read().get(&target).cloned();
                    if let Some(peer) = peer {
                        let _ = peer.send(message).await;
                    }
                }
            }
        }
    });

    // Spawn block production loop (for primary)
    let blockchain_for_blocks = blockchain.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;

            let bc = blockchain_for_blocks.read().await;
            if bc.is_primary().await {
                match bc.propose_block().await {
                    Ok(Some(block)) => {
                        info!(
                            "Proposed block: height={}, txs={}",
                            block.height(),
                            block.transactions.len()
                        );
                    }
                    Ok(None) => {} // No transactions
                    Err(e) => warn!("Failed to propose block: {}", e),
                }
            }
        }
    });

    // Spawn committed block handler
    let blockchain_for_commits = blockchain.clone();
    tokio::spawn(async move {
        let mut bc = blockchain_for_commits.write().await;
        if let Some(mut block_rx) = bc.take_block_receiver() {
            drop(bc);
            while let Some(block) = block_rx.recv().await {
                let bc = blockchain_for_commits.read().await;
                if let Err(e) = bc.process_committed_block(block).await {
                    warn!("Failed to process committed block: {}", e);
                }
            }
        }
    });

    // Status display
    let blockchain_for_status = blockchain.clone();
    let network_peers_for_status = network.peers.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            let bc = blockchain_for_status.read().await;
            let height = bc.height();
            let is_primary = bc.is_primary().await;
            let mempool_size = bc.mempool_stats().total_transactions;
            drop(bc);

            let peers = network_peers_for_status.read().len();
            info!(
                "Status: height={}, primary={}, mempool={}, peers={}",
                height, is_primary, mempool_size, peers
            );
        }
    });

    info!("Blockchain node started!");
    info!("  JSON-RPC available at http://localhost:{}", rpc_port);

    // Main message processing loop
    loop {
        tokio::select! {
            Some((peer_id, msg)) = network_rx.recv() => {
                match msg {
                    NetworkMessage::Consensus(consensus_msg) => {
                        let bc = blockchain.read().await;
                        if let Some(consensus) = bc.consensus() {
                            if let Err(e) = consensus.process_message(consensus_msg).await {
                                warn!("Error processing message from {}...: {}", &peer_id[..16], e);
                            }
                        }
                    }
                    NetworkMessage::Ping { timestamp, .. } => {
                        let pong = NetworkMessage::Pong {
                            node_id: node_id.clone(),
                            timestamp,
                        };
                        let _ = network.send_to_peer(&peer_id, pong).await;
                    }
                    _ => {}
                }
            }
            else => break,
        }
    }

    Ok(())
}
