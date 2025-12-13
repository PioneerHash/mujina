//! Stratum V2 job source implementation.
//!
//! This module implements the Stratum V2 mining protocol as a job source,
//! parallel to the existing Stratum V1 implementation.
//!
//! # Architecture
//!
//! The Stratum V2 source follows the same pattern as V1:
//! - Wraps an SV2 protocol client
//! - Converts SV2 messages to `SourceEvent` for the scheduler
//! - Converts `SourceCommand` to SV2 messages for the pool
//!
//! # Two-Phase Job Lifecycle
//!
//! SV2 uses a two-phase job activation model:
//! 1. **NewMiningJob** arrives (is_future = true) with merkle_root and version
//! 2. **SetNewPrevHash** arrives with prev_hash, nbits, ntime
//! 3. When both are present, combine them into a JobTemplate and start mining
//!
//! This allows pools to pre-distribute block templates before the previous block
//! is found, reducing latency when a new block arrives.

pub mod client;
pub mod messages;
pub mod state;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use stratum_core::mining_sv2::{SetTarget, NewMiningJob, SetNewPrevHash};
use stratum_core::parsers_sv2::Mining;

use crate::job_source::{SourceCommand, SourceEvent};
use crate::tracing::prelude::*;

use client::StratumV2Client;
use messages::{job_to_template, share_to_submit};
use state::ProtocolState;

/// Stratum V2 pool configuration
#[derive(Debug, Clone)]
pub struct StratumV2Config {
    pub url: String,
    pub worker: String,
    pub password: Option<String>,
    pub user_agent: String,
}

/// Stratum V2 job source
///
/// Wraps the Stratum V2 client and bridges between the SV2 protocol
/// and mujina's SourceEvent/SourceCommand abstraction.
pub struct StratumV2Source {
    config: StratumV2Config,
    command_rx: mpsc::Receiver<SourceCommand>,
    event_tx: mpsc::Sender<SourceEvent>,
    shutdown: CancellationToken,
    state: ProtocolState,
}

impl StratumV2Source {
    pub fn new(
        config: StratumV2Config,
        command_rx: mpsc::Receiver<SourceCommand>,
        event_tx: mpsc::Sender<SourceEvent>,
        shutdown: CancellationToken,
    ) -> Self {
        Self {
            config,
            command_rx,
            event_tx,
            shutdown,
            state: ProtocolState::new(),
        }
    }

    pub async fn run(mut self) -> Result<()> {
        info!("Starting Stratum V2 source");

        // Connect to pool with Noise protocol
        let mut client = StratumV2Client::connect(&self.config)
            .await
            .context("Failed to connect to SV2 pool")?;

        // Setup connection
        client
            .setup_connection(&self.config)
            .await
            .context("SetupConnection failed")?;

        // Open standard mining channel
        let channel_id = client
            .open_standard_mining_channel(&self.config)
            .await
            .context("OpenStandardMiningChannel failed")?;

        self.state.channel_id = Some(channel_id);

        info!("Stratum V2 connection established, entering main loop");

        // Main event loop
        loop {
            tokio::select! {
                // Receive messages from pool
                Ok(frame) = client.next_message() => {
                    if let Err(e) = self.handle_pool_message(frame, &mut client).await {
                        error!("Error handling pool message: {}", e);
                    }
                }

                // Receive commands from scheduler
                Some(cmd) = self.command_rx.recv() => {
                    if let Err(e) = self.handle_scheduler_command(cmd, &mut client).await {
                        error!("Error handling scheduler command: {}", e);
                    }
                }

                // Shutdown signal
                _ = self.shutdown.cancelled() => {
                    info!("Stratum V2 source shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Handle message from pool
    async fn handle_pool_message(
        &mut self,
        mut frame: client::StdFrame,
        _client: &mut StratumV2Client,
    ) -> Result<()> {
        // Parse message type from frame header
        let header = frame.get_header().context("Missing frame header")?;
        let message_type = header.msg_type();
        let mut payload = frame.payload().to_vec();

        // Try to decode as Mining message using TryFrom
        let mining_msg: Mining = (message_type, payload.as_mut_slice())
            .try_into()
            .map_err(|e| anyhow::anyhow!("Failed to decode Mining message: {:?}", e))?;

        // Handle based on message type
        match mining_msg {
            Mining::NewMiningJob(job) => {
                self.handle_new_mining_job(job.as_static()).await?;
            }
            Mining::SetNewPrevHash(prev_hash) => {
                self.handle_set_new_prev_hash(prev_hash.as_static()).await?;
            }
            Mining::SetTarget(target) => {
                self.handle_set_target(target.as_static()).await?;
            }
            Mining::SubmitSharesSuccess(_) => {
                debug!("Share accepted by pool");
                // TODO: Emit share acceptance event for statistics
            }
            Mining::SubmitSharesError(error) => {
                let error_code = String::from_utf8_lossy(error.error_code.as_ref());
                warn!("Share rejected by pool: {}", error_code);
                // TODO: Emit share rejection event for statistics
            }
            Mining::SetCustomMiningJob(_) => {
                // SetCustomMiningJob - not supported for standard channels
                warn!("Received SetCustomMiningJob on standard channel (ignored)");
            }
            other => {
                debug!("Unhandled mining message: {:?}", other);
            }
        }

        Ok(())
    }

    /// Handle NewMiningJob message
    async fn handle_new_mining_job(
        &mut self,
        job: NewMiningJob<'static>,
    ) -> Result<()> {
        info!(
            "NewMiningJob: channel_id={}, job_id={}, is_future={}",
            job.channel_id,
            job.job_id,
            job.is_future()
        );

        if job.is_future() {
            // Store for later activation when SetNewPrevHash arrives
            let job_id = job.job_id;
            self.state.store_future_job(job);

            // Try to activate if we already have a matching prev_hash
            if let Some(prev_hash) = &self.state.prev_hash {
                if prev_hash.job_id == job_id {
                    self.activate_job(job_id).await?;
                }
            }
        } else {
            // Non-future jobs should not happen with standard channels
            warn!("Received non-future NewMiningJob (unexpected for standard channel)");

            // Try to activate immediately if we have prev_hash
            if let Some(prev_hash) = &self.state.prev_hash {
                let template = job_to_template(
                    &job,
                    prev_hash,
                    self.state.current_target.as_deref().unwrap_or(&[0xFF; 32]),
                    self.state.version_mask,
                )?;

                self.event_tx
                    .send(SourceEvent::UpdateJob(template))
                    .await
                    .context("Failed to send UpdateJob event")?;
            }
        }

        // Clean old jobs (keep last 10)
        self.state.clean_old_jobs(10);

        Ok(())
    }

    /// Handle SetNewPrevHash message
    async fn handle_set_new_prev_hash(
        &mut self,
        prev_hash: SetNewPrevHash<'static>,
    ) -> Result<()> {
        info!(
            "SetNewPrevHash: channel_id={}, job_id={}",
            prev_hash.channel_id, prev_hash.job_id
        );

        // Store prev_hash
        let job_id = prev_hash.job_id;
        self.state.prev_hash = Some(prev_hash);

        // Try to activate matching future job
        self.activate_job(job_id).await?;

        Ok(())
    }

    /// Activate a job when both NewMiningJob and SetNewPrevHash are available
    async fn activate_job(&mut self, job_id: u32) -> Result<()> {
        // Get the future job
        let job = match self.state.get_future_job(job_id) {
            Some(j) => j,
            None => {
                debug!("Cannot activate job {}: future job not found yet", job_id);
                return Ok(());
            }
        };

        // Get the prev_hash
        let prev_hash = match &self.state.prev_hash {
            Some(ph) if ph.job_id == job_id => ph,
            _ => {
                debug!(
                    "Cannot activate job {}: matching prev_hash not found yet",
                    job_id
                );
                return Ok(());
            }
        };

        // Convert to JobTemplate
        let template = job_to_template(
            job,
            prev_hash,
            self.state.current_target.as_deref().unwrap_or(&[0xFF; 32]),
            self.state.version_mask,
        )?;

        info!("Activating job {}: sending ReplaceJob to scheduler", job_id);

        // Send ReplaceJob to scheduler (clean jobs, old work invalid)
        self.event_tx
            .send(SourceEvent::ReplaceJob(template))
            .await
            .context("Failed to send ReplaceJob event")?;

        Ok(())
    }

    /// Handle SetTarget message
    async fn handle_set_target(&mut self, target: SetTarget<'static>) -> Result<()> {
        info!("SetTarget: channel_id={}", target.channel_id);

        // Store new target
        self.state.current_target = Some(target.maximum_target.to_vec());

        // TODO: If we have an active job, send UpdateJob with new difficulty
        // For now, the next job will use the new target

        Ok(())
    }

    /// Handle command from scheduler
    async fn handle_scheduler_command(
        &mut self,
        cmd: SourceCommand,
        client: &mut StratumV2Client,
    ) -> Result<()> {
        match cmd {
            SourceCommand::SubmitShare(share) => {
                // Get channel_id
                let channel_id = self
                    .state
                    .channel_id
                    .context("Cannot submit share: no channel opened")?;

                // Get sequence number
                let sequence_number = self.state.next_sequence_number();

                // Convert share to SV2 format
                let submit = share_to_submit(&share, channel_id, sequence_number)?;

                // Submit to pool
                client.submit_share(submit).await?;
            }
        }

        Ok(())
    }
}
