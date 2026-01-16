//! Wallet CLI for P2P PBFT Blockchain
//!
//! This tool provides account management and transaction signing capabilities.

use clap::{Parser, Subcommand};
use p2p_pbft::crypto::KeyPair;
use p2p_pbft::types::{address_to_hex, public_key_to_address, Transaction};
use serde::{Deserialize, Serialize};

#[derive(Parser)]
#[command(name = "wallet")]
#[command(about = "Wallet CLI for P2P PBFT Blockchain", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a new keypair
    New {
        /// Optional seed (hex string, 32 bytes)
        #[arg(short, long)]
        seed: Option<String>,
    },

    /// Show account info from seed
    Account {
        /// Seed (hex string, 32 bytes)
        #[arg(short, long)]
        seed: String,
    },

    /// Create and sign a transfer transaction
    Transfer {
        /// Sender's seed (hex string, 32 bytes)
        #[arg(short, long)]
        seed: String,

        /// Recipient address (hex, 0x-prefixed)
        #[arg(short, long)]
        to: String,

        /// Amount to transfer
        #[arg(short, long)]
        value: u64,

        /// Transaction nonce
        #[arg(short, long)]
        nonce: u64,
    },

    /// Create a signed transaction and submit via RPC
    Send {
        /// Sender's seed (hex string, 32 bytes)
        #[arg(short, long)]
        seed: String,

        /// Recipient address (hex, 0x-prefixed)
        #[arg(short, long)]
        to: String,

        /// Amount to transfer
        #[arg(short, long)]
        value: u64,

        /// Transaction nonce
        #[arg(short, long)]
        nonce: u64,

        /// RPC endpoint
        #[arg(short, long, default_value = "http://localhost:8545")]
        rpc: String,
    },

    /// Show validator accounts (deterministic from index)
    Validators {
        /// Number of validators
        #[arg(short, long, default_value = "4")]
        count: usize,
    },
}

#[derive(Serialize)]
struct RpcRequest {
    jsonrpc: &'static str,
    method: &'static str,
    params: serde_json::Value,
    id: u32,
}

#[derive(Deserialize)]
struct RpcResponse {
    result: Option<serde_json::Value>,
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    message: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::New { seed } => {
            let keypair = if let Some(seed_hex) = seed {
                let seed_bytes: [u8; 32] = hex::decode(&seed_hex)?
                    .try_into()
                    .map_err(|_| "Seed must be 32 bytes")?;
                KeyPair::from_seed(&seed_bytes)
            } else {
                KeyPair::generate()
            };

            let address = public_key_to_address(&keypair.public_key_bytes());

            println!("New Account Created");
            println!("==================");
            println!("Public Key (Node ID): {}", keypair.public_key_hex());
            println!("Address:              {}", address_to_hex(&address));
            println!();
            println!("IMPORTANT: Save your seed securely!");
            println!("The private key is derived from the seed.");
        }

        Commands::Account { seed } => {
            let seed_bytes: [u8; 32] = hex::decode(&seed)?
                .try_into()
                .map_err(|_| "Seed must be 32 bytes")?;
            let keypair = KeyPair::from_seed(&seed_bytes);
            let address = public_key_to_address(&keypair.public_key_bytes());

            println!("Account Information");
            println!("===================");
            println!("Seed:     {}", seed);
            println!("Node ID:  {}", keypair.public_key_hex());
            println!("Address:  {}", address_to_hex(&address));
        }

        Commands::Transfer {
            seed,
            to,
            value,
            nonce,
        } => {
            let seed_bytes: [u8; 32] = hex::decode(&seed)?
                .try_into()
                .map_err(|_| "Seed must be 32 bytes")?;
            let keypair = KeyPair::from_seed(&seed_bytes);

            let to_address = parse_address(&to)?;

            let tx = Transaction::transfer(&keypair, to_address, value, nonce);

            println!("Signed Transaction");
            println!("==================");
            println!("Hash:      0x{}", hex::encode(tx.hash));
            println!("From:      {}", address_to_hex(&tx.from));
            println!("To:        {}", to);
            println!("Value:     {}", value);
            println!("Nonce:     {}", nonce);
            println!("Signature: 0x{}", hex::encode(&tx.signature));
            println!();
            println!("RPC Payload:");
            let payload = serde_json::json!({
                "from": address_to_hex(&tx.from),
                "to": to,
                "value": value,
                "nonce": nonce,
                "signature": format!("0x{}", hex::encode(&tx.signature))
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }

        Commands::Send {
            seed,
            to,
            value,
            nonce,
            rpc,
        } => {
            let seed_bytes: [u8; 32] = hex::decode(&seed)?
                .try_into()
                .map_err(|_| "Seed must be 32 bytes")?;
            let keypair = KeyPair::from_seed(&seed_bytes);

            let to_address = parse_address(&to)?;
            let from_address = public_key_to_address(&keypair.public_key_bytes());

            let tx = Transaction::transfer(&keypair, to_address, value, nonce);

            println!("Sending Transaction");
            println!("===================");
            println!("From:  {}", address_to_hex(&from_address));
            println!("To:    {}", to);
            println!("Value: {}", value);
            println!("Nonce: {}", nonce);
            println!("RPC:   {}", rpc);
            println!();

            // Submit via RPC
            let client = reqwest::blocking::Client::new();
            let request = RpcRequest {
                jsonrpc: "2.0",
                method: "chain_submitTransaction",
                params: serde_json::json!({
                    "from": address_to_hex(&tx.from),
                    "to": to,
                    "value": value,
                    "nonce": nonce,
                    "signature": format!("0x{}", hex::encode(&tx.signature))
                }),
                id: 1,
            };

            let response = client
                .post(&rpc)
                .json(&request)
                .send()?
                .json::<RpcResponse>()?;

            if let Some(result) = response.result {
                println!("Transaction submitted!");
                println!("Result: {}", serde_json::to_string_pretty(&result)?);
            } else if let Some(error) = response.error {
                eprintln!("Error: {}", error.message);
            }
        }

        Commands::Validators { count } => {
            println!("Validator Accounts");
            println!("==================");
            println!();

            for i in 0..count {
                let mut seed = [0u8; 32];
                seed[0] = i as u8;
                let keypair = KeyPair::from_seed(&seed);
                let address = public_key_to_address(&keypair.public_key_bytes());

                println!("Validator {}:", i);
                println!("  Seed:    {}", hex::encode(seed));
                println!("  Node ID: {}...", &keypair.public_key_hex()[..16]);
                println!("  Address: {}", address_to_hex(&address));
                println!();
            }
        }
    }

    Ok(())
}

fn parse_address(s: &str) -> Result<[u8; 20], Box<dyn std::error::Error>> {
    let hex_str = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(hex_str)?;
    if bytes.len() != 20 {
        return Err("Address must be 20 bytes".into());
    }
    let mut address = [0u8; 20];
    address.copy_from_slice(&bytes);
    Ok(address)
}
