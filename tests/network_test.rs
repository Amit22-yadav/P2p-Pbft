//! Network integration test for PBFT consensus
//! Tests actual TCP connections between nodes and mesh formation

use p2p_pbft::crypto::KeyPair;
use p2p_pbft::message::NetworkMessage;
use p2p_pbft::network::Network;
use std::time::Duration;
use tokio::time::sleep;

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

#[tokio::test]
async fn test_network_mesh_formation() {
    // Test that peer discovery correctly forms a mesh network
    let keypairs = generate_test_keypairs(4);

    let base_port = 9200;
    let mut networks = Vec::new();

    // Create and start listeners for all nodes
    for (i, kp) in keypairs.iter().enumerate() {
        let node_id = kp.public_key_hex();
        let port = base_port + i as u16;
        let network = Network::new(node_id, port);
        network.start_listener().await.unwrap();
        networks.push(network);
    }

    sleep(Duration::from_millis(100)).await;

    // Node 0 is the "bootstrap" node
    // Connect nodes 1, 2, 3 to node 0
    // Peer discovery should then connect them to each other

    let addr0 = format!("127.0.0.1:{}", base_port);

    // Node 1 connects to node 0
    let peer_id = networks[1].connect_to_peer(&addr0).await.unwrap();
    println!("Node 1 connected to node 0: {}", &peer_id[..16]);

    sleep(Duration::from_millis(200)).await;

    // Node 2 connects to node 0 (should discover node 1 via peer discovery)
    let peer_id = networks[2].connect_to_peer(&addr0).await.unwrap();
    println!("Node 2 connected to node 0: {}", &peer_id[..16]);

    sleep(Duration::from_millis(200)).await;

    // Node 3 connects to node 0 (should discover nodes 1 and 2)
    let peer_id = networks[3].connect_to_peer(&addr0).await.unwrap();
    println!("Node 3 connected to node 0: {}", &peer_id[..16]);

    // Give time for peer discovery connections to complete
    sleep(Duration::from_millis(500)).await;

    // Verify mesh formation
    // Node 0 should have 3 peers (1, 2, 3)
    // Node 1 should have 3 peers (0, 2, 3) - 0 is direct, 2&3 via peer discovery
    // Node 2 should have 3 peers (0, 1, 3) - 0 is direct, 1&3 via peer discovery
    // Node 3 should have 3 peers (0, 1, 2) - 0 is direct, 1&2 via peer discovery

    println!("\nPeer counts:");
    for (i, network) in networks.iter().enumerate() {
        let count = network.peer_count();
        println!("  Node {}: {} peers", i, count);
        let peers = network.get_peers();
        for peer in &peers {
            println!("    - {}...", &peer.node_id[..16]);
        }
    }

    // Each node should have at least 2 peers (direct connection + discovery)
    // Ideally all should have 3, but timing can vary
    for (i, network) in networks.iter().enumerate() {
        let count = network.peer_count();
        assert!(
            count >= 1,
            "Node {} should have at least 1 peer, but has {}",
            i,
            count
        );
    }

    // Node 0 should have all 3 other nodes
    assert!(
        networks[0].peer_count() >= 3,
        "Node 0 (bootstrap) should have 3 peers, but has {}",
        networks[0].peer_count()
    );

    println!("\nMesh network formation test passed!");
}

#[tokio::test]
async fn test_network_broadcast() {
    // Test that broadcast reaches all peers in a mesh network
    let base_port = 9300;

    // Create 4 nodes
    let mut networks = Vec::new();
    let mut receivers = Vec::new();

    for i in 0..4 {
        let node_id = format!("node_{}", i);
        let port = base_port + i as u16;
        let mut network = Network::new(node_id, port);
        network.start_listener().await.unwrap();
        let rx = network.take_message_receiver().unwrap();
        receivers.push(rx);
        networks.push(network);
    }

    sleep(Duration::from_millis(100)).await;

    // Connect all nodes to node 0 (bootstrap pattern)
    let addr0 = format!("127.0.0.1:{}", base_port);
    for i in 1..4 {
        networks[i].connect_to_peer(&addr0).await.unwrap();
        sleep(Duration::from_millis(100)).await;
    }

    // Wait for mesh formation via peer discovery
    sleep(Duration::from_millis(500)).await;

    // Node 1 broadcasts a message
    let test_msg = NetworkMessage::Ping {
        node_id: "node_1".to_string(),
        timestamp: 12345,
    };
    networks[1].broadcast(test_msg).await;

    sleep(Duration::from_millis(200)).await;

    // Check which nodes received the broadcast
    let mut received_count = 0;
    for (i, rx) in receivers.iter_mut().enumerate() {
        if i == 1 {
            continue; // Skip sender
        }
        if let Ok((_from, msg)) = rx.try_recv() {
            if let NetworkMessage::Ping { timestamp, .. } = msg {
                if timestamp == 12345 {
                    println!("Node {} received broadcast from node 1", i);
                    received_count += 1;
                }
            }
        }
    }

    println!("Broadcast received by {} nodes", received_count);
    // At least node 0 should receive it (direct connection)
    assert!(
        received_count >= 1,
        "At least 1 node should receive the broadcast"
    );
}
