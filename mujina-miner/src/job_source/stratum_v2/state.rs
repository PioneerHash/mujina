//! Stratum V2 protocol state management.
//!
//! Tracks:
//! - Channel ID and sequence numbers
//! - Future jobs awaiting SetNewPrevHash
//! - Current difficulty target
//! - Version mask for version rolling

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use stratum_core::mining_sv2::{NewMiningJob, SetNewPrevHash};

/// Protocol state for SV2 connection
pub struct ProtocolState {
    /// Channel ID from OpenStandardMiningChannelSuccess
    pub channel_id: Option<u32>,

    /// Sequence number for share submissions (auto-incrementing)
    sequence_number: AtomicU32,

    /// Future jobs waiting for SetNewPrevHash activation
    pub future_jobs: HashMap<u32, NewMiningJob<'static>>,

    /// Most recent SetNewPrevHash (may arrive before matching job)
    pub prev_hash: Option<SetNewPrevHash<'static>>,

    /// Current difficulty target from SetTarget message
    pub current_target: Option<Vec<u8>>,

    /// Version mask for version rolling (from SetupConnectionSuccess)
    pub version_mask: Option<u32>,
}

impl ProtocolState {
    pub fn new() -> Self {
        Self {
            channel_id: None,
            sequence_number: AtomicU32::new(0),
            future_jobs: HashMap::new(),
            prev_hash: None,
            current_target: None,
            version_mask: None,
        }
    }

    /// Get next sequence number for share submission
    pub fn next_sequence_number(&self) -> u32 {
        self.sequence_number.fetch_add(1, Ordering::Relaxed)
    }

    /// Store a future job
    pub fn store_future_job(&mut self, job: NewMiningJob<'static>) {
        self.future_jobs.insert(job.job_id, job);
    }

    /// Get a future job by job_id
    pub fn get_future_job(&self, job_id: u32) -> Option<&NewMiningJob<'static>> {
        self.future_jobs.get(&job_id)
    }

    /// Remove old future jobs (keep only last N)
    pub fn clean_old_jobs(&mut self, keep_count: usize) {
        if self.future_jobs.len() > keep_count {
            // Keep only the most recent jobs (by job_id)
            let mut job_ids: Vec<u32> = self.future_jobs.keys().copied().collect();
            job_ids.sort_unstable();

            let to_remove = job_ids.len().saturating_sub(keep_count);
            for &job_id in &job_ids[..to_remove] {
                self.future_jobs.remove(&job_id);
            }
        }
    }
}

impl Default for ProtocolState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sequence_numbers() {
        let state = ProtocolState::new();
        assert_eq!(state.next_sequence_number(), 0);
        assert_eq!(state.next_sequence_number(), 1);
        assert_eq!(state.next_sequence_number(), 2);
    }

    #[test]
    fn test_clean_old_jobs() {
        let mut state = ProtocolState::new();

        // Add several jobs (using mock data - this is just for testing the cleanup logic)
        // In real usage, NewMiningJob would come from the pool
        // For now, just test that the HashMap management works

        // Test that new state is empty
        assert_eq!(state.future_jobs.len(), 0);

        // Clean with no jobs does nothing
        state.clean_old_jobs(5);
        assert_eq!(state.future_jobs.len(), 0);
    }
}
