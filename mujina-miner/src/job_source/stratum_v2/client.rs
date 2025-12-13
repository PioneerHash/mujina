//! Stratum V2 protocol client implementation.
//!
//! This module handles the low-level SV2 protocol details including:
//! - Noise protocol encryption handshake
//! - Binary message framing and encoding/decoding
//! - Channel management (SetupConnection, OpenStandardMiningChannel)
//! - Mining protocol messages (NewMiningJob, SetNewPrevHash, SubmitSharesStandard, etc.)

use anyhow::{bail, Context, Result};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpStream;

use async_channel::{Receiver, Sender};
use stratum_apps::network_helpers::noise_connection::Connection;
use stratum_core::{
    codec_sv2::{HandshakeRole, StandardEitherFrame, StandardSv2Frame},
    common_messages_sv2::{Protocol, SetupConnection},
    mining_sv2::{OpenStandardMiningChannel, SubmitSharesStandard},
    noise_sv2::Initiator,
    parsers_sv2::{CommonMessages, Mining, MiningDeviceMessages},
};

use crate::tracing::prelude::*;

use super::StratumV2Config;

/// Type alias for SV2 message frames (following mining-device pattern)
pub type Message = MiningDeviceMessages<'static>;
pub type StdFrame = StandardSv2Frame<Message>;
pub type EitherFrame = StandardEitherFrame<Message>;

/// Stratum V2 protocol client
///
/// Handles connection to SV2 pool, Noise handshake, and protocol message exchange.
pub struct StratumV2Client {
    /// Message receiver from pool
    receiver: Receiver<EitherFrame>,

    /// Message sender to pool
    sender: Sender<EitherFrame>,

    /// Pool address for logging
    address: SocketAddr,
}

impl StratumV2Client {
    /// Connect to SV2 pool with Noise protocol handshake
    pub async fn connect(config: &StratumV2Config) -> Result<Self> {
        info!("Connecting to SV2 pool: {}", config.url);

        // Parse URL (sv2+tcp://host:port)
        let address = Self::parse_url(&config.url)?;

        // Connect TCP socket with retry and timeout
        let socket = Self::connect_with_retry(address).await?;

        // Noise handshake (Initiator role, no authentication key for now)
        let initiator = Initiator::new(None);
        let (receiver, sender) =
            Connection::new(socket, HandshakeRole::Initiator(initiator))
                .await
                .map_err(|e| anyhow::anyhow!("Noise handshake failed: {:?}", e))?;

        info!("Noise handshake completed successfully");

        Ok(Self {
            receiver,
            sender,
            address,
        })
    }

    /// Parse sv2+tcp://host:port URL
    fn parse_url(url: &str) -> Result<SocketAddr> {
        // Remove sv2+tcp:// prefix
        let addr_str = url
            .strip_prefix("sv2+tcp://")
            .context("URL must start with sv2+tcp://")?;

        // Parse as SocketAddr
        addr_str
            .parse()
            .with_context(|| format!("Invalid address: {}", addr_str))
    }

    /// Connect TCP socket with retry logic
    async fn connect_with_retry(address: SocketAddr) -> Result<TcpStream> {
        let max_retries = 3;
        let retry_delay = Duration::from_secs(5);

        for attempt in 1..=max_retries {
            debug!("Connection attempt {}/{} to {}", attempt, max_retries, address);

            match tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(address)).await
            {
                Ok(Ok(socket)) => {
                    info!("TCP connection established to {}", address);
                    return Ok(socket);
                }
                Ok(Err(e)) => {
                    warn!(
                        "Connection attempt {}/{} failed: {}",
                        attempt, max_retries, e
                    );
                }
                Err(_) => {
                    warn!(
                        "Connection attempt {}/{} timed out after 10s",
                        attempt, max_retries
                    );
                }
            }

            if attempt < max_retries {
                debug!("Retrying in {:?}...", retry_delay);
                tokio::time::sleep(retry_delay).await;
            }
        }

        bail!("Failed to connect after {} attempts", max_retries)
    }

    /// Send SetupConnection and wait for SetupConnectionSuccess
    pub async fn setup_connection(&mut self, config: &StratumV2Config) -> Result<()> {
        info!("Sending SetupConnection");

        // Build SetupConnection message
        let setup = SetupConnection {
            protocol: Protocol::MiningProtocol,
            min_version: 2,
            max_version: 2,
            flags: 0b0000_0000_0000_0000_0000_0000_0000_0001, // Requires standard jobs
            endpoint_host: self.address.ip().to_string().into_bytes().try_into()
                .map_err(|e| anyhow::anyhow!("Invalid endpoint_host: {:?}", e))?,
            endpoint_port: self.address.port(),
            vendor: String::new().try_into()
                .map_err(|e| anyhow::anyhow!("Invalid vendor: {:?}", e))?,
            hardware_version: String::new().try_into()
                .map_err(|e| anyhow::anyhow!("Invalid hardware_version: {:?}", e))?,
            firmware: String::new().try_into()
                .map_err(|e| anyhow::anyhow!("Invalid firmware: {:?}", e))?,
            device_id: config.worker.clone().try_into()
                .map_err(|e| anyhow::anyhow!("Invalid device_id: {:?}", e))?,
        };

        // Send message
        let frame: StdFrame = MiningDeviceMessages::Common(CommonMessages::SetupConnection(setup))
            .try_into()
            .map_err(|e| anyhow::anyhow!("Failed to encode SetupConnection: {:?}", e))?;

        self.sender
            .send(frame.into())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send SetupConnection: {:?}", e))?;

        // Wait for SetupConnectionSuccess
        let mut response: StdFrame = self
            .receiver
            .recv()
            .await
            .context("Connection closed while waiting for SetupConnectionSuccess")?
            .try_into()
            .map_err(|e| anyhow::anyhow!("Invalid frame type: {:?}", e))?;

        // Parse response using TryFrom
        let header = response.get_header().context("Missing frame header")?;
        let message_type = header.msg_type();
        let mut payload = response.payload().to_vec();

        // Try to decode as CommonMessages
        let common_msg: CommonMessages = (message_type, payload.as_mut_slice())
            .try_into()
            .map_err(|e| anyhow::anyhow!("Failed to parse Common message: {:?}", e))?;

        match common_msg {
            CommonMessages::SetupConnectionSuccess(success) => {
                info!(
                    "SetupConnectionSuccess: version={}, flags={:b}",
                    success.used_version, success.flags
                );
                Ok(())
            }
            CommonMessages::SetupConnectionError(_) => {
                bail!("Pool rejected SetupConnection");
            }
            other => {
                bail!("Unexpected common message: {:?}", other);
            }
        }
    }

    /// Open StandardMiningChannel and return channel_id
    pub async fn open_standard_mining_channel(
        &mut self,
        config: &StratumV2Config,
    ) -> Result<u32> {
        info!("Opening StandardMiningChannel");

        // Build OpenStandardMiningChannel message
        // Use nominal hashrate = 1.0 TH/s as placeholder (real hashrate will be measured)
        let open_channel = OpenStandardMiningChannel {
            request_id: 0_u32.into(),
            user_identity: config.worker.clone().try_into()
                .map_err(|e| anyhow::anyhow!("Invalid user_identity: {:?}", e))?,
            nominal_hash_rate: 1_000_000_000_000.0, // 1 TH/s
            max_target: vec![0xFF_u8; 32].try_into()
                .map_err(|e| anyhow::anyhow!("Invalid max_target: {:?}", e))?, // Accept any difficulty
        };

        // Send message
        let frame: StdFrame = MiningDeviceMessages::Mining(Mining::OpenStandardMiningChannel(
            open_channel,
        ))
        .try_into()
        .map_err(|e| anyhow::anyhow!("Failed to encode OpenStandardMiningChannel: {:?}", e))?;

        self.sender
            .send(frame.into())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send OpenStandardMiningChannel: {:?}", e))?;

        // Wait for OpenStandardMiningChannelSuccess
        let mut response: StdFrame = self
            .receiver
            .recv()
            .await
            .context("Connection closed while waiting for OpenStandardMiningChannelSuccess")?
            .try_into()
            .map_err(|e| anyhow::anyhow!("Invalid frame type: {:?}", e))?;

        // Parse response using TryFrom
        let header = response.get_header().context("Missing frame header")?;
        let message_type = header.msg_type();
        let mut payload = response.payload().to_vec();

        // Try to decode as Mining message
        let mining_msg: Mining = (message_type, payload.as_mut_slice())
            .try_into()
            .map_err(|e| anyhow::anyhow!("Failed to parse Mining message: {:?}", e))?;

        match mining_msg {
            Mining::OpenStandardMiningChannelSuccess(success) => {
                let channel_id = success.channel_id;
                info!(
                    "StandardMiningChannel opened: channel_id={}, group_id={}, request_id={}",
                    channel_id,
                    success.group_channel_id,
                    success.get_request_id_as_u32()
                );

                Ok(channel_id)
            }
            Mining::OpenMiningChannelError(_) => {
                bail!("Pool rejected OpenStandardMiningChannel");
            }
            other => {
                bail!("Unexpected mining message: {:?}", other);
            }
        }
    }

    /// Receive next message from pool
    pub async fn next_message(&mut self) -> Result<StdFrame> {
        let frame = self
            .receiver
            .recv()
            .await
            .context("Connection closed")?;

        frame.try_into().map_err(|e| anyhow::anyhow!("Invalid frame type: {:?}", e))
    }

    /// Send message to pool
    pub async fn send_message(&mut self, msg: Mining<'static>) -> Result<()> {
        let frame: StdFrame = MiningDeviceMessages::Mining(msg)
            .try_into()
            .map_err(|e| anyhow::anyhow!("Failed to encode message: {:?}", e))?;

        self.sender
            .send(frame.into())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send message: {:?}", e))
    }

    /// Submit share to pool
    pub async fn submit_share(&mut self, share: SubmitSharesStandard) -> Result<()> {
        debug!(
            "Submitting share: channel_id={}, job_id={}, nonce={:#x}",
            share.channel_id, share.job_id, share.nonce
        );

        self.send_message(Mining::SubmitSharesStandard(share))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_url() {
        // Valid URL
        let addr = StratumV2Client::parse_url("sv2+tcp://127.0.0.1:3333").unwrap();
        assert_eq!(addr.port(), 3333);

        // Invalid prefix
        assert!(StratumV2Client::parse_url("stratum+tcp://127.0.0.1:3333").is_err());

        // Invalid address
        assert!(StratumV2Client::parse_url("sv2+tcp://invalid").is_err());
    }
}
