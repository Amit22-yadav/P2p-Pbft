use crate::crypto::{verify_signature, KeyPair};
use crate::message::{
    Checkpoint, Commit, ConsensusMessage, Hash, NetworkMessage, NewView, NodeId, PrePrepare,
    Prepare, Request, SequenceNumber, ViewChange, ViewNumber,
};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Convert a hex-encoded node ID to public key bytes
fn node_id_to_public_key(node_id: &str) -> Option<Vec<u8>> {
    hex::decode(node_id).ok()
}

/// Verify a signature from a node
fn verify_node_signature(node_id: &str, message: &[u8], signature: &[u8]) -> bool {
    match node_id_to_public_key(node_id) {
        Some(pk) => verify_signature(&pk, message, signature).is_ok(),
        None => false,
    }
}

#[derive(Error, Debug)]
pub enum PbftError {
    #[error("Invalid message")]
    InvalidMessage,
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Invalid view")]
    InvalidView,
    #[error("Invalid sequence number")]
    InvalidSequence,
    #[error("Not the primary")]
    NotPrimary,
    #[error("Duplicate message")]
    DuplicateMessage,
}

/// Outgoing message wrapper that supports both broadcast and targeted sends
#[derive(Debug, Clone)]
pub enum OutgoingMessage {
    /// Broadcast to all peers
    Broadcast(NetworkMessage),
    /// Send to a specific peer
    SendTo { target: NodeId, message: NetworkMessage },
}

/// PBFT consensus phase
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Idle,
    PrePrepared,
    Prepared,
    Committed,
}

/// State for a single consensus instance
#[derive(Debug, Clone)]
pub struct ConsensusInstance {
    pub sequence: SequenceNumber,
    pub view: ViewNumber,
    pub request: Option<Request>,
    pub digest: Option<Hash>,
    pub phase: Phase,
    pub pre_prepare: Option<PrePrepare>,
    pub prepares: HashMap<NodeId, Prepare>,
    pub commits: HashMap<NodeId, Commit>,
    pub started_at: Instant,
}

impl ConsensusInstance {
    pub fn new(sequence: SequenceNumber, view: ViewNumber) -> Self {
        Self {
            sequence,
            view,
            request: None,
            digest: None,
            phase: Phase::Idle,
            pre_prepare: None,
            prepares: HashMap::new(),
            commits: HashMap::new(),
            started_at: Instant::now(),
        }
    }
}

/// Configuration for PBFT
#[derive(Debug, Clone)]
pub struct PbftConfig {
    /// Total number of nodes in the network
    pub n: usize,
    /// Maximum faulty nodes (f = (n-1)/3)
    pub f: usize,
    /// Timeout for view change
    pub view_change_timeout: Duration,
    /// Checkpoint interval
    pub checkpoint_interval: u64,
}

impl PbftConfig {
    pub fn new(n: usize) -> Self {
        let f = (n - 1) / 3;
        Self {
            n,
            f,
            view_change_timeout: Duration::from_secs(10),
            checkpoint_interval: 100,
        }
    }

    /// Quorum size (2f + 1)
    pub fn quorum(&self) -> usize {
        2 * self.f + 1
    }
}

/// PBFT Replica state
pub struct PbftState {
    /// Our node ID
    pub node_id: NodeId,
    /// Our keypair for signing
    pub keypair: KeyPair,
    /// Current view number
    pub view: ViewNumber,
    /// Current sequence number (for primary)
    pub sequence: SequenceNumber,
    /// Low water mark (last stable checkpoint)
    pub low_watermark: SequenceNumber,
    /// High water mark (low_watermark + checkpoint_interval * 2)
    pub high_watermark: SequenceNumber,
    /// Configuration
    pub config: PbftConfig,
    /// List of all replica IDs in order
    pub replicas: Vec<NodeId>,
    /// Consensus instances
    pub log: HashMap<SequenceNumber, ConsensusInstance>,
    /// Executed requests
    pub executed: HashSet<Hash>,
    /// Checkpoints
    pub checkpoints: HashMap<SequenceNumber, HashMap<NodeId, Checkpoint>>,
    /// Stable checkpoint
    pub stable_checkpoint: SequenceNumber,
    /// View change messages received
    pub view_changes: HashMap<ViewNumber, HashMap<NodeId, ViewChange>>,
    /// Are we in view change mode?
    pub view_changing: bool,
    /// Last time we received a message from primary
    pub last_primary_activity: Instant,
}

impl PbftState {
    pub fn new(node_id: NodeId, keypair: KeyPair, replicas: Vec<NodeId>, config: PbftConfig) -> Self {
        Self {
            node_id,
            keypair,
            view: 0,
            sequence: 0,
            low_watermark: 0,
            high_watermark: config.checkpoint_interval * 2,
            config,
            replicas,
            log: HashMap::new(),
            executed: HashSet::new(),
            checkpoints: HashMap::new(),
            stable_checkpoint: 0,
            view_changes: HashMap::new(),
            view_changing: false,
            last_primary_activity: Instant::now(),
        }
    }

    /// Get the primary for the current view
    pub fn primary(&self) -> &NodeId {
        let primary_idx = (self.view as usize) % self.replicas.len();
        &self.replicas[primary_idx]
    }

    /// Check if we are the primary
    pub fn is_primary(&self) -> bool {
        self.primary() == &self.node_id
    }

    /// Get or create a consensus instance
    pub fn get_or_create_instance(&mut self, sequence: SequenceNumber) -> &mut ConsensusInstance {
        let view = self.view;
        self.log
            .entry(sequence)
            .or_insert_with(|| ConsensusInstance::new(sequence, view))
    }

    /// Sign data
    pub fn sign(&self, data: &[u8]) -> Vec<u8> {
        self.keypair.sign(data)
    }
}

/// PBFT consensus engine
pub struct PbftConsensus {
    state: Arc<RwLock<PbftState>>,
    outgoing_tx: mpsc::Sender<OutgoingMessage>,
    committed_tx: mpsc::Sender<Request>,
}

impl PbftConsensus {
    pub fn new(
        state: PbftState,
        outgoing_tx: mpsc::Sender<OutgoingMessage>,
        committed_tx: mpsc::Sender<Request>,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(state)),
            outgoing_tx,
            committed_tx,
        }
    }

    /// Get the state for reading
    pub fn state(&self) -> &Arc<RwLock<PbftState>> {
        &self.state
    }

    /// Handle a new client request (as primary)
    pub async fn handle_request(&self, request: Request) -> Result<(), PbftError> {
        let mut state = self.state.write();

        if !state.is_primary() {
            // Forward to primary only (not broadcast)
            let primary = state.primary().clone();
            drop(state);
            let msg = NetworkMessage::Consensus(ConsensusMessage::Request(request));
            let _ = self.outgoing_tx.send(OutgoingMessage::SendTo {
                target: primary,
                message: msg
            }).await;
            return Ok(());
        }

        // Check if already executed
        let digest = request.digest();
        if state.executed.contains(&digest) {
            return Ok(());
        }

        // Assign sequence number
        state.sequence += 1;
        let sequence = state.sequence;
        let view = state.view;

        // Check water marks
        if sequence > state.high_watermark {
            warn!("Sequence {} exceeds high watermark", sequence);
            return Err(PbftError::InvalidSequence);
        }

        info!(
            "Primary creating PrePrepare for sequence {} view {}",
            sequence, view
        );

        // Create pre-prepare message
        let sign_data = format!("{}-{}-{:?}", view, sequence, digest);
        let signature = state.sign(sign_data.as_bytes());

        let pre_prepare = PrePrepare {
            view,
            sequence,
            digest,
            request: request.clone(),
            primary_id: state.node_id.clone(),
            signature,
        };

        // Create our own prepare message (primary also participates in prepare phase)
        let prepare_sign_data = format!("{}-{}-{:?}", view, sequence, digest);
        let prepare_signature = state.sign(prepare_sign_data.as_bytes());
        let prepare = Prepare {
            view,
            sequence,
            digest,
            replica_id: state.node_id.clone(),
            signature: prepare_signature,
        };

        // Update local state
        let node_id = state.node_id.clone();
        let instance = state.get_or_create_instance(sequence);
        instance.request = Some(request);
        instance.digest = Some(digest);
        instance.pre_prepare = Some(pre_prepare.clone());
        instance.phase = Phase::PrePrepared;
        // Primary adds its own prepare
        instance.prepares.insert(node_id, prepare.clone());

        // Broadcast pre-prepare
        drop(state);
        let msg = NetworkMessage::Consensus(ConsensusMessage::PrePrepare(pre_prepare));
        let _ = self.outgoing_tx.send(OutgoingMessage::Broadcast(msg)).await;

        // Also broadcast our prepare message
        let prepare_msg = NetworkMessage::Consensus(ConsensusMessage::Prepare(prepare));
        let _ = self.outgoing_tx.send(OutgoingMessage::Broadcast(prepare_msg)).await;

        // Try to advance (in case we already have enough prepares)
        self.try_advance_to_prepared(sequence).await?;

        Ok(())
    }

    /// Handle a pre-prepare message (as backup)
    pub async fn handle_pre_prepare(&self, pre_prepare: PrePrepare) -> Result<(), PbftError> {
        let prepare = {
            let mut state = self.state.write();

            // Validate view
            if pre_prepare.view != state.view {
                return Err(PbftError::InvalidView);
            }

            // Validate primary
            if &pre_prepare.primary_id != state.primary() {
                return Err(PbftError::NotPrimary);
            }

            // Validate sequence
            if pre_prepare.sequence <= state.low_watermark
                || pre_prepare.sequence > state.high_watermark
            {
                return Err(PbftError::InvalidSequence);
            }

            // Verify signature
            let sign_data = format!(
                "{}-{}-{:?}",
                pre_prepare.view, pre_prepare.sequence, pre_prepare.digest
            );
            if !verify_node_signature(
                &pre_prepare.primary_id,
                sign_data.as_bytes(),
                &pre_prepare.signature,
            ) {
                warn!("Invalid signature on PrePrepare from {}", pre_prepare.primary_id);
                return Err(PbftError::InvalidSignature);
            }

            // Check for conflicting pre-prepare
            if let Some(instance) = state.log.get(&pre_prepare.sequence) {
                if instance.pre_prepare.is_some() {
                    if instance.digest != Some(pre_prepare.digest) {
                        return Err(PbftError::InvalidMessage);
                    }
                    return Ok(()); // Already have this pre-prepare
                }
            }

            // Verify digest
            let computed_digest = pre_prepare.request.digest();
            if computed_digest != pre_prepare.digest {
                return Err(PbftError::InvalidMessage);
            }

            info!(
                "Received PrePrepare for sequence {} view {}",
                pre_prepare.sequence, pre_prepare.view
            );

            // Extract values before borrowing mutably
            let sequence = pre_prepare.sequence;
            let view = pre_prepare.view;
            let digest = pre_prepare.digest;
            let node_id = state.node_id.clone();

            // Create prepare message before mutating instance
            let sign_data = format!("{}-{}-{:?}", view, sequence, digest);
            let signature = state.sign(sign_data.as_bytes());

            let prepare = Prepare {
                view,
                sequence,
                digest,
                replica_id: node_id.clone(),
                signature,
            };

            // Now update instance
            let instance = state.get_or_create_instance(sequence);
            instance.request = Some(pre_prepare.request.clone());
            instance.digest = Some(digest);
            instance.pre_prepare = Some(pre_prepare);
            instance.phase = Phase::PrePrepared;
            instance.prepares.insert(node_id, prepare.clone());

            state.last_primary_activity = Instant::now();

            prepare
        };

        let msg = NetworkMessage::Consensus(ConsensusMessage::Prepare(prepare.clone()));
        let _ = self.outgoing_tx.send(OutgoingMessage::Broadcast(msg)).await;

        // Check if we have enough prepares
        self.try_advance_to_prepared(prepare.sequence).await?;

        Ok(())
    }

    /// Handle a prepare message
    pub async fn handle_prepare(&self, prepare: Prepare) -> Result<(), PbftError> {
        let sequence = {
            let mut state = self.state.write();

            // Validate view
            if prepare.view != state.view {
                return Err(PbftError::InvalidView);
            }

            // Validate sequence
            if prepare.sequence <= state.low_watermark || prepare.sequence > state.high_watermark {
                return Err(PbftError::InvalidSequence);
            }

            // Verify signature
            let sign_data = format!(
                "{}-{}-{:?}",
                prepare.view, prepare.sequence, prepare.digest
            );
            if !verify_node_signature(
                &prepare.replica_id,
                sign_data.as_bytes(),
                &prepare.signature,
            ) {
                warn!("Invalid signature on Prepare from {}", prepare.replica_id);
                return Err(PbftError::InvalidSignature);
            }

            let sequence = prepare.sequence;
            let instance = state.get_or_create_instance(sequence);

            // Check for matching pre-prepare
            if let Some(ref pp) = instance.pre_prepare {
                if pp.digest != prepare.digest {
                    return Err(PbftError::InvalidMessage);
                }
            }

            // Check for duplicate
            if instance.prepares.contains_key(&prepare.replica_id) {
                return Ok(());
            }

            debug!(
                "Received Prepare for sequence {} from {}",
                prepare.sequence, prepare.replica_id
            );

            // Add prepare
            instance.prepares.insert(prepare.replica_id.clone(), prepare);

            sequence
        };

        // Check if we have enough prepares
        self.try_advance_to_prepared(sequence).await?;

        Ok(())
    }

    /// Try to advance to prepared state
    async fn try_advance_to_prepared(&self, sequence: SequenceNumber) -> Result<(), PbftError> {
        let commit = {
            let mut state = self.state.write();
            let quorum = state.config.quorum();
            let node_id = state.node_id.clone();

            let instance = match state.log.get(&sequence) {
                Some(i) => i,
                None => return Ok(()),
            };

            // Need pre-prepare and quorum of prepares
            if instance.pre_prepare.is_none() {
                return Ok(());
            }

            if instance.phase != Phase::PrePrepared {
                return Ok(());
            }

            if instance.prepares.len() < quorum {
                return Ok(());
            }

            info!("Prepared for sequence {}", sequence);

            // Extract values we need
            let view = instance.view;
            let digest = instance.digest.unwrap();

            // Create commit message
            let sign_data = format!("{}-{}-{:?}-commit", view, sequence, digest);
            let signature = state.sign(sign_data.as_bytes());

            let commit = Commit {
                view,
                sequence,
                digest,
                replica_id: node_id.clone(),
                signature,
            };

            // Now get mutable reference and update
            let instance = state.log.get_mut(&sequence).unwrap();
            instance.phase = Phase::Prepared;
            instance.commits.insert(node_id, commit.clone());

            commit
        };

        let msg = NetworkMessage::Consensus(ConsensusMessage::Commit(commit));
        let _ = self.outgoing_tx.send(OutgoingMessage::Broadcast(msg)).await;

        // Check if we have enough commits
        self.try_advance_to_committed(sequence).await?;

        Ok(())
    }

    /// Handle a commit message
    pub async fn handle_commit(&self, commit: Commit) -> Result<(), PbftError> {
        let sequence = {
            let mut state = self.state.write();

            // Validate view
            if commit.view != state.view {
                return Err(PbftError::InvalidView);
            }

            // Validate sequence
            if commit.sequence <= state.low_watermark || commit.sequence > state.high_watermark {
                return Err(PbftError::InvalidSequence);
            }

            // Verify signature
            let sign_data = format!(
                "{}-{}-{:?}-commit",
                commit.view, commit.sequence, commit.digest
            );
            if !verify_node_signature(
                &commit.replica_id,
                sign_data.as_bytes(),
                &commit.signature,
            ) {
                warn!("Invalid signature on Commit from {}", commit.replica_id);
                return Err(PbftError::InvalidSignature);
            }

            let sequence = commit.sequence;
            let instance = state.get_or_create_instance(sequence);

            // Validate digest matches the instance's digest (if we have one)
            if let Some(instance_digest) = instance.digest {
                if commit.digest != instance_digest {
                    warn!(
                        "Commit digest mismatch from {}: expected {:?}, got {:?}",
                        commit.replica_id, instance_digest, commit.digest
                    );
                    return Err(PbftError::InvalidMessage);
                }
            }

            // Check for duplicate
            if instance.commits.contains_key(&commit.replica_id) {
                return Ok(());
            }

            info!(
                "Received Commit for sequence {} from {}, total commits: {}",
                commit.sequence, commit.replica_id, instance.commits.len() + 1
            );

            // Add commit
            instance.commits.insert(commit.replica_id.clone(), commit);

            sequence
        };

        // Check if we have enough commits
        self.try_advance_to_committed(sequence).await?;

        Ok(())
    }

    /// Try to advance to committed state
    async fn try_advance_to_committed(&self, sequence: SequenceNumber) -> Result<(), PbftError> {
        let request_to_commit = {
            let mut state = self.state.write();
            let quorum = state.config.quorum();

            let instance = match state.log.get(&sequence) {
                Some(i) => i,
                None => return Ok(()),
            };

            // Need prepared state and quorum of commits
            if instance.phase != Phase::Prepared {
                debug!(
                    "try_advance_to_committed: phase is {:?}, not Prepared",
                    instance.phase
                );
                return Ok(());
            }

            if instance.commits.len() < quorum {
                debug!(
                    "try_advance_to_committed: commits {} < quorum {}",
                    instance.commits.len(), quorum
                );
                return Ok(());
            }

            info!("Committed for sequence {}", sequence);

            // Check if we should execute
            let request_to_commit = if let Some(ref request) = instance.request {
                let digest = request.digest();
                if !state.executed.contains(&digest) {
                    Some((request.clone(), digest))
                } else {
                    None
                }
            } else {
                None
            };

            // Update phase
            let instance = state.log.get_mut(&sequence).unwrap();
            instance.phase = Phase::Committed;

            // Insert into executed if we have a request
            if let Some((_, digest)) = &request_to_commit {
                state.executed.insert(*digest);
            }

            request_to_commit
        };

        // Execute the request outside the lock
        if let Some((request, _)) = request_to_commit {
            let _ = self.committed_tx.send(request).await;

            // Check for checkpoint
            self.try_checkpoint(sequence).await?;
        }

        Ok(())
    }

    /// Try to create a checkpoint
    async fn try_checkpoint(&self, sequence: SequenceNumber) -> Result<(), PbftError> {
        let checkpoint = {
            let state = self.state.read();
            let interval = state.config.checkpoint_interval;

            if sequence % interval != 0 {
                return Ok(());
            }

            // Create checkpoint
            let digest = [0u8; 32]; // In practice, this would be state hash
            let sign_data = format!("{}-{:?}", sequence, digest);
            let signature = state.sign(sign_data.as_bytes());

            Checkpoint {
                sequence,
                digest,
                replica_id: state.node_id.clone(),
                signature,
            }
        };

        let msg = NetworkMessage::Consensus(ConsensusMessage::Checkpoint(checkpoint));
        let _ = self.outgoing_tx.send(OutgoingMessage::Broadcast(msg)).await;

        Ok(())
    }

    /// Handle a checkpoint message
    pub async fn handle_checkpoint(&self, checkpoint: Checkpoint) -> Result<(), PbftError> {
        let mut state = self.state.write();

        let checkpoint_seq = checkpoint.sequence;
        let checkpoint_replica = checkpoint.replica_id.clone();

        state
            .checkpoints
            .entry(checkpoint_seq)
            .or_insert_with(HashMap::new)
            .insert(checkpoint_replica, checkpoint.clone());

        // Check if we have enough checkpoints for stability
        let quorum = state.config.quorum();
        let entry_len = state.checkpoints.get(&checkpoint_seq).map(|e| e.len()).unwrap_or(0);

        if entry_len >= quorum && checkpoint_seq > state.stable_checkpoint {
            info!("Stable checkpoint at sequence {}", checkpoint_seq);
            state.stable_checkpoint = checkpoint_seq;
            state.low_watermark = checkpoint_seq;
            state.high_watermark = checkpoint_seq + state.config.checkpoint_interval * 2;

            // Garbage collect old log entries
            let low_watermark = state.low_watermark;
            let stable_checkpoint = state.stable_checkpoint;
            state.log.retain(|seq, _| *seq > low_watermark);
            state.checkpoints.retain(|seq, _| *seq >= stable_checkpoint);
        }

        Ok(())
    }

    /// Handle view change message
    pub async fn handle_view_change(&self, view_change: ViewChange) -> Result<(), PbftError> {
        let new_view_msg = {
            let mut state = self.state.write();

            if view_change.new_view <= state.view {
                return Ok(());
            }

            info!(
                "Received ViewChange for view {} from {}",
                view_change.new_view, view_change.replica_id
            );

            let new_view = view_change.new_view;
            let vc_replica_id = view_change.replica_id.clone();

            state
                .view_changes
                .entry(new_view)
                .or_insert_with(HashMap::new)
                .insert(vc_replica_id, view_change);

            // Check if we have enough view changes
            let quorum = state.config.quorum();
            let entry_len = state.view_changes.get(&new_view).map(|e| e.len()).unwrap_or(0);

            if entry_len >= quorum {
                let replicas_len = state.replicas.len();
                let primary_idx = (new_view as usize) % replicas_len;
                let is_new_primary = state.replicas[primary_idx] == state.node_id;

                // If we are the new primary, send NewView
                if is_new_primary {
                    info!("We are new primary for view {}", new_view);

                    let view_changes: Vec<ViewChange> = state
                        .view_changes
                        .get(&new_view)
                        .map(|e| e.values().cloned().collect())
                        .unwrap_or_default();

                    let sign_data = format!("{}-new-view", new_view);
                    let signature = state.sign(sign_data.as_bytes());

                    Some(NewView {
                        new_view,
                        view_changes,
                        primary_id: state.node_id.clone(),
                        signature,
                    })
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(new_view_msg) = new_view_msg {
            let msg = NetworkMessage::Consensus(ConsensusMessage::NewView(new_view_msg));
            let _ = self.outgoing_tx.send(OutgoingMessage::Broadcast(msg)).await;
        }

        Ok(())
    }

    /// Handle new view message
    pub async fn handle_new_view(&self, new_view: NewView) -> Result<(), PbftError> {
        let mut state = self.state.write();

        if new_view.new_view <= state.view {
            return Ok(());
        }

        info!("Received NewView for view {}", new_view.new_view);

        // Update to new view
        state.view = new_view.new_view;
        state.view_changing = false;
        state.last_primary_activity = Instant::now();

        // Clear view change state
        let current_view = state.view;
        state.view_changes.retain(|v, _| *v > current_view);

        Ok(())
    }

    /// Start a view change
    pub async fn start_view_change(&self) {
        let view_change = {
            let mut state = self.state.write();

            if state.view_changing {
                return;
            }

            let new_view = state.view + 1;
            info!("Starting view change to view {}", new_view);
            state.view_changing = true;

            let sign_data = format!("{}-view-change", new_view);
            let signature = state.sign(sign_data.as_bytes());
            let node_id = state.node_id.clone();
            let last_sequence = state.sequence;

            let view_change = ViewChange {
                new_view,
                replica_id: node_id.clone(),
                last_sequence,
                signature,
            };

            // Add our own view change
            state
                .view_changes
                .entry(new_view)
                .or_insert_with(HashMap::new)
                .insert(node_id, view_change.clone());

            view_change
        };

        let msg = NetworkMessage::Consensus(ConsensusMessage::ViewChange(view_change));
        let _ = self.outgoing_tx.send(OutgoingMessage::Broadcast(msg)).await;
    }

    /// Check view change timeout
    pub async fn check_timeout(&self) {
        let state = self.state.read();

        if state.view_changing {
            return;
        }

        if !state.is_primary() {
            if state.last_primary_activity.elapsed() > state.config.view_change_timeout {
                drop(state);
                self.start_view_change().await;
            }
        }
    }

    /// Process an incoming consensus message
    pub async fn process_message(&self, msg: ConsensusMessage) -> Result<(), PbftError> {
        match msg {
            ConsensusMessage::Request(req) => self.handle_request(req).await,
            ConsensusMessage::PrePrepare(pp) => self.handle_pre_prepare(pp).await,
            ConsensusMessage::Prepare(p) => self.handle_prepare(p).await,
            ConsensusMessage::Commit(c) => self.handle_commit(c).await,
            ConsensusMessage::Reply(_) => Ok(()), // Replies go to clients
            ConsensusMessage::ViewChange(vc) => self.handle_view_change(vc).await,
            ConsensusMessage::NewView(nv) => self.handle_new_view(nv).await,
            ConsensusMessage::Checkpoint(cp) => self.handle_checkpoint(cp).await,
        }
    }
}
