//! Stratum v1 job source implementation.
//!
//! This module integrates the Stratum v1 client into mujina-miner's job source
//! abstraction. It handles the conversion between Stratum protocol messages and
//! the internal JobTemplate/Share types used by the scheduler.

use anyhow::Result;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::stratum_v1::{ClientEvent, JobNotification, PoolConfig};

use super::{
    job, Extranonce2Range, GeneralPurposeBits, JobTemplate, MerkleRootKind, MerkleRootTemplate,
    Share, SourceCommand, SourceEvent, VersionTemplate,
};

/// Stratum v1 job source.
///
/// Wraps a StratumV1Client and bridges between the Stratum protocol and
/// mujina-miner's job source abstraction. Converts incoming mining.notify
/// messages to JobTemplates and outgoing Share submissions to Stratum format.
pub struct StratumV1Source {
    /// Pool configuration
    config: PoolConfig,

    /// Where to send events to scheduler
    event_tx: mpsc::Sender<SourceEvent>,

    /// Where to receive commands from scheduler
    command_rx: mpsc::Receiver<SourceCommand>,

    /// Shutdown signal
    shutdown: CancellationToken,

    /// Protocol state from subscription
    state: Option<ProtocolState>,
}

/// Protocol state after successful subscription.
#[derive(Debug, Clone)]
struct ProtocolState {
    /// Extranonce1 from mining.subscribe
    extranonce1: Vec<u8>,

    /// Extranonce2 size in bytes
    extranonce2_size: usize,

    /// Current share difficulty (from mining.set_difficulty)
    share_difficulty: Option<u64>,

    /// Authorized version mask (from mining.configure or mining.set_version_mask)
    version_mask: Option<u32>,
}

impl StratumV1Source {
    /// Create a new Stratum v1 source.
    pub fn new(
        config: PoolConfig,
        command_rx: mpsc::Receiver<SourceCommand>,
        event_tx: mpsc::Sender<SourceEvent>,
        shutdown: CancellationToken,
    ) -> Self {
        Self {
            config,
            event_tx,
            command_rx,
            shutdown,
            state: None,
        }
    }

    /// Convert Stratum JobNotification to JobTemplate.
    fn job_to_template(&self, job: JobNotification) -> Result<JobTemplate> {
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No protocol state (not subscribed)"))?;

        // Create extranonce2 range (full range for the given size)
        let extranonce2_range = Extranonce2Range::new(state.extranonce2_size as u8)?;

        // Convert version to VersionTemplate
        // Use authorized mask from pool (or none if pool didn't authorize version rolling)
        let gp_bits_mask = state
            .version_mask
            .map(|mask| GeneralPurposeBits::from(&mask.to_be_bytes()))
            .unwrap_or_else(GeneralPurposeBits::none);

        let version_template = VersionTemplate::new(job.version, gp_bits_mask)?;

        // Convert share difficulty to target
        // Default to difficulty 1 if not yet set by pool
        let share_difficulty = state.share_difficulty.unwrap_or(1);
        let share_target = job::difficulty_to_target(share_difficulty);

        Ok(JobTemplate {
            id: job.job_id,
            prev_blockhash: job.prev_hash,
            version: version_template,
            bits: job.nbits,
            share_target,
            time: job.ntime,
            merkle_root: MerkleRootKind::Computed(MerkleRootTemplate {
                coinbase1: job.coinbase1,
                extranonce1: state.extranonce1.clone(),
                extranonce2_range,
                coinbase2: job.coinbase2,
                merkle_branches: job.merkle_branches,
            }),
        })
    }

    /// Handle a client event.
    async fn handle_client_event(&mut self, event: ClientEvent) -> Result<()> {
        match event {
            ClientEvent::VersionRollingConfigured { authorized_mask } => {
                if let Some(mask) = authorized_mask {
                    info!(
                        mask = format!("{:#x}", mask),
                        "Version rolling authorized by pool"
                    );
                } else {
                    info!("Pool doesn't support version rolling");
                }

                // Store the mask (or lack thereof)
                if let Some(state) = &mut self.state {
                    state.version_mask = authorized_mask;
                } else {
                    // Configure happens before subscribe, so state might not exist yet
                    // Create temporary state that will be updated by Subscribed event
                    self.state = Some(ProtocolState {
                        extranonce1: Vec::new(),
                        extranonce2_size: 0,
                        share_difficulty: None,
                        version_mask: authorized_mask,
                    });
                }
            }

            ClientEvent::Subscribed {
                extranonce1,
                extranonce2_size,
            } => {
                info!(
                    pool = %self.config.url,
                    extranonce1 = hex::encode(&extranonce1),
                    extranonce2_size = %extranonce2_size,
                    "Subscribed to pool"
                );

                // Update or create protocol state
                // Preserve version_mask if already set by VersionRollingConfigured
                if let Some(state) = &mut self.state {
                    state.extranonce1 = extranonce1;
                    state.extranonce2_size = extranonce2_size;
                } else {
                    self.state = Some(ProtocolState {
                        extranonce1,
                        extranonce2_size,
                        share_difficulty: None,
                        version_mask: None,
                    });
                }
            }

            ClientEvent::NewJob(job) => {
                debug!(job_id = %job.job_id, clean_jobs = job.clean_jobs, "Received job from pool");

                let template = self.job_to_template(job.clone())?;

                // Clean jobs means previous work is invalid
                let event = if job.clean_jobs {
                    SourceEvent::ReplaceJob(template)
                } else {
                    SourceEvent::UpdateJob(template)
                };

                self.event_tx.send(event).await?;
            }

            ClientEvent::DifficultyChanged(diff) => {
                info!(difficulty = diff, "Pool difficulty changed");
                if let Some(state) = &mut self.state {
                    state.share_difficulty = Some(diff);
                }
            }

            ClientEvent::VersionMaskSet(mask) => {
                info!(mask = format!("{:#010x}", mask), "Version mask set");
                if let Some(state) = &mut self.state {
                    state.version_mask = Some(mask);
                }
            }

            ClientEvent::ShareAccepted { job_id } => {
                info!(job_id = %job_id, "Share accepted by pool");
            }

            ClientEvent::ShareRejected { job_id, reason } => {
                warn!(job_id = %job_id, reason = %reason, "Share rejected by pool");
            }

            ClientEvent::Disconnected => {
                warn!("Disconnected from pool");
                self.event_tx.send(SourceEvent::ClearJobs).await?;
            }

            ClientEvent::Error(err) => {
                warn!(error = %err, "Pool error");
            }
        }

        Ok(())
    }

    /// Convert Share to SubmitParams.
    fn share_to_submit_params(&self, share: Share) -> Result<crate::stratum_v1::SubmitParams> {
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No protocol state (not subscribed)"))?;

        // Extract extranonce2 from share (if present)
        let extranonce2 = share
            .extranonce2
            .map(Vec::from)
            .unwrap_or_else(|| vec![0; state.extranonce2_size]);

        // Extract version bits if version rolling was authorized
        // Always include version_bits parameter when pool authorized rolling,
        // even if the value is 0x00000000 (pool requires the field)
        let version_bits = state.version_mask.map(|mask| {
            let rolled = share.version.to_consensus() as u32;
            rolled & mask
        });

        Ok(crate::stratum_v1::SubmitParams {
            username: self.config.username.clone(),
            job_id: share.job_id,
            extranonce2,
            ntime: share.time,
            nonce: share.nonce,
            version_bits,
        })
    }

    /// Run the source (main event loop).
    ///
    /// Spawns the Stratum client and bridges events between the client and
    /// the job source interface.
    pub async fn run(mut self) -> Result<()> {
        info!(pool = %self.config.url, username = %self.config.username, "Starting Stratum v1 source");

        // Create channels for client communication
        let (client_event_tx, mut client_event_rx) = mpsc::channel(100);
        let (client_command_tx, client_command_rx) = mpsc::channel(100);

        // Create the Stratum client with command channel
        let client = crate::stratum_v1::StratumV1Client::with_commands(
            self.config.clone(),
            client_event_tx,
            client_command_rx,
            self.shutdown.clone(),
        );

        // Spawn client task
        let client_handle = tokio::spawn(async move {
            if let Err(e) = client.run().await {
                warn!(error = %e, "Stratum client error");
            }
        });

        // Main event loop
        loop {
            tokio::select! {
                // Events from Stratum client
                Some(event) = client_event_rx.recv() => {
                    if let Err(e) = self.handle_client_event(event).await {
                        warn!(error = %e, "Error handling client event");
                    }
                }

                // Commands from scheduler
                Some(cmd) = self.command_rx.recv() => {
                    match cmd {
                        SourceCommand::SubmitShare(share) => {
                            debug!(
                                job_id = %share.job_id,
                                nonce = format!("{:#x}", share.nonce),
                                "Submitting share to pool"
                            );

                            // Convert share to Stratum format and send to client
                            match self.share_to_submit_params(share) {
                                Ok(submit_params) => {
                                    if let Err(e) = client_command_tx.send(
                                        crate::stratum_v1::ClientCommand::SubmitShare(submit_params)
                                    ).await {
                                        warn!(error = %e, "Failed to send share to client");
                                    }
                                }
                                Err(e) => {
                                    warn!(error = %e, "Failed to convert share");
                                }
                            }
                        }
                    }
                }

                // Shutdown
                _ = self.shutdown.cancelled() => {
                    info!("Stratum v1 source shutting down");
                    break;
                }
            }
        }

        // Wait for client to finish
        client_handle.await?;

        Ok(())
    }
}
