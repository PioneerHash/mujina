//! Serial frame assembly for BM13xx protocol.
//!
//! TODO: Build unit tests for frame assembly using known serial captures
//! - Test 88-byte work frame assembly with correct timeouts
//! - Test 11-byte response frame assembly with proper boundaries
//! - Test frame assembly edge cases (timeouts, incomplete frames, errors)
//! - Use timing data from actual captures to validate assembly logic

use crate::capture::{Channel, SerialEvent};

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
            timeout_seconds: 0.005, // 5ms timeout between bytes (response frames may need more time)
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
        match &mut self.state {
            AssemblyState::Idle => {
                // Look for preamble start
                match self.direction {
                    Direction::HostToChip => {
                        if byte == 0x55 {
                            self.state = AssemblyState::FoundFirst(timestamp);
                        }
                    }
                    Direction::ChipToHost => {
                        if byte == 0xAA {
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
                    *expected_len = Some(byte as usize);
                }

                // Check if frame is complete
                let complete = match self.direction {
                    Direction::HostToChip => {
                        // Command frame: check against expected length
                        // Length field is from type byte to end (includes CRC, excludes preamble)
                        // Total frame = 2 (preamble) + length
                        if let Some(len) = expected_len {
                            data.len() >= 2 + *len
                        } else {
                            false
                        }
                    }
                    Direction::ChipToHost => {
                        // Response frame: should be 11 bytes according to protocol docs
                        // Format: preamble(2) + reg_value(4) + chip_addr(1) + reg_addr(1) + unknown(2) + crc5(1) = 11 bytes
                        data.len() >= 11
                    }
                };

                if complete {
                    let frame = SerialFrame {
                        direction: self.direction,
                        start_time: *start_time,
                        data: data.clone(),
                        has_errors: has_error,
                    };
                    self.state = AssemblyState::Idle;
                    Some(frame)
                } else {
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
