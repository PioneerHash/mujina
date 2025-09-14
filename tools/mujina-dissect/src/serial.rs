//! Serial frame assembly for BM13xx protocol.

use crate::capture::{Channel, SerialEvent};
use anyhow::Result;
use std::collections::VecDeque;

/// Direction of serial communication
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Host to ASIC (CI channel)
    HostToChip,
    /// ASIC to Host (RO channel)
    ChipToHost,
}

impl From<Channel> for Direction {
    fn from(channel: Channel) -> Self {
        match channel {
            Channel::CI => Direction::HostToChip,
            Channel::RO => Direction::ChipToHost,
        }
    }
}

/// Assembled serial frame
#[derive(Debug, Clone)]
pub struct SerialFrame {
    pub direction: Direction,
    pub start_time: f64,
    pub end_time: f64,
    pub data: Vec<u8>,
    pub has_errors: bool,
}

/// Frame assembly state
#[derive(Debug, Clone)]
enum AssemblyState {
    /// Waiting for frame start
    Idle,
    /// Found first preamble byte
    FoundFirst(f64), // timestamp
    /// Collecting frame data
    Collecting {
        start_time: f64,
        data: Vec<u8>,
        expected_len: Option<usize>,
    },
}

/// Frame assembler for a single channel
pub struct FrameAssembler {
    direction: Direction,
    state: AssemblyState,
    timeout_seconds: f64,
    last_event_time: f64,
}

impl FrameAssembler {
    /// Create a new frame assembler
    pub fn new(direction: Direction) -> Self {
        Self {
            direction,
            state: AssemblyState::Idle,
            timeout_seconds: 0.001, // 1ms timeout between bytes
            last_event_time: 0.0,
        }
    }

    /// Process a serial event and potentially output a frame
    pub fn process(&mut self, event: &SerialEvent) -> Option<SerialFrame> {
        // Check for timeout
        if event.timestamp - self.last_event_time > self.timeout_seconds {
            if let Some(frame) = self.timeout() {
                self.state = AssemblyState::Idle;
                self.last_event_time = event.timestamp;
                self.process_byte(event.data, event.timestamp, event.error.is_some());
                return Some(frame);
            }
        }

        self.last_event_time = event.timestamp;
        self.process_byte(event.data, event.timestamp, event.error.is_some())
    }

    /// Process a single byte
    fn process_byte(&mut self, byte: u8, timestamp: f64, has_error: bool) -> Option<SerialFrame> {
        println!(
            "DEBUG: {:?} processing byte 0x{:02x} at {:.6} (error: {})",
            self.direction, byte, timestamp, has_error
        );

        match &mut self.state {
            AssemblyState::Idle => {
                // Look for preamble start
                match self.direction {
                    Direction::HostToChip => {
                        if byte == 0x55 {
                            println!("DEBUG: Found first preamble byte for HostToChip");
                            self.state = AssemblyState::FoundFirst(timestamp);
                        }
                    }
                    Direction::ChipToHost => {
                        if byte == 0xAA {
                            println!("DEBUG: Found first preamble byte for ChipToHost");
                            self.state = AssemblyState::FoundFirst(timestamp);
                        }
                    }
                }
                None
            }
            AssemblyState::FoundFirst(start_time) => {
                // Check for second preamble byte
                let valid = match self.direction {
                    Direction::HostToChip => byte == 0xAA,
                    Direction::ChipToHost => byte == 0x55,
                };

                if valid {
                    println!("DEBUG: Found complete preamble, starting frame collection");
                    // Start collecting frame
                    self.state = AssemblyState::Collecting {
                        start_time: *start_time,
                        data: vec![
                            match self.direction {
                                Direction::HostToChip => 0x55,
                                Direction::ChipToHost => 0xAA,
                            },
                            byte,
                        ],
                        expected_len: None,
                    };
                    None
                } else {
                    println!("DEBUG: Invalid preamble sequence, going back to idle");
                    // Not a valid preamble, go back to idle
                    self.state = AssemblyState::Idle;
                    // Reprocess this byte in idle state
                    self.process_byte(byte, timestamp, has_error)
                }
            }
            AssemblyState::Collecting {
                start_time,
                data,
                expected_len,
            } => {
                data.push(byte);

                // For command frames, byte 3 is the length
                if self.direction == Direction::HostToChip
                    && data.len() == 4
                    && expected_len.is_none()
                {
                    println!(
                        "DEBUG: Setting expected length to {} for command frame",
                        byte
                    );
                    *expected_len = Some(byte as usize);
                }

                // Check if frame is complete
                let complete = match self.direction {
                    Direction::HostToChip => {
                        // Command frame: check against expected length
                        if let Some(len) = expected_len {
                            data.len() >= *len
                        } else {
                            false
                        }
                    }
                    Direction::ChipToHost => {
                        // Response frame: heuristic based on typical sizes
                        // Minimum response is 7 bytes (preamble + chip_id + reg + value + crc)
                        // Maximum reasonable size is ~20 bytes
                        // Common response lengths: 6, 7, 9, 10, 11
                        data.len() >= 7
                            && (data.len() >= 20 || matches!(data.len(), 6 | 7 | 9 | 10 | 11))
                    }
                };

                if complete {
                    println!(
                        "DEBUG: Frame complete! Length: {}, data: {:02x?}",
                        data.len(),
                        data
                    );
                    let frame = SerialFrame {
                        direction: self.direction,
                        start_time: *start_time,
                        end_time: timestamp,
                        data: data.clone(),
                        has_errors: has_error,
                    };
                    self.state = AssemblyState::Idle;
                    Some(frame)
                } else {
                    println!(
                        "DEBUG: Frame not complete yet, length: {}, expected: {:?}",
                        data.len(),
                        expected_len
                    );
                    None
                }
            }
        }
    }

    /// Handle timeout - return incomplete frame if any
    fn timeout(&mut self) -> Option<SerialFrame> {
        match &self.state {
            AssemblyState::Collecting {
                start_time, data, ..
            } => {
                let frame = SerialFrame {
                    direction: self.direction,
                    start_time: *start_time,
                    end_time: self.last_event_time,
                    data: data.clone(),
                    has_errors: true,
                };
                Some(frame)
            }
            _ => None,
        }
    }

    /// Flush any pending frame (call at end of capture)
    pub fn flush(&mut self) -> Option<SerialFrame> {
        self.timeout()
    }
}

/// Multi-channel frame assembler
pub struct MultiChannelAssembler {
    ci_assembler: FrameAssembler,
    ro_assembler: FrameAssembler,
    frames: VecDeque<SerialFrame>,
}

impl MultiChannelAssembler {
    pub fn new() -> Self {
        Self {
            ci_assembler: FrameAssembler::new(Direction::HostToChip),
            ro_assembler: FrameAssembler::new(Direction::ChipToHost),
            frames: VecDeque::new(),
        }
    }

    /// Process a serial event
    pub fn process(&mut self, event: &SerialEvent) {
        let assembler = match event.channel {
            Channel::CI => &mut self.ci_assembler,
            Channel::RO => &mut self.ro_assembler,
        };

        if let Some(frame) = assembler.process(event) {
            println!(
                "DEBUG: Assembled frame from {:?}: {} bytes",
                event.channel,
                frame.data.len()
            );
            self.frames.push_back(frame);
        }
    }

    /// Get next assembled frame
    pub fn next_frame(&mut self) -> Option<SerialFrame> {
        self.frames.pop_front()
    }

    /// Flush all pending frames
    pub fn flush(&mut self) {
        if let Some(frame) = self.ci_assembler.flush() {
            self.frames.push_back(frame);
        }
        if let Some(frame) = self.ro_assembler.flush() {
            self.frames.push_back(frame);
        }
    }
}
