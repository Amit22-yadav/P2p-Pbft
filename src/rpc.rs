use crate::blockchain::Blockchain;
use crate::message::Hash;
use crate::types::{address_from_hex, address_to_hex, hash_to_hex, Transaction, TransactionPayload};
use parking_lot::RwLock as SyncRwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use warp::Filter;

/// Type alias for peer count provider
pub type PeerCountFn = Arc<SyncRwLock<HashMap<String, crate::network::PeerHandle>>>;

#[derive(Error, Debug)]
pub enum RpcError {
    #[error("Invalid params: {0}")]
    InvalidParams(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Method not found: {0}")]
    MethodNotFound(String),
    #[error("Parse error")]
    ParseError,
}

/// JSON-RPC request
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<serde_json::Value>,
    pub id: serde_json::Value,
}

/// JSON-RPC response
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: serde_json::Value,
}

/// JSON-RPC error
#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    pub fn error(id: serde_json::Value, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data: None,
            }),
            id,
        }
    }
}

// ==================== Request/Response Types ====================

#[derive(Debug, Deserialize)]
pub struct SendTransactionParams {
    pub from: String,      // Hex address
    pub to: Option<String>, // Hex address (None for state operations)
    pub value: Option<u64>,
    pub nonce: u64,
    pub data: Option<String>, // Hex encoded data
    pub signature: String,    // Hex encoded signature
    pub timestamp: Option<u64>, // Optional timestamp (if provided, uses this instead of current time)
}

#[derive(Debug, Serialize)]
pub struct TransactionResponse {
    pub hash: String,
    pub from: String,
    pub to: Option<String>,
    pub value: u64,
    pub nonce: u64,
    pub block_height: Option<u64>,
    pub tx_index: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct BlockResponse {
    pub height: u64,
    pub hash: String,
    pub prev_hash: String,
    pub timestamp: u64,
    pub tx_root: String,
    pub state_root: String,
    pub proposer: String,
    pub transaction_count: usize,
    pub transactions: Vec<String>, // Transaction hashes
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub node_id: String,
    pub address: String,
    pub chain_height: u64,
    pub is_primary: bool,
    pub mempool_size: usize,
    pub connected_peers: usize,
}

/// RPC Server
pub struct RpcServer {
    blockchain: Arc<RwLock<Blockchain>>,
    addr: SocketAddr,
    peers: Option<PeerCountFn>,
}

impl RpcServer {
    pub fn new(blockchain: Arc<RwLock<Blockchain>>, addr: SocketAddr) -> Self {
        Self {
            blockchain,
            addr,
            peers: None,
        }
    }

    /// Create RPC server with peer tracking
    pub fn with_peers(
        blockchain: Arc<RwLock<Blockchain>>,
        addr: SocketAddr,
        peers: PeerCountFn,
    ) -> Self {
        Self {
            blockchain,
            addr,
            peers: Some(peers),
        }
    }

    /// Start the RPC server
    pub async fn start(self) {
        let blockchain = self.blockchain.clone();
        let peers = self.peers.clone();

        // CORS configuration for frontend access
        let cors = warp::cors()
            .allow_any_origin()
            .allow_methods(vec!["POST", "OPTIONS"])
            .allow_headers(vec!["Content-Type"]);

        let rpc_route = warp::path("rpc")
            .or(warp::path::end())
            .unify()
            .and(warp::post())
            .and(warp::body::json())
            .and(warp::any().map(move || blockchain.clone()))
            .and(warp::any().map(move || peers.clone()))
            .and_then(handle_rpc_request)
            .with(cors);

        info!("Starting JSON-RPC server on {}", self.addr);

        warp::serve(rpc_route).run(self.addr).await;
    }
}

async fn handle_rpc_request(
    request: JsonRpcRequest,
    blockchain: Arc<RwLock<Blockchain>>,
    peers: Option<PeerCountFn>,
) -> Result<impl warp::Reply, warp::Rejection> {
    debug!("RPC request: method={}", request.method);

    let response = match request.method.as_str() {
        // Ethereum-compatible methods
        "eth_blockNumber" => handle_block_number(&blockchain).await,
        "eth_getBlockByNumber" => {
            handle_get_block_by_number(&blockchain, request.params.clone()).await
        }
        "eth_getBlockByHash" => handle_get_block_by_hash(&blockchain, request.params.clone()).await,
        "eth_getBalance" => handle_get_balance(&blockchain, request.params.clone()).await,
        "eth_getTransactionCount" => {
            handle_get_transaction_count(&blockchain, request.params.clone()).await
        }
        "eth_getTransactionByHash" => {
            handle_get_transaction_by_hash(&blockchain, request.params.clone()).await
        }
        "eth_sendRawTransaction" => {
            handle_send_raw_transaction(&blockchain, request.params.clone()).await
        }

        // Custom methods
        "pbft_getStatus" => handle_get_status(&blockchain, &peers).await,
        "pbft_getMempool" => handle_get_mempool(&blockchain).await,
        "chain_submitTransaction" => {
            handle_submit_transaction(&blockchain, request.params.clone()).await
        }
        "chain_getState" => handle_get_state(&blockchain, request.params.clone()).await,

        _ => Err(RpcError::MethodNotFound(request.method.clone())),
    };

    let json_response = match response {
        Ok(result) => JsonRpcResponse::success(request.id, result),
        Err(e) => {
            let (code, message) = match e {
                RpcError::InvalidParams(msg) => (-32602, msg),
                RpcError::Internal(msg) => (-32603, msg),
                RpcError::MethodNotFound(msg) => (-32601, format!("Method not found: {}", msg)),
                RpcError::ParseError => (-32700, "Parse error".to_string()),
            };
            JsonRpcResponse::error(request.id, code, message)
        }
    };

    Ok(warp::reply::json(&json_response))
}

// ==================== Handler Functions ====================

async fn handle_block_number(
    blockchain: &Arc<RwLock<Blockchain>>,
) -> Result<serde_json::Value, RpcError> {
    let bc = blockchain.read().await;
    let height = bc.height();
    Ok(serde_json::json!(format!("0x{:x}", height)))
}

async fn handle_get_block_by_number(
    blockchain: &Arc<RwLock<Blockchain>>,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, RpcError> {
    let params = params.ok_or_else(|| RpcError::InvalidParams("Missing params".to_string()))?;
    let params: Vec<serde_json::Value> =
        serde_json::from_value(params).map_err(|e| RpcError::InvalidParams(e.to_string()))?;

    let height_str = params
        .get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError::InvalidParams("Missing block number".to_string()))?;

    let height = if height_str == "latest" {
        blockchain.read().await.height()
    } else {
        let hex = height_str.strip_prefix("0x").unwrap_or(height_str);
        u64::from_str_radix(hex, 16).map_err(|e| RpcError::InvalidParams(e.to_string()))?
    };

    let bc = blockchain.read().await;
    match bc.get_block(height) {
        Ok(Some(block)) => Ok(block_to_json(&block)),
        Ok(None) => Ok(serde_json::Value::Null),
        Err(e) => Err(RpcError::Internal(e.to_string())),
    }
}

async fn handle_get_block_by_hash(
    blockchain: &Arc<RwLock<Blockchain>>,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, RpcError> {
    let params = params.ok_or_else(|| RpcError::InvalidParams("Missing params".to_string()))?;
    let params: Vec<serde_json::Value> =
        serde_json::from_value(params).map_err(|e| RpcError::InvalidParams(e.to_string()))?;

    let hash_str = params
        .get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError::InvalidParams("Missing block hash".to_string()))?;

    let hash = parse_hash(hash_str)?;

    let bc = blockchain.read().await;
    match bc.storage().get_block_by_hash(&hash) {
        Ok(Some(block)) => Ok(block_to_json(&block)),
        Ok(None) => Ok(serde_json::Value::Null),
        Err(e) => Err(RpcError::Internal(e.to_string())),
    }
}

async fn handle_get_balance(
    blockchain: &Arc<RwLock<Blockchain>>,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, RpcError> {
    let params = params.ok_or_else(|| RpcError::InvalidParams("Missing params".to_string()))?;
    let params: Vec<serde_json::Value> =
        serde_json::from_value(params).map_err(|e| RpcError::InvalidParams(e.to_string()))?;

    let addr_str = params
        .get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError::InvalidParams("Missing address".to_string()))?;

    let address = address_from_hex(addr_str).map_err(|e| RpcError::InvalidParams(e.to_string()))?;

    let bc = blockchain.read().await;
    match bc.get_balance(&address) {
        Ok(balance) => Ok(serde_json::json!(format!("0x{:x}", balance))),
        Err(e) => Err(RpcError::Internal(e.to_string())),
    }
}

async fn handle_get_transaction_count(
    blockchain: &Arc<RwLock<Blockchain>>,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, RpcError> {
    let params = params.ok_or_else(|| RpcError::InvalidParams("Missing params".to_string()))?;
    let params: Vec<serde_json::Value> =
        serde_json::from_value(params).map_err(|e| RpcError::InvalidParams(e.to_string()))?;

    let addr_str = params
        .get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError::InvalidParams("Missing address".to_string()))?;

    let address = address_from_hex(addr_str).map_err(|e| RpcError::InvalidParams(e.to_string()))?;

    let bc = blockchain.read().await;
    match bc.get_nonce(&address) {
        Ok(nonce) => Ok(serde_json::json!(format!("0x{:x}", nonce))),
        Err(e) => Err(RpcError::Internal(e.to_string())),
    }
}

async fn handle_get_transaction_by_hash(
    blockchain: &Arc<RwLock<Blockchain>>,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, RpcError> {
    let params = params.ok_or_else(|| RpcError::InvalidParams("Missing params".to_string()))?;
    let params: Vec<serde_json::Value> =
        serde_json::from_value(params).map_err(|e| RpcError::InvalidParams(e.to_string()))?;

    let hash_str = params
        .get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError::InvalidParams("Missing transaction hash".to_string()))?;

    let hash = parse_hash(hash_str)?;

    let bc = blockchain.read().await;
    match bc.get_transaction(&hash) {
        Ok(Some((tx, block_height, tx_index))) => Ok(tx_to_json(&tx, Some(block_height), Some(tx_index))),
        Ok(None) => Ok(serde_json::Value::Null),
        Err(e) => Err(RpcError::Internal(e.to_string())),
    }
}

async fn handle_send_raw_transaction(
    blockchain: &Arc<RwLock<Blockchain>>,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, RpcError> {
    let params = params.ok_or_else(|| RpcError::InvalidParams("Missing params".to_string()))?;
    let params: Vec<serde_json::Value> =
        serde_json::from_value(params).map_err(|e| RpcError::InvalidParams(e.to_string()))?;

    let tx_hex = params
        .get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError::InvalidParams("Missing raw transaction".to_string()))?;

    // Decode transaction
    let tx_bytes =
        hex::decode(tx_hex.strip_prefix("0x").unwrap_or(tx_hex)).map_err(|e| RpcError::InvalidParams(e.to_string()))?;

    let tx: Transaction =
        bincode::deserialize(&tx_bytes).map_err(|e| RpcError::InvalidParams(e.to_string()))?;

    let bc = blockchain.read().await;
    match bc.submit_transaction(tx).await {
        Ok(hash) => Ok(serde_json::json!(hash_to_hex(&hash))),
        Err(e) => Err(RpcError::Internal(e.to_string())),
    }
}

async fn handle_get_status(
    blockchain: &Arc<RwLock<Blockchain>>,
    peers: &Option<PeerCountFn>,
) -> Result<serde_json::Value, RpcError> {
    let bc = blockchain.read().await;
    let is_primary = bc.is_primary().await;

    let connected_peers = peers
        .as_ref()
        .map(|p| p.read().len())
        .unwrap_or(0);

    let status = StatusResponse {
        node_id: bc.node_id()[..16].to_string(),
        address: address_to_hex(bc.address()),
        chain_height: bc.height(),
        is_primary,
        mempool_size: bc.mempool_stats().total_transactions,
        connected_peers,
    };

    Ok(serde_json::to_value(status).unwrap())
}

async fn handle_get_mempool(
    blockchain: &Arc<RwLock<Blockchain>>,
) -> Result<serde_json::Value, RpcError> {
    let bc = blockchain.read().await;
    let stats = bc.mempool_stats();
    let transactions = bc.get_mempool_transactions();

    let tx_list: Vec<serde_json::Value> = transactions
        .iter()
        .map(|tx| {
            let (to, value) = match &tx.payload {
                TransactionPayload::Transfer { to, value } => (Some(address_to_hex(to)), *value),
                _ => (None, 0),
            };
            serde_json::json!({
                "hash": hash_to_hex(&tx.hash),
                "from": address_to_hex(&tx.from),
                "to": to,
                "value": value,
                "nonce": tx.nonce,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "total": stats.total_transactions,
        "transactions": tx_list,
        "unique_senders": stats.unique_senders,
        "max_size": stats.max_size,
    }))
}

async fn handle_submit_transaction(
    blockchain: &Arc<RwLock<Blockchain>>,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, RpcError> {
    let params = params.ok_or_else(|| RpcError::InvalidParams("Missing params".to_string()))?;
    let tx_params: SendTransactionParams =
        serde_json::from_value(params).map_err(|e| RpcError::InvalidParams(e.to_string()))?;

    // Parse addresses
    let from =
        address_from_hex(&tx_params.from).map_err(|e| RpcError::InvalidParams(e.to_string()))?;

    let payload = if let Some(to_str) = &tx_params.to {
        let to = address_from_hex(to_str).map_err(|e| RpcError::InvalidParams(e.to_string()))?;
        TransactionPayload::Transfer {
            to,
            value: tx_params.value.unwrap_or(0),
        }
    } else if let Some(data_hex) = &tx_params.data {
        let data = hex::decode(data_hex.strip_prefix("0x").unwrap_or(data_hex))
            .map_err(|e| RpcError::InvalidParams(e.to_string()))?;
        // Simple key-value format: first 32 bytes = key, rest = value
        if data.len() < 32 {
            return Err(RpcError::InvalidParams("Data too short for state operation".to_string()));
        }
        let key = data[..32].to_vec();
        let value = data[32..].to_vec();
        TransactionPayload::SetState { key, value }
    } else {
        return Err(RpcError::InvalidParams("Either 'to' or 'data' must be provided".to_string()));
    };

    // Parse signature
    let signature = hex::decode(tx_params.signature.strip_prefix("0x").unwrap_or(&tx_params.signature))
        .map_err(|e| RpcError::InvalidParams(e.to_string()))?;

    // Create transaction (use provided timestamp if available for proper signature verification)
    let mut tx = if let Some(timestamp) = tx_params.timestamp {
        Transaction::new_with_timestamp(from, tx_params.nonce, payload, timestamp)
    } else {
        Transaction::new(from, tx_params.nonce, payload)
    };
    tx.signature = signature;

    let bc = blockchain.read().await;
    match bc.submit_transaction(tx).await {
        Ok(hash) => Ok(serde_json::json!({ "hash": hash_to_hex(&hash) })),
        Err(e) => Err(RpcError::Internal(e.to_string())),
    }
}

async fn handle_get_state(
    blockchain: &Arc<RwLock<Blockchain>>,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, RpcError> {
    let params = params.ok_or_else(|| RpcError::InvalidParams("Missing params".to_string()))?;
    let params: Vec<serde_json::Value> =
        serde_json::from_value(params).map_err(|e| RpcError::InvalidParams(e.to_string()))?;

    let addr_str = params
        .get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError::InvalidParams("Missing address".to_string()))?;

    let key_str = params
        .get(1)
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError::InvalidParams("Missing key".to_string()))?;

    let address = address_from_hex(addr_str).map_err(|e| RpcError::InvalidParams(e.to_string()))?;
    let key = hex::decode(key_str.strip_prefix("0x").unwrap_or(key_str))
        .map_err(|e| RpcError::InvalidParams(e.to_string()))?;

    let bc = blockchain.read().await;
    match bc.get_state(&address, &key) {
        Ok(Some(value)) => Ok(serde_json::json!(format!("0x{}", hex::encode(value)))),
        Ok(None) => Ok(serde_json::Value::Null),
        Err(e) => Err(RpcError::Internal(e.to_string())),
    }
}

// ==================== Helper Functions ====================

fn parse_hash(s: &str) -> Result<Hash, RpcError> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s).map_err(|e| RpcError::InvalidParams(e.to_string()))?;
    if bytes.len() != 32 {
        return Err(RpcError::InvalidParams("Hash must be 32 bytes".to_string()));
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes);
    Ok(hash)
}

fn block_to_json(block: &crate::types::Block) -> serde_json::Value {
    serde_json::json!({
        "number": format!("0x{:x}", block.height()),
        "hash": hash_to_hex(&block.hash()),
        "parentHash": hash_to_hex(&block.header.prev_hash),
        "timestamp": format!("0x{:x}", block.header.timestamp),
        "transactionsRoot": hash_to_hex(&block.header.tx_root),
        "stateRoot": hash_to_hex(&block.header.state_root),
        "miner": block.header.proposer.clone(),
        "transactions": block.transactions.iter().map(|tx| hash_to_hex(&tx.hash)).collect::<Vec<_>>(),
        "size": format!("0x{:x}", bincode::serialize(block).map(|b| b.len()).unwrap_or(0)),
    })
}

fn tx_to_json(
    tx: &Transaction,
    block_height: Option<u64>,
    tx_index: Option<usize>,
) -> serde_json::Value {
    let (to, value) = match &tx.payload {
        TransactionPayload::Transfer { to, value } => (Some(address_to_hex(to)), *value),
        _ => (None, 0),
    };

    serde_json::json!({
        "hash": hash_to_hex(&tx.hash),
        "from": address_to_hex(&tx.from),
        "to": to,
        "value": format!("0x{:x}", value),
        "nonce": format!("0x{:x}", tx.nonce),
        "blockNumber": block_height.map(|h| format!("0x{:x}", h)),
        "transactionIndex": tx_index.map(|i| format!("0x{:x}", i)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hash() {
        let hash_str = "0x0000000000000000000000000000000000000000000000000000000000000001";
        let hash = parse_hash(hash_str).unwrap();
        assert_eq!(hash[31], 1);
    }

    #[test]
    fn test_json_rpc_response() {
        let response = JsonRpcResponse::success(
            serde_json::json!(1),
            serde_json::json!("0x1"),
        );
        assert!(response.error.is_none());
        assert!(response.result.is_some());
    }
}
