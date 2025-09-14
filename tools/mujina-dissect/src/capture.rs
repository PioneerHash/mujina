//! Saleae Logic 2 CSV capture parsing.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

/// Raw event from Saleae Logic 2 CSV export
#[derive(Debug, Clone, Deserialize)]
pub struct RawEvent {
    pub name: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub start_time: f64,
    pub duration: f64,
    pub data: Option<String>,
    pub error: Option<String>,
    pub ack: Option<String>,
    pub address: Option<String>,
    pub read: Option<String>,
}

/// Parsed capture event
#[derive(Debug, Clone)]
pub enum CaptureEvent {
    Serial(SerialEvent),
    I2c(I2cEvent),
}

/// Serial channel event
#[derive(Debug, Clone)]
pub struct SerialEvent {
    pub channel: Channel,
    pub baud_rate: BaudRate,
    pub timestamp: f64,
    pub data: u8,
    pub error: Option<String>,
}

/// I2C bus event
#[derive(Debug, Clone)]
pub struct I2cEvent {
    pub event_type: I2cEventType,
    pub timestamp: f64,
    pub address: Option<u8>,
    pub data: Option<u8>,
    pub ack: bool,
    pub read: bool,
}

/// I2C event types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum I2cEventType {
    Start,
    Stop,
    Address,
    Data,
}

/// Serial channel identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    /// Command Input (host to ASIC)
    CI,
    /// Response Output (ASIC to host)
    RO,
}

/// Baud rate for serial channels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaudRate {
    Baud115200,
    Baud1M,
}

impl RawEvent {
    /// Parse raw event into typed capture event
    pub fn parse(&self) -> Result<Option<CaptureEvent>> {
        // Handle serial channels
        if self.name.starts_with("CI Async Serial") || self.name.starts_with("RO Async Serial") {
            let channel = if self.name.starts_with("CI") {
                Channel::CI
            } else {
                Channel::RO
            };

            let baud_rate = if self.name.contains("115k") {
                BaudRate::Baud115200
            } else if self.name.contains("1M") {
                BaudRate::Baud1M
            } else {
                return Ok(None); // Unknown baud rate
            };

            // Only process data events
            if self.event_type != "data" {
                return Ok(None);
            }

            // Parse hex data
            let data = if let Some(data_str) = &self.data {
                parse_hex_value(data_str)
                    .with_context(|| format!("Failed to parse data: {}", data_str))?
            } else {
                return Ok(None);
            };

            Ok(Some(CaptureEvent::Serial(SerialEvent {
                channel,
                baud_rate,
                timestamp: self.start_time,
                data,
                error: self.error.clone(),
            })))
        }
        // Handle I2C events
        else if self.name == "I2C" {
            let event_type = match self.event_type.as_str() {
                "start" => I2cEventType::Start,
                "stop" => I2cEventType::Stop,
                "address" => I2cEventType::Address,
                "data" => I2cEventType::Data,
                _ => return Ok(None),
            };

            let address = self.address.as_ref().and_then(|s| parse_hex_value(s).ok());

            let data = self.data.as_ref().and_then(|s| parse_hex_value(s).ok());

            let ack = self
                .ack
                .as_ref()
                .map(|s| s.to_lowercase() == "true")
                .unwrap_or(false);

            let read = self
                .read
                .as_ref()
                .map(|s| s.to_lowercase() == "true")
                .unwrap_or(false);

            Ok(Some(CaptureEvent::I2c(I2cEvent {
                event_type,
                timestamp: self.start_time,
                address,
                data,
                ack,
                read,
            })))
        } else {
            Ok(None)
        }
    }
}

/// Parse hex value from string (handles 0x prefix)
fn parse_hex_value(s: &str) -> Result<u8> {
    let s = s.trim();
    let s = if s.starts_with("0x") || s.starts_with("0X") {
        &s[2..]
    } else {
        s
    };
    u8::from_str_radix(s, 16).with_context(|| format!("Invalid hex value: {}", s))
}

/// CSV capture reader
pub struct CaptureReader {
    reader: csv::Reader<std::fs::File>,
}

impl CaptureReader {
    /// Open a CSV capture file
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = std::fs::File::open(path.as_ref())
            .with_context(|| format!("Failed to open capture file: {:?}", path.as_ref()))?;
        let reader = csv::Reader::from_reader(file);
        Ok(Self { reader })
    }

    /// Read and parse events
    pub fn events(&mut self) -> impl Iterator<Item = Result<CaptureEvent>> + '_ {
        self.reader
            .deserialize::<RawEvent>()
            .filter_map(move |result| match result {
                Ok(raw_event) => match raw_event.parse() {
                    Ok(Some(event)) => Some(Ok(event)),
                    Ok(None) => None,
                    Err(e) => Some(Err(e)),
                },
                Err(e) => Some(Err(e.into())),
            })
    }
}

/// Filter for deduplicating serial events with framing errors
pub struct SerialDeduplicator {
    last_ci_events: Vec<(f64, BaudRate, bool)>, // (timestamp, baud_rate, has_error)
    last_ro_events: Vec<(f64, BaudRate, bool)>,
}

impl SerialDeduplicator {
    pub fn new() -> Self {
        Self {
            last_ci_events: Vec::new(),
            last_ro_events: Vec::new(),
        }
    }

    /// Check if a serial event should be kept
    /// Returns false if this is a duplicate with framing error
    pub fn should_keep(&mut self, event: &SerialEvent) -> bool {
        println!(
            "DEBUG: Deduplicator checking {:?} {:?} 0x{:02x} at {:.6} (error: {:?})",
            event.channel, event.baud_rate, event.data, event.timestamp, event.error
        );
        let events = match event.channel {
            Channel::CI => &mut self.last_ci_events,
            Channel::RO => &mut self.last_ro_events,
        };

        let has_error = event.error.is_some();
        let timestamp = event.timestamp;

        // Check if we have a recent event with the same timestamp
        let same_time_events: Vec<_> = events
            .iter()
            .filter(|(t, _, _)| (*t - timestamp).abs() < 0.000001) // Within 1 microsecond
            .collect();

        if !same_time_events.is_empty() {
            println!(
                "DEBUG: Found {} events at similar timestamp",
                same_time_events.len()
            );

            // Check if we have a 115k event at this timestamp
            let has_115k = same_time_events
                .iter()
                .any(|(_, baud, _)| *baud == BaudRate::Baud115200);

            if has_115k && event.baud_rate == BaudRate::Baud1M {
                println!("DEBUG: Skipping 1M event because we have 115k at same time");
                return false;
            }

            // If this is 115k and we have 1M events, remove the 1M events
            if event.baud_rate == BaudRate::Baud115200 {
                let had_1m = same_time_events
                    .iter()
                    .any(|(_, baud, _)| *baud == BaudRate::Baud1M);
                if had_1m {
                    println!("DEBUG: Removing 1M events because this is 115k");
                    events.retain(|(t, baud, _)| {
                        (*t - timestamp).abs() >= 0.000001 || *baud == BaudRate::Baud115200
                    });
                }
            }
        }

        // Add this event to our tracking
        events.push((timestamp, event.baud_rate, has_error));

        // Keep only recent events (last 1000 to avoid memory growth)
        if events.len() > 1000 {
            events.drain(0..500);
        }

        true
    }
}
