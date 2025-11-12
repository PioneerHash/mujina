//! Daemon lifecycle management for mujina-miner.
//!
//! This module handles the core daemon functionality including initialization,
//! task management, signal handling, and graceful shutdown.

use tokio::signal::unix::{self, SignalKind};
use tokio::sync::mpsc;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::tracing::prelude::*;
use crate::{
    api::{self, ApiConfig},
    backplane::Backplane,
    hash_thread::HashThread,
    job_source::{dummy::DummySource, SourceEvent},
    scheduler::{self, SourceRegistration},
    transport::{TransportEvent, UsbTransport},
};

/// The main daemon.
pub struct Daemon {
    shutdown: CancellationToken,
    tracker: TaskTracker,
}

impl Daemon {
    /// Create a new daemon instance.
    pub fn new() -> Self {
        Self {
            shutdown: CancellationToken::new(),
            tracker: TaskTracker::new(),
        }
    }

    /// Run the daemon until shutdown is requested.
    pub async fn run(self) -> anyhow::Result<()> {
        // Create channels for component communication
        let (transport_tx, transport_rx) = mpsc::channel::<TransportEvent>(100);
        let (thread_tx, thread_rx) = mpsc::channel::<Vec<Box<dyn HashThread>>>(10);
        let (source_reg_tx, source_reg_rx) = mpsc::channel::<SourceRegistration>(10);

        // Create and start USB transport discovery
        let usb_transport = UsbTransport::new(transport_tx.clone());
        if let Err(e) = usb_transport.start_discovery(self.shutdown.clone()).await {
            error!("Failed to start USB discovery: {}", e);
        }

        // Create and start backplane
        let mut backplane = Backplane::new(transport_rx, thread_tx);
        self.tracker.spawn({
            let shutdown = self.shutdown.clone();
            async move {
                tokio::select! {
                    result = backplane.run() => {
                        if let Err(e) = result {
                            error!("Backplane error: {}", e);
                        }
                    }
                    _ = shutdown.cancelled() => {}
                }

                backplane.shutdown_all_boards().await;
            }
        });

        // Create and register DummySource
        let (source_event_tx, source_event_rx) = mpsc::channel::<SourceEvent>(100);
        let (source_cmd_tx, source_cmd_rx) = mpsc::channel(10);

        let dummy_source = DummySource::new(
            source_cmd_rx,
            source_event_tx,
            self.shutdown.clone(),
            tokio::time::Duration::from_secs(30),
        )?;

        source_reg_tx
            .send(SourceRegistration {
                name: "dummy".into(),
                event_rx: source_event_rx,
                command_tx: source_cmd_tx,
            })
            .await?;

        self.tracker.spawn(async move {
            if let Err(e) = dummy_source.run().await {
                error!("DummySource error: {}", e);
            }
        });

        // Start the scheduler
        self.tracker.spawn(scheduler::task(
            self.shutdown.clone(),
            thread_rx,
            source_reg_rx,
        ));

        // Start the API server
        self.tracker.spawn({
            let shutdown = self.shutdown.clone();
            async move {
                let config = ApiConfig::default();
                if let Err(e) = api::serve(config, shutdown).await {
                    error!("API server error: {}", e);
                }
            }
        });

        self.tracker.close();

        info!("Started.");
        info!("For hardware debugging, set RUST_LOG=mujina_miner=trace to see all communication");

        // Install signal handlers
        let mut sigint = unix::signal(SignalKind::interrupt())?;
        let mut sigterm = unix::signal(SignalKind::terminate())?;

        // Wait for shutdown signal
        tokio::select! {
            _ = sigint.recv() => {
                info!("Received SIGINT");
            },
            _ = sigterm.recv() => {
                info!("Received SIGTERM");
            },
        }

        // Initiate shutdown
        self.shutdown.cancel();

        // Wait for all tasks to complete
        self.tracker.wait().await;
        info!("Exiting.");

        Ok(())
    }
}

impl Default for Daemon {
    fn default() -> Self {
        Self::new()
    }
}
