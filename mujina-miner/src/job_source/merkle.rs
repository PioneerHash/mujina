//! Merkle root specification for mining jobs.

use bitcoin::hash_types::TxMerkleNode;

use super::extranonce2::Extranonce2Range;

/// Specifies how to obtain the merkle root for a mining job.
///
/// The merkle root can either be provided directly as a fixed value or computed
/// dynamically from coinbase transaction parts. In Stratum v1, miners typically
/// roll extranonce2 to generate different merkle roots. In Stratum v2 simple
/// mode, the merkle root is fixed and only the nonce is rolled.
#[derive(Debug, Clone)]
pub enum MerkleRootKind {
    /// Pre-computed merkle root that never changes.
    ///
    /// Used for test jobs, dummy work, Stratum v2 simple mode, or any scenario
    /// where the coinbase transaction is already finalized and won't be modified
    /// by the miner.
    Fixed(TxMerkleNode),

    /// Merkle root must be computed from coinbase parts.
    ///
    /// The coinbase transaction contains extranonce fields that can be modified
    /// to generate different merkle roots. Each unique extranonce2 value produces
    /// a different coinbase hash, requiring recomputation of the merkle tree.
    /// This is the standard mode for Stratum v1 pool mining.
    Computed(MerkleRootTemplate),
}

/// Template for computing merkle roots from coinbase transaction parts.
///
/// Contains all the components needed to build a coinbase transaction and compute
/// its merkle root. As extranonce2 is rolled, each unique value produces a different
/// coinbase transaction hash, which propagates up the merkle tree to produce a
/// different merkle root.
#[derive(Debug, Clone)]
pub struct MerkleRootTemplate {
    /// First part of coinbase transaction (before extranonces).
    pub coinbase1: Vec<u8>,

    /// Extranonce1 value assigned by the source.
    ///
    /// This is set once per connection and tends to remain constant for all jobs from
    /// this source.
    pub extranonce1: Vec<u8>,

    /// Extranonce2 range defining the available rolling space.
    ///
    /// The caller will create an iterator from this range to generate different
    /// extranonce2 values for unique block headers.
    pub extranonce2_range: Extranonce2Range,

    /// Second part of coinbase transaction (after extranonces).
    pub coinbase2: Vec<u8>,

    /// Merkle branches for building the merkle root.
    ///
    /// After hashing the coinbase transaction, these branches are used to climb
    /// the merkle tree to compute the final merkle root for the block header.
    pub merkle_branches: Vec<TxMerkleNode>,
}
