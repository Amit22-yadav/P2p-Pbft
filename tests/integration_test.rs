//! Integration test for PBFT consensus
//!
//! This test verifies that the PBFT consensus works correctly by:
//! 1. Creating 4 nodes with deterministic keys
//! 2. Connecting them together
//! 3. Submitting a request to the primary
//! 4. Verifying all nodes reach consensus

use p2p_pbft::crypto::KeyPair;
use p2p_pbft::message::{ConsensusMessage, NetworkMessage, Request};
use p2p_pbft::pbft::{OutgoingMessage, PbftConfig, PbftConsensus, PbftState};
use tokio::sync::mpsc;

/// Generate deterministic keypairs for testing
fn generate_test_keypairs(count: usize) -> Vec<KeyPair> {
    (0..count)
        .map(|i| {
            let mut seed = [0u8; 32];
            seed[0] = i as u8;
            KeyPair::from_seed(&seed)
        })
        .collect()
}

/// Get sorted replica IDs
fn get_replica_ids(keypairs: &[KeyPair]) -> Vec<String> {
    let mut ids: Vec<_> = keypairs.iter().map(|kp| kp.public_key_hex()).collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn test_pbft_consensus_basic() {
    // Create 4 keypairs
    let keypairs = generate_test_keypairs(4);
    let replica_ids = get_replica_ids(&keypairs);

    // Create channels for each node
    let mut outgoing_txs = Vec::new();
    let mut outgoing_rxs = Vec::new();
    let mut committed_txs = Vec::new();
    let mut committed_rxs = Vec::new();

    for _ in 0..4 {
        let (out_tx, out_rx) = mpsc::channel::<OutgoingMessage>(100);
        let (com_tx, com_rx) = mpsc::channel::<Request>(100);
        outgoing_txs.push(out_tx);
        outgoing_rxs.push(out_rx);
        committed_txs.push(com_tx);
        committed_rxs.push(com_rx);
    }

    // Create consensus instances for each node
    let config = PbftConfig::new(4);
    let mut consensus_nodes = Vec::new();

    for (i, kp) in keypairs.into_iter().enumerate() {
        let node_id = kp.public_key_hex();
        let state = PbftState::new(
            node_id,
            kp,
            replica_ids.clone(),
            config.clone(),
        );
        let consensus = PbftConsensus::new(
            state,
            outgoing_txs[i].clone(),
            committed_txs[i].clone(),
        );
        consensus_nodes.push(consensus);
    }

    // Find which node is the primary (view 0)
    let primary_idx = {
        let state = consensus_nodes[0].state().read().await;
        let primary_id = state.primary().clone();
        replica_ids.iter().position(|id| id == &primary_id).unwrap()
    };

    println!("Primary node index: {}", primary_idx);

    // Submit a request to the primary
    let request = Request::new("test_client".to_string(), b"hello consensus".to_vec());
    consensus_nodes[primary_idx]
        .handle_request(request.clone())
        .await
        .expect("Failed to handle request");

    // Collect outgoing messages from primary
    let mut messages_to_distribute: Vec<(usize, OutgoingMessage)> = Vec::new();

    // Get messages from the primary's outgoing channel
    while let Ok(msg) = outgoing_rxs[primary_idx].try_recv() {
        messages_to_distribute.push((primary_idx, msg));
    }

    println!("Primary sent {} messages", messages_to_distribute.len());

    // Distribute PrePrepare and Prepare from primary to all other nodes
    for (from_idx, outgoing) in messages_to_distribute {
        if let OutgoingMessage::Broadcast(msg) = outgoing {
            if let NetworkMessage::Consensus(consensus_msg) = msg {
                for (i, node) in consensus_nodes.iter().enumerate() {
                    if i != from_idx {
                        let result = node.process_message(consensus_msg.clone()).await;
                        if let Err(e) = result {
                            println!("Node {} error processing message: {:?}", i, e);
                        }
                    }
                }
            }
        }
    }

    // Collect and distribute Prepare messages from all nodes
    for round in 0..3 {
        println!("Round {} - collecting messages", round);
        let mut round_messages: Vec<(usize, NetworkMessage)> = Vec::new();

        for (i, rx) in outgoing_rxs.iter_mut().enumerate() {
            while let Ok(outgoing) = rx.try_recv() {
                if let OutgoingMessage::Broadcast(msg) = outgoing {
                    round_messages.push((i, msg));
                }
            }
        }

        println!("Collected {} messages in round {}", round_messages.len(), round);

        if round_messages.is_empty() {
            break;
        }

        // Distribute messages to all other nodes
        for (from_idx, msg) in round_messages {
            if let NetworkMessage::Consensus(consensus_msg) = msg {
                let msg_type = match &consensus_msg {
                    ConsensusMessage::PrePrepare(_) => "PrePrepare",
                    ConsensusMessage::Prepare(_) => "Prepare",
                    ConsensusMessage::Commit(_) => "Commit",
                    _ => "Other",
                };
                println!("Node {} broadcasts {}", from_idx, msg_type);

                for (i, node) in consensus_nodes.iter().enumerate() {
                    if i != from_idx {
                        let _ = node.process_message(consensus_msg.clone()).await;
                    }
                }
            }
        }
    }

    // Check how many nodes committed the request
    let mut committed_count = 0;
    for (i, rx) in committed_rxs.iter_mut().enumerate() {
        if let Ok(committed_request) = rx.try_recv() {
            println!("Node {} committed request: {:?}", i, String::from_utf8_lossy(&committed_request.operation));
            committed_count += 1;
        }
    }

    println!("Total nodes that committed: {}", committed_count);

    // At least 3 nodes (quorum) should have committed
    assert!(
        committed_count >= 3,
        "Expected at least 3 nodes to commit, but only {} did",
        committed_count
    );

    println!("PBFT consensus test passed!");
}

#[tokio::test]
async fn test_request_forwarding_to_primary() {
    // Create 4 keypairs
    let keypairs = generate_test_keypairs(4);
    let replica_ids = get_replica_ids(&keypairs);

    // Create channels
    let mut outgoing_txs = Vec::new();
    let mut outgoing_rxs = Vec::new();
    let mut committed_txs = Vec::new();
    let mut _committed_rxs = Vec::new();

    for _ in 0..4 {
        let (out_tx, out_rx) = mpsc::channel::<OutgoingMessage>(100);
        let (com_tx, com_rx) = mpsc::channel::<Request>(100);
        outgoing_txs.push(out_tx);
        outgoing_rxs.push(out_rx);
        committed_txs.push(com_tx);
        _committed_rxs.push(com_rx);
    }

    // Create consensus instances
    let config = PbftConfig::new(4);
    let mut consensus_nodes = Vec::new();

    for (i, kp) in keypairs.into_iter().enumerate() {
        let node_id = kp.public_key_hex();
        let state = PbftState::new(
            node_id,
            kp,
            replica_ids.clone(),
            config.clone(),
        );
        let consensus = PbftConsensus::new(
            state,
            outgoing_txs[i].clone(),
            committed_txs[i].clone(),
        );
        consensus_nodes.push(consensus);
    }

    // Find primary and a non-primary node
    let primary_id = {
        let state = consensus_nodes[0].state().read().await;
        state.primary().clone()
    };

    // Find a non-primary node
    let mut non_primary_idx = 0;
    for (i, node) in consensus_nodes.iter().enumerate() {
        let state = node.state().read().await;
        if state.node_id != primary_id {
            non_primary_idx = i;
            break;
        }
    }

    println!("Testing request forwarding from non-primary node {}", non_primary_idx);

    // Submit request to non-primary
    let request = Request::new("test_client".to_string(), b"forwarded request".to_vec());
    consensus_nodes[non_primary_idx]
        .handle_request(request)
        .await
        .expect("Failed to handle request");

    // Check that the non-primary sent a targeted message to the primary
    let outgoing = outgoing_rxs[non_primary_idx].try_recv();
    assert!(outgoing.is_ok(), "Non-primary should send a message");

    let msg = outgoing.unwrap();
    match msg {
        OutgoingMessage::SendTo { target, message: _ } => {
            assert_eq!(target, primary_id, "Request should be forwarded to primary");
            println!("Request correctly forwarded to primary: {}", &target[..16]);
        }
        OutgoingMessage::Broadcast(_) => {
            panic!("Non-primary should NOT broadcast, should forward to primary only");
        }
    }

    println!("Request forwarding test passed!");
}
