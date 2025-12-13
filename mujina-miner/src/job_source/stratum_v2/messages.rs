//! Message type conversions between SV2 and mujina types.
//!
//! This module handles converting between:
//! - SV2 mining messages (NewMiningJob, SetNewPrevHash) → JobTemplate
//! - Share → SubmitSharesStandard

use anyhow::{Context, Result};
use bitcoin::block::Version;
use bitcoin::hash_types::TxMerkleNode;
use bitcoin::hashes::Hash;
use bitcoin::pow::Target;

// Note: We don't need stratum_core bitcoin types for conversion
// We only use them for parsing SV2 messages, then convert to bitcoin crate types
use stratum_core::mining_sv2::{NewMiningJob, SetNewPrevHash};

use crate::job_source::{GeneralPurposeBits, JobTemplate, MerkleRootKind, Share, VersionTemplate};

/// Convert SV2 NewMiningJob + SetNewPrevHash to JobTemplate
///
/// SV2 uses a two-phase job lifecycle:
/// 1. NewMiningJob arrives (is_future = true) with merkle_root
/// 2. SetNewPrevHash arrives with prev_hash, nbits, ntime
/// 3. Combine them to create complete JobTemplate
pub fn job_to_template(
    job: &NewMiningJob<'static>,
    prev_hash: &SetNewPrevHash<'static>,
    current_target: &[u8],
    version_mask: Option<u32>,
) -> Result<JobTemplate> {
    // Extract merkle root from job (SV2 provides it directly)
    let merkle_root_bytes: [u8; 32] = job
        .merkle_root
        .to_vec()
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid merkle root length"))?;

    // Convert to TxMerkleNode (from bitcoin crate)
    let merkle_root = TxMerkleNode::from_byte_array(merkle_root_bytes);

    // Convert target bytes to Target
    // SV2 uses 32-byte little-endian target
    let share_target = if current_target.len() == 32 {
        Target::from_le_bytes(
            current_target
                .try_into()
                .context("Invalid target length")?,
        )
    } else {
        // Fallback to max target if not set yet
        Target::MAX
    };

    // Convert prev_hash bytes to BlockHash
    let prev_hash_bytes: [u8; 32] = prev_hash
        .prev_hash
        .to_vec()
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid prev_hash length"))?;

    // Convert to bitcoin crate BlockHash
    let prev_blockhash = bitcoin::hash_types::BlockHash::from_byte_array(prev_hash_bytes);

    // Create version template with optional rolling mask
    // Convert version_mask (u32) to GeneralPurposeBits (16 bits, bits 13-28)
    let gp_bits = if let Some(mask) = version_mask {
        // Extract bits 13-28 from the mask (middle 16 bits)
        let gp_mask = ((mask >> 13) & 0xffff) as u16;
        GeneralPurposeBits::new(gp_mask.to_be_bytes())
    } else {
        // If no mask provided, allow full rolling (SV2 default)
        GeneralPurposeBits::full()
    };
    let version_template = VersionTemplate::new(Version::from_consensus(job.version as i32), gp_bits)
        .context("Invalid version")?;

    Ok(JobTemplate {
        id: job.job_id.to_string(),
        prev_blockhash,
        version: version_template,
        bits: bitcoin::pow::CompactTarget::from_consensus(prev_hash.nbits),
        share_target,
        time: prev_hash.min_ntime, // Use min_ntime from SetNewPrevHash
        merkle_root: MerkleRootKind::Fixed(merkle_root),
    })
}

/// Convert mujina Share to SV2 SubmitSharesStandard
pub fn share_to_submit(
    share: &Share,
    channel_id: u32,
    sequence_number: u32,
) -> Result<stratum_core::mining_sv2::SubmitSharesStandard> {
    // Parse job_id from string to u32
    let job_id: u32 = share
        .job_id
        .parse()
        .context("Invalid job_id (not a u32)")?;

    Ok(stratum_core::mining_sv2::SubmitSharesStandard {
        channel_id,
        sequence_number,
        job_id,
        nonce: share.nonce,
        ntime: share.time,
        version: share.version.to_consensus() as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_share_to_submit() {
        use crate::job_source::Share;
        use bitcoin::block::Version;

        let share = Share {
            job_id: "12345".to_string(),
            nonce: 0xdeadbeef,
            time: 1234567890,
            version: Version::from_consensus(0x20000000),
            extranonce2: None,
        };

        let submit = share_to_submit(&share, 1, 42).unwrap();
        assert_eq!(submit.channel_id, 1);
        assert_eq!(submit.sequence_number, 42);
        assert_eq!(submit.job_id, 12345);
        assert_eq!(submit.nonce, 0xdeadbeef);
        assert_eq!(submit.ntime, 1234567890);
        assert_eq!(submit.version, 0x20000000);
    }
}
