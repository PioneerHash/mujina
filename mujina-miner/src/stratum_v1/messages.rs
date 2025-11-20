//! Stratum v1 message types and JSON-RPC serialization.
//!
//! This module defines the wire format for Stratum v1 protocol messages using
//! serde for JSON serialization. Messages follow the JSON-RPC format with
//! some Stratum-specific conventions.

use bitcoin::block::Version;
use bitcoin::hashes::Hash;
use bitcoin::{BlockHash, CompactTarget, TxMerkleNode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Events emitted by the Stratum client.
///
/// These events are sent via channel to the client consumer (typically a job
/// source) to notify about protocol state changes and new work.
#[derive(Debug, Clone)]
pub enum ClientEvent {
    /// Version rolling configured (result of mining.configure)
    ///
    /// Emitted after mining.configure if the pool supports version rolling.
    /// Contains the authorized mask, or None if pool declined/doesn't support.
    VersionRollingConfigured {
        /// Authorized version mask (bits that may be rolled)
        /// None if pool doesn't support or declined
        authorized_mask: Option<u32>,
    },

    /// Successfully connected and subscribed to pool
    Subscribed {
        /// Extranonce1 value from subscription
        extranonce1: Vec<u8>,
        /// Extranonce2 size in bytes
        extranonce2_size: usize,
    },

    /// New mining job received from pool
    NewJob(JobNotification),

    /// Difficulty changed
    DifficultyChanged(u64),

    /// Version mask set (for version rolling)
    VersionMaskSet(u32),

    /// Share was accepted by pool
    ShareAccepted {
        /// Job ID that was accepted
        job_id: String,
    },

    /// Share was rejected by pool
    ShareRejected {
        /// Job ID that was rejected
        job_id: String,
        /// Rejection reason from pool
        reason: String,
    },

    /// Disconnected from pool
    Disconnected,

    /// Error occurred (non-fatal, client may continue)
    Error(String),
}

/// Commands sent to the Stratum client.
///
/// External code (typically job source) sends commands to request actions.
#[derive(Debug, Clone)]
pub enum ClientCommand {
    /// Submit a share to the pool
    SubmitShare(SubmitParams),
}

/// Mining job notification from pool (mining.notify).
///
/// This is the core work unit sent by the pool. It contains all the data
/// needed to construct block headers for mining. Uses Rust Bitcoin types
/// for type safety.
#[derive(Debug, Clone)]
pub struct JobNotification {
    /// Unique job identifier
    pub job_id: String,

    /// Previous block hash
    pub prev_hash: BlockHash,

    /// First part of coinbase transaction (before extranonce)
    pub coinbase1: Vec<u8>,

    /// Second part of coinbase transaction (after extranonce)
    pub coinbase2: Vec<u8>,

    /// Merkle branch hashes for computing merkle root
    pub merkle_branches: Vec<TxMerkleNode>,

    /// Block version field
    pub version: Version,

    /// Encoded difficulty target (nbits)
    pub nbits: CompactTarget,

    /// Block timestamp (Unix epoch seconds)
    pub ntime: u32,

    /// If true, abandon all previous jobs
    pub clean_jobs: bool,
}

impl JobNotification {
    /// Parse from Stratum JSON array parameters.
    ///
    /// Converts hex strings from the pool protocol into typed Bitcoin structures.
    /// Uses manual parsing for better error context than serde tuple structs.
    pub fn from_stratum_params(params: &[Value]) -> Result<Self, String> {
        if params.len() < 9 {
            return Err("mining.notify params too short".to_string());
        }

        // Parse job_id
        let job_id = params[0].as_str().ok_or("job_id not a string")?.to_string();

        // Parse prev_hash (hex string, little-endian in Stratum)
        let prev_hash_str = params[1].as_str().ok_or("prev_hash not a string")?;
        let prev_hash = parse_block_hash(prev_hash_str)?;

        // Parse coinbase1 and coinbase2
        let coinbase1_str = params[2].as_str().ok_or("coinbase1 not a string")?;
        let coinbase1 = hex::decode(coinbase1_str).map_err(|e| format!("coinbase1 hex: {}", e))?;

        let coinbase2_str = params[3].as_str().ok_or("coinbase2 not a string")?;
        let coinbase2 = hex::decode(coinbase2_str).map_err(|e| format!("coinbase2 hex: {}", e))?;

        // Parse merkle branches
        let branches_json = params[4].as_array().ok_or("merkle_branches not an array")?;
        let mut merkle_branches = Vec::new();
        for branch in branches_json {
            let branch_str = branch.as_str().ok_or("merkle branch not a string")?;
            let node = parse_merkle_node(branch_str)?;
            merkle_branches.push(node);
        }

        // Parse version (hex string, big-endian)
        let version_str = params[5].as_str().ok_or("version not a string")?;
        let version_u32 =
            u32::from_str_radix(version_str, 16).map_err(|e| format!("version hex: {}", e))?;
        let version = Version::from_consensus(version_u32 as i32);

        // Parse nbits (hex string)
        let nbits_str = params[6].as_str().ok_or("nbits not a string")?;
        let nbits_u32 =
            u32::from_str_radix(nbits_str, 16).map_err(|e| format!("nbits hex: {}", e))?;
        let nbits = CompactTarget::from_consensus(nbits_u32);

        // Parse ntime (hex string)
        let ntime_str = params[7].as_str().ok_or("ntime not a string")?;
        let ntime = u32::from_str_radix(ntime_str, 16).map_err(|e| format!("ntime hex: {}", e))?;

        // Parse clean_jobs
        let clean_jobs = params[8].as_bool().ok_or("clean_jobs not a bool")?;

        Ok(Self {
            job_id,
            prev_hash,
            coinbase1,
            coinbase2,
            merkle_branches,
            version,
            nbits,
            ntime,
            clean_jobs,
        })
    }
}

/// Parse a block hash from Stratum hex string.
///
/// # Stratum v1's "Goofy" Block Hash Encoding
///
/// Stratum v1 uses a peculiar encoding for block hashes that differs from both
/// the human-readable display format and the raw internal byte representation.
///
/// ## The Problem
///
/// A block hash has three representations:
///
/// 1. **Human-readable (display)**: Big-endian, as shown in block explorers
///    Example: `000000000000000000015296bc96391d0d67f4a3...`
///
/// 2. **Internal (byte array)**: Little-endian, used in Bitcoin's wire protocol
///    Example: `[0xfd, 0x55, 0x64, 0x6b, 0xc1, 0x62, ...]`
///
/// 3. **Stratum v1 (this function)**: "Word-swapped" - 8 little-endian 4-byte
///    words, but transmitted as big-endian hex within each word
///    Example: `6b6455fd6db962c101f2d4fc0d67f4a3bc96391d...`
///
/// ## Why This Encoding?
///
/// Historical accident. Early Stratum implementations treated the 256-bit hash
/// as 8 separate 32-bit words for easier manipulation on 32-bit systems. Each
/// word was already little-endian internally, but when serialized to hex, the
/// bytes within each word appear in big-endian order.
///
/// ## Conversion Algorithm
///
/// To convert from Stratum format to internal format:
/// 1. Hex decode the string (gives 32 bytes)
/// 2. Split into 8 chunks of 4 bytes each
/// 3. Reverse the bytes within each chunk (word-swap)
/// 4. The result is now in internal byte array format
///
/// ## Example
///
/// ```text
/// Stratum:  "6b6455fd 6db962c1 01f2d4fc 0d67f4a3 bc96391d 00015296 00000000 00000000"
///            |------| |------| |------| |------| |------| |------| |------| |------|
///              W0       W1       W2       W3       W4       W5       W6       W7
///
/// After reversing each word:
/// Internal: [fd 55 64 6b] [c1 62 b9 6d] [fc d4 f2 01] [a3 f4 67 0d] ...
///
/// Display (reverse all):
///           000000000000000000015296bc96391d0d67f4a301f2d4fc6db962c16b6455fd
/// ```
///
/// ## References
///
/// - Real capture in `test_data::esp_miner_job::notify::PREV_BLOCKHASH_STRING`
/// - Discussion: https://github.com/slushpool/stratumprotocol/issues/9
fn parse_block_hash(hex: &str) -> Result<BlockHash, String> {
    let mut bytes = hex::decode(hex).map_err(|e| format!("block hash hex: {}", e))?;
    if bytes.len() != 32 {
        return Err(format!("block hash wrong length: {}", bytes.len()));
    }

    // Stratum's "word-swap" encoding: reverse bytes within each 4-byte word
    for chunk in bytes.chunks_mut(4) {
        chunk.reverse();
    }

    BlockHash::from_slice(&bytes).map_err(|e| format!("block hash parse: {}", e))
}

/// Parse a merkle node from Stratum hex string.
fn parse_merkle_node(hex: &str) -> Result<TxMerkleNode, String> {
    let bytes = hex::decode(hex).map_err(|e| format!("merkle node hex: {}", e))?;
    if bytes.len() != 32 {
        return Err(format!("merkle node wrong length: {}", bytes.len()));
    }
    TxMerkleNode::from_slice(&bytes).map_err(|e| format!("merkle node parse: {}", e))
}

/// Parameters for submitting a share to the pool.
#[derive(Debug, Clone)]
pub struct SubmitParams {
    /// Worker username
    pub username: String,

    /// Job ID this share is for
    pub job_id: String,

    /// Extranonce2 used
    pub extranonce2: Vec<u8>,

    /// Timestamp used (Unix epoch seconds)
    pub ntime: u32,

    /// Nonce found
    pub nonce: u32,

    /// Version bits used (optional, for version rolling)
    pub version_bits: Option<u32>,
}

impl SubmitParams {
    /// Convert to Stratum hex string format for transmission.
    ///
    /// Converts Bitcoin types back to the hex strings expected by the pool.
    pub fn to_stratum_json(&self) -> Vec<Value> {
        let extranonce2_hex = hex::encode(&self.extranonce2);
        let ntime_hex = format!("{:08x}", self.ntime);
        let nonce_hex = format!("{:08x}", self.nonce);

        let mut params = vec![
            Value::String(self.username.clone()),
            Value::String(self.job_id.clone()),
            Value::String(extranonce2_hex),
            Value::String(ntime_hex),
            Value::String(nonce_hex),
        ];

        if let Some(version_bits) = self.version_bits {
            params.push(Value::String(format!("{:08x}", version_bits)));
        }

        params
    }
}

/// JSON-RPC message envelope.
///
/// Stratum uses a simplified JSON-RPC format where messages can be:
/// - Requests (have method and params, may have id)
/// - Responses (have id and result or error)
/// - Notifications (have method and params, no id)
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    /// Request or notification from client or server
    Request {
        /// Message ID (null for notifications)
        id: Option<u64>,
        /// Method name (e.g., "mining.notify", "mining.subscribe")
        method: String,
        /// Method parameters
        params: Value,
    },

    /// Response to a request
    Response {
        /// Message ID matching the request
        id: u64,
        /// Result value (present on success)
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        /// Error value (present on failure)
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<Value>,
    },
}

impl JsonRpcMessage {
    /// Create a new request message.
    pub fn request(id: u64, method: impl Into<String>, params: Value) -> Self {
        JsonRpcMessage::Request {
            id: Some(id),
            method: method.into(),
            params,
        }
    }

    /// Create a notification (request without ID).
    pub fn notification(method: impl Into<String>, params: Value) -> Self {
        JsonRpcMessage::Request {
            id: None,
            method: method.into(),
            params,
        }
    }

    /// Get the message ID if present.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "will be used as client matures")
    )]
    pub fn id(&self) -> Option<u64> {
        match self {
            JsonRpcMessage::Request { id, .. } => *id,
            JsonRpcMessage::Response { id, .. } => Some(*id),
        }
    }

    /// Check if this is a notification (request without ID).
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "will be used as client matures")
    )]
    pub fn is_notification(&self) -> bool {
        matches!(self, JsonRpcMessage::Request { id: None, .. })
    }

    /// Get the method name for requests.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "will be used as client matures")
    )]
    pub fn method(&self) -> Option<&str> {
        match self {
            JsonRpcMessage::Request { method, .. } => Some(method),
            JsonRpcMessage::Response { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_mining_notify() {
        let json = json!({
            "id": null,
            "method": "mining.notify",
            "params": [
                "job1",
                "prevhash",
                "coinbase1",
                "coinbase2",
                ["merkle1", "merkle2"],
                "20000000",
                "1a00ffff",
                "504e86b9",
                true
            ]
        });

        let msg: JsonRpcMessage = serde_json::from_value(json).unwrap();
        assert!(msg.is_notification());
        assert_eq!(msg.method(), Some("mining.notify"));
    }

    #[test]
    fn test_parse_response() {
        let json = json!({
            "id": 1,
            "result": true,
            "error": null
        });

        let msg: JsonRpcMessage = serde_json::from_value(json).unwrap();
        assert_eq!(msg.id(), Some(1));
    }

    #[test]
    fn test_create_request() {
        let msg = JsonRpcMessage::request(1, "mining.subscribe", json!(["mujina-miner/0.1.0"]));

        let serialized = serde_json::to_string(&msg).unwrap();
        assert!(serialized.contains("mining.subscribe"));
        assert!(serialized.contains("\"id\":1"));
    }

    #[test]
    fn test_parse_block_hash_stratum_encoding() {
        // Stratum sends block hashes with word-swapped encoding
        // This is "6b6455fd" + "6db962c1" + ... (8 words of 4 bytes each)
        let stratum_hex = "6b6455fd6db962c101f2d4fc0d67f4a3bc96391d000152960000000000000000";
        let hash = parse_block_hash(stratum_hex).unwrap();

        // After word-swapping, the internal bytes should be:
        // [fd 55 64 6b] [c1 62 b9 6d] [fc d4 f2 01] [a3 f4 67 0d]
        // [1d 39 96 bc] [96 52 01 00] [00 00 00 00] [00 00 00 00]
        let bytes = hash.as_byte_array();
        assert_eq!(bytes.len(), 32);

        // Verify word-swap happened correctly (first word)
        assert_eq!(&bytes[0..4], &[0xfd, 0x55, 0x64, 0x6b]);

        // Verify word-swap happened correctly (second word)
        assert_eq!(&bytes[4..8], &[0xc1, 0x62, 0xb9, 0x6d]);

        // The human-readable display should be the reverse of internal bytes
        let display = format!("{}", hash);
        assert_eq!(
            display,
            "000000000000000000015296bc96391d0d67f4a301f2d4fc6db962c16b6455fd"
        );
    }

    #[test]
    fn test_parse_merkle_node() {
        let hex = "d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6";
        let node = parse_merkle_node(hex).unwrap();

        let bytes = node.as_byte_array();
        assert_eq!(bytes.len(), 32);
        assert_eq!(bytes[0], 0xd5);
        assert_eq!(bytes[31], 0xe6);
    }

    #[test]
    fn test_parse_invalid_block_hash() {
        // Too short
        let result = parse_block_hash("deadbeef");
        assert!(result.is_err());

        // Invalid hex
        let result =
            parse_block_hash("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz");
        assert!(result.is_err());
    }

    #[test]
    fn test_submit_params_to_stratum_json() {
        let params = SubmitParams {
            username: "worker1".to_string(),
            job_id: "job123".to_string(),
            extranonce2: vec![0xde, 0xad, 0xbe, 0xef],
            ntime: 0x65432100,
            nonce: 0x12345678,
            version_bits: Some(0x20000000),
        };

        let json = params.to_stratum_json();

        assert_eq!(json[0], Value::String("worker1".to_string()));
        assert_eq!(json[1], Value::String("job123".to_string()));
        assert_eq!(json[2], Value::String("deadbeef".to_string()));
        assert_eq!(json[3], Value::String("65432100".to_string()));
        assert_eq!(json[4], Value::String("12345678".to_string()));
        assert_eq!(json[5], Value::String("20000000".to_string()));
    }

    #[test]
    fn test_submit_params_without_version_bits() {
        let params = SubmitParams {
            username: "worker1".to_string(),
            job_id: "job123".to_string(),
            extranonce2: vec![0xaa, 0xbb],
            ntime: 0x11223344,
            nonce: 0x55667788,
            version_bits: None,
        };

        let json = params.to_stratum_json();

        // Should have 5 elements (no version_bits)
        assert_eq!(json.len(), 5);
        assert_eq!(json[2], Value::String("aabb".to_string()));
        assert_eq!(json[3], Value::String("11223344".to_string()));
        assert_eq!(json[4], Value::String("55667788".to_string()));
    }

    #[test]
    fn test_job_notification_minimal_params() {
        // Minimum valid params array
        let params = json!([
            "job1",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "aa",
            "bb",
            [],
            "20000000",
            "1d00ffff",
            "5a5a5a5a",
            false
        ]);

        let params_array = params.as_array().unwrap();
        let job = JobNotification::from_stratum_params(params_array).unwrap();

        assert_eq!(job.job_id, "job1");
        assert_eq!(job.coinbase1, vec![0xaa]);
        assert_eq!(job.coinbase2, vec![0xbb]);
        assert_eq!(job.merkle_branches.len(), 0);
        assert!(!job.clean_jobs);
    }

    #[test]
    fn test_job_notification_from_real_bitaxe_capture() {
        // Real capture from Bitaxe Gamma running esp-miner
        // This validates our parsing against actual pool communication
        // that resulted in an accepted share at difficulty 29588
        use crate::asic::bm13xx::test_data::esp_miner_job::notify;
        use crate::asic::bm13xx::test_data::esp_miner_job::wire_tx;

        // Build Stratum params array from the capture
        let params = json!([
            notify::JOB_ID_STRING,
            notify::PREV_BLOCKHASH_STRING,
            notify::COINBASE1,
            notify::COINBASE2,
            notify::MERKLE_BRANCH_STRINGS.to_vec(),
            notify::VERSION_STRING,
            notify::NBITS_STRING,
            notify::NTIME_STRING,
            notify::CLEAN_JOBS
        ]);

        // Parse with our implementation
        let job = JobNotification::from_stratum_params(params.as_array().unwrap())
            .expect("Failed to parse real pool capture");

        // Validate job ID
        assert_eq!(job.job_id, notify::JOB_ID_STRING);

        // Validate against known-good Bitcoin types from wire capture
        // The wire capture was sent to hardware and produced an accepted share,
        // so we know these values are correct
        assert_eq!(
            job.prev_hash,
            *notify::PREV_BLOCKHASH,
            "Previous block hash mismatch"
        );

        assert_eq!(job.version, *wire_tx::VERSION, "Version mismatch");

        assert_eq!(job.nbits, *wire_tx::NBITS, "Difficulty bits mismatch");

        assert_eq!(job.ntime, *wire_tx::NTIME, "Timestamp mismatch");

        // Validate merkle branches
        assert_eq!(
            job.merkle_branches.len(),
            12,
            "Wrong number of merkle branches"
        );
        for (i, branch) in job.merkle_branches.iter().enumerate() {
            assert_eq!(
                branch,
                &notify::MERKLE_BRANCHES[i],
                "Merkle branch {} mismatch",
                i
            );
        }

        // Validate coinbase parts
        assert_eq!(
            job.coinbase1,
            hex::decode(notify::COINBASE1).unwrap(),
            "Coinbase1 mismatch"
        );
        assert_eq!(
            job.coinbase2,
            hex::decode(notify::COINBASE2).unwrap(),
            "Coinbase2 mismatch"
        );

        assert_eq!(job.clean_jobs, notify::CLEAN_JOBS);
    }
}
