//! BM13xx protocol codec wrapper for dissecting captured serial data.
//!
//! This module wraps the driver's FrameCodec to dissect serial frames from
//! captured logic analyzer data. It feeds raw bytes to the same codec used
//! during runtime to ensure consistency.

use crate::capture::{BaudRate, Channel, SerialEvent};
use bytes::{Buf, BytesMut};
use mujina_miner::asic::bm13xx::{
    crc::{crc16, crc5, crc5_is_valid},
    error::ProtocolError,
    protocol::{Command, FrameCodec, JobFullFormat, Register, RegisterAddress, Response},
};
use mujina_miner::tracing::prelude::*;
use std::collections::VecDeque;
use std::io;
use tokio_util::codec::Decoder;

/// Trait for streaming parsers
pub trait StreamingParser {
    fn process_event(&mut self, event: &SerialEvent) -> Box<dyn Iterator<Item = ParsedItem> + '_>;
}

/// Direction of serial communication
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// Parsed item from streaming parser
#[derive(Debug)]
pub enum ParsedItem {
    ValidFrame {
        command: Command,
        raw_bytes: Vec<u8>,
        timestamps: Vec<f64>,
    },
    ValidResponse {
        response: Response,
        raw_bytes: Vec<u8>,
        timestamps: Vec<f64>,
    },
    InvalidBytes {
        raw_bytes: Vec<u8>,
        timestamps: Vec<f64>,
        reason: String,
    },
}

/// Decoded frame with timing information (legacy)
#[derive(Debug)]
pub enum DecodedFrame {
    Command {
        timestamp: f64,
        command: Command,
        raw_bytes: Vec<u8>,
        has_errors: bool,
        baud_rate: BaudRate,
    },
    Response {
        timestamp: f64,
        response: Response,
        raw_bytes: Vec<u8>,
        has_errors: bool,
        baud_rate: BaudRate,
    },
    Error {
        timestamp: f64,
        error: String,
        raw_bytes: Vec<u8>,
        baud_rate: BaudRate,
    },
}

/// Streaming command parser for CI (host-to-chip) streams
pub struct CommandStreamingParser {
    buffer: BytesMut,
    byte_queue: VecDeque<(u8, f64)>, // (byte, timestamp)
    invalid_accumulator: Vec<(u8, f64)>,
}

impl CommandStreamingParser {
    pub fn new() -> Self {
        Self {
            buffer: BytesMut::new(),
            byte_queue: VecDeque::new(),
            invalid_accumulator: Vec::new(),
        }
    }

    /// Process a single serial event and yield any completed frames
    pub fn process_event(&mut self, event: &SerialEvent) -> impl Iterator<Item = ParsedItem> + '_ {
        // Add byte to queue with timestamp - skip events with framing errors
        if event.error.is_none() {
            self.byte_queue.push_back((event.data, event.timestamp));
        }

        // Try to extract frames from the queue
        std::iter::from_fn(move || self.try_extract_frame())
    }

    fn try_extract_frame(&mut self) -> Option<ParsedItem> {
        const PREAMBLE: [u8; 2] = [0x55, 0xaa];

        // Convert queue to buffer for parsing
        self.sync_buffer_from_queue();

        if self.buffer.len() < 5 {
            return None; // Need more data
        }

        // Check for valid preamble
        if self.buffer[0] != PREAMBLE[0] || self.buffer[1] != PREAMBLE[1] {
            // Invalid byte - accumulate it
            if let Some((byte, timestamp)) = self.byte_queue.pop_front() {
                self.invalid_accumulator.push((byte, timestamp));
                self.buffer.advance(1);
            }

            // Check if we should flush accumulated invalid bytes
            if !self.invalid_accumulator.is_empty() && self.looks_like_frame_start() {
                return self.flush_invalid_bytes();
            }

            return None;
        }

        // Flush any accumulated invalid bytes first
        if !self.invalid_accumulator.is_empty() {
            return self.flush_invalid_bytes();
        }

        // Try to parse a valid frame
        let frame_length = self.buffer[3] as usize;
        let total_length = 2 + frame_length;

        if self.buffer.len() < total_length {
            return None; // Need more data
        }

        // Extract frame data and timestamps
        let frame_bytes = self.buffer[..total_length].to_vec();
        let frame_timestamps: Vec<f64> = self
            .byte_queue
            .drain(..total_length)
            .map(|(_, ts)| ts)
            .collect();

        self.buffer.advance(total_length);

        // Parse the command frame
        match parse_bm13xx_command_frame(&frame_bytes) {
            Ok(command) => Some(ParsedItem::ValidFrame {
                command,
                raw_bytes: frame_bytes,
                timestamps: frame_timestamps,
            }),
            Err(_) => Some(ParsedItem::InvalidBytes {
                raw_bytes: frame_bytes,
                timestamps: frame_timestamps,
                reason: "crc_failed".to_string(),
            }),
        }
    }

    fn sync_buffer_from_queue(&mut self) {
        // Ensure buffer matches the current queue state
        let queue_bytes: Vec<u8> = self.byte_queue.iter().map(|(b, _)| *b).collect();
        if queue_bytes != self.buffer.as_ref() {
            self.buffer.clear();
            self.buffer.extend_from_slice(&queue_bytes);
        }
    }

    fn looks_like_frame_start(&self) -> bool {
        self.buffer.len() >= 2 && self.buffer[0] == 0x55 && self.buffer[1] == 0xaa
    }

    fn flush_invalid_bytes(&mut self) -> Option<ParsedItem> {
        if self.invalid_accumulator.is_empty() {
            return None;
        }

        let (bytes, timestamps): (Vec<u8>, Vec<f64>) = self.invalid_accumulator.drain(..).unzip();
        Some(ParsedItem::InvalidBytes {
            raw_bytes: bytes,
            timestamps,
            reason: "invalid_preamble".to_string(),
        })
    }
}

/// Streaming response parser for RO (chip-to-host) streams
pub struct ResponseStreamingParser {
    buffer: BytesMut,
    byte_queue: VecDeque<(u8, f64)>, // (byte, timestamp)
    invalid_accumulator: Vec<(u8, f64)>,
    frame_codec: FrameCodec, // Reuse existing response decoder
}

impl StreamingParser for ResponseStreamingParser {
    fn process_event(&mut self, event: &SerialEvent) -> Box<dyn Iterator<Item = ParsedItem> + '_> {
        // Add byte to queue with timestamp - skip events with framing errors
        if event.error.is_none() {
            self.byte_queue.push_back((event.data, event.timestamp));
        }

        // Try to extract frames from the queue
        Box::new(std::iter::from_fn(move || self.try_extract_frame()))
    }
}

impl ResponseStreamingParser {
    pub fn new() -> Self {
        Self {
            buffer: BytesMut::new(),
            byte_queue: VecDeque::new(),
            invalid_accumulator: Vec::new(),
            frame_codec: FrameCodec::default(),
        }
    }

    fn try_extract_frame(&mut self) -> Option<ParsedItem> {
        // Convert queue to buffer for parsing
        self.sync_buffer_from_queue();

        if self.buffer.len() < 3 {
            return None; // Need more data for minimum response frame
        }

        // Try to decode using existing FrameCodec
        let buffer_before = self.buffer.clone();
        match self.frame_codec.decode(&mut self.buffer) {
            Ok(Some(response)) => {
                let consumed_bytes = buffer_before.len() - self.buffer.len();
                let frame_bytes = buffer_before[..consumed_bytes].to_vec();
                let frame_timestamps: Vec<f64> = self
                    .byte_queue
                    .drain(..consumed_bytes)
                    .map(|(_, ts)| ts)
                    .collect();

                Some(ParsedItem::ValidResponse {
                    response,
                    raw_bytes: frame_bytes,
                    timestamps: frame_timestamps,
                })
            }
            Ok(None) => None, // Need more data
            Err(_) => {
                // Invalid byte - accumulate it
                if let Some((byte, timestamp)) = self.byte_queue.pop_front() {
                    self.invalid_accumulator.push((byte, timestamp));
                    self.buffer.advance(1);
                }

                // Flush invalid bytes if we see a potential frame start
                if !self.invalid_accumulator.is_empty() && self.looks_like_response_start() {
                    self.flush_invalid_bytes()
                } else {
                    None
                }
            }
        }
    }

    fn sync_buffer_from_queue(&mut self) {
        let queue_bytes: Vec<u8> = self.byte_queue.iter().map(|(b, _)| *b).collect();
        if queue_bytes != self.buffer.as_ref() {
            self.buffer.clear();
            self.buffer.extend_from_slice(&queue_bytes);
        }
    }

    fn looks_like_response_start(&self) -> bool {
        self.buffer.len() >= 2 && self.buffer[0] == 0xaa && self.buffer[1] == 0x55
    }

    fn flush_invalid_bytes(&mut self) -> Option<ParsedItem> {
        if self.invalid_accumulator.is_empty() {
            return None;
        }

        let (bytes, timestamps): (Vec<u8>, Vec<f64>) = self.invalid_accumulator.drain(..).unzip();
        Some(ParsedItem::InvalidBytes {
            raw_bytes: bytes,
            timestamps,
            reason: "invalid_response".to_string(),
        })
    }
}

/// Standalone BM13xx command frame parser (shared between old and new implementations)
fn parse_bm13xx_command_frame(data: &[u8]) -> Result<Command, ProtocolError> {
    if data.len() < 5 {
        return Err(ProtocolError::InvalidFrame);
    }

    // data[0..2] is preamble (already validated)
    let type_flags = data[2];
    let _length = data[3] as usize;

    // Parse type flags according to protocol documentation
    let is_work = (type_flags & 0x40) == 0;
    let is_broadcast = (type_flags & 0x10) != 0;
    let cmd = type_flags & 0x0f;

    // Validate CRC
    let crc_valid = if is_work {
        // Work frames use CRC16
        if data.len() >= 4 {
            let payload_end = data.len() - 2;
            let crc_bytes = &data[payload_end..];
            let payload = &data[2..payload_end];
            let expected_crc = u16::from_be_bytes([crc_bytes[0], crc_bytes[1]]);
            let calculated_crc = crc16(payload);
            calculated_crc == expected_crc
        } else {
            false
        }
    } else {
        // Register frames use CRC5
        if data.len() >= 3 {
            let payload = &data[2..data.len() - 1];
            let expected_crc = data[data.len() - 1];
            let calculated_crc = crc5(payload);
            calculated_crc == expected_crc
        } else {
            false
        }
    };

    if !crc_valid {
        return Err(ProtocolError::InvalidFrame);
    }

    if is_work {
        // Parse work frame (JobFull)
        let job_data_len = _length - 4;
        if job_data_len == 82 && data.len() >= 2 + _length {
            let job_data_bytes = &data[4..(4 + 82)];
            let job_data = JobFullFormat {
                job_id: job_data_bytes[0],
                num_midstates: job_data_bytes[1],
                starting_nonce: job_data_bytes[2..6].try_into().unwrap(),
                nbits: job_data_bytes[6..10].try_into().unwrap(),
                ntime: job_data_bytes[10..14].try_into().unwrap(),
                merkle_root: job_data_bytes[14..46].try_into().unwrap(),
                prev_block_hash: job_data_bytes[46..78].try_into().unwrap(),
                version: job_data_bytes[78..82].try_into().unwrap(),
            };
            return Ok(Command::JobFull { job_data });
        } else {
            return Err(ProtocolError::InvalidFrame);
        }
    }

    // Parse register commands
    let command = match (cmd, is_broadcast) {
        (0, false) => Command::SetChipAddress {
            chip_address: data[4],
        },
        (1, true) => {
            let register_address =
                RegisterAddress::from_repr(data[5]).ok_or(ProtocolError::InvalidFrame)?;
            let register_value = [data[6], data[7], data[8], data[9]];
            let register = Register::decode(register_address, &register_value);
            Command::WriteRegister {
                all: true,
                chip_address: 0,
                register,
            }
        }
        (1, false) => {
            let register_address =
                RegisterAddress::from_repr(data[5]).ok_or(ProtocolError::InvalidFrame)?;
            let register_value = [data[6], data[7], data[8], data[9]];
            let register = Register::decode(register_address, &register_value);
            Command::WriteRegister {
                all: false,
                chip_address: data[4],
                register,
            }
        }
        (2, true) => {
            let register_address =
                RegisterAddress::from_repr(data[5]).ok_or(ProtocolError::InvalidFrame)?;
            Command::ReadRegister {
                all: true,
                chip_address: 0,
                register_address,
            }
        }
        (2, false) => {
            let register_address =
                RegisterAddress::from_repr(data[5]).ok_or(ProtocolError::InvalidFrame)?;
            Command::ReadRegister {
                all: false,
                chip_address: data[4],
                register_address,
            }
        }
        _ => return Err(ProtocolError::InvalidFrame),
    };

    Ok(command)
}

impl DecodedFrame {
    pub fn timestamp(&self) -> f64 {
        match self {
            DecodedFrame::Command { timestamp, .. } => *timestamp,
            DecodedFrame::Response { timestamp, .. } => *timestamp,
            DecodedFrame::Error { timestamp, .. } => *timestamp,
        }
    }

    pub fn direction(&self) -> Direction {
        match self {
            DecodedFrame::Command { .. } => Direction::HostToChip,
            DecodedFrame::Response { .. } => Direction::ChipToHost,
            DecodedFrame::Error { .. } => Direction::HostToChip, // Default, could be either
        }
    }

    pub fn baud_rate(&self) -> BaudRate {
        match self {
            DecodedFrame::Command { baud_rate, .. } => *baud_rate,
            DecodedFrame::Response { baud_rate, .. } => *baud_rate,
            DecodedFrame::Error { baud_rate, .. } => *baud_rate,
        }
    }
}

/// Codec wrapper that tracks timing and handles both directions
pub struct TimestampedCodec {
    direction: Direction,
    response_codec: FrameCodec,
    command_codec: CommandDecoder,
    buffer: BytesMut,
    // Track byte timestamps parallel to buffer
    byte_timestamps: Vec<f64>,
    byte_errors: Vec<bool>,
}

impl TimestampedCodec {
    /// Create a new timestamped codec for the given direction
    pub fn new(direction: Direction) -> Self {
        Self {
            direction,
            response_codec: FrameCodec::default(),
            command_codec: CommandDecoder::default(),
            buffer: BytesMut::new(),
            byte_timestamps: Vec::new(),
            byte_errors: Vec::new(),
        }
    }

    /// Feed a serial event to the codec and get any decoded frames
    pub fn feed_event(&mut self, event: &SerialEvent, baud_rate: BaudRate) -> Vec<DecodedFrame> {
        let mut results = Vec::new();

        // Don't flush discarded bytes based on time - only flush when valid frame is found
        // This ensures all consecutive invalid bytes are grouped together

        // Add byte to buffer and timestamp tracking
        self.buffer.extend_from_slice(&[event.data]);
        self.byte_timestamps.push(event.timestamp);
        self.byte_errors.push(event.error.is_some());

        // Removed debug output

        // Try to decode frames from the buffer
        loop {
            // Capture buffer state before decoding
            let buffer_before = self.buffer.clone();

            match self.direction {
                Direction::HostToChip => {
                    // Use CommandDecoder for command frames
                    match self.command_codec.decode(&mut self.buffer) {
                        Ok(Some(command)) => {
                            let consumed_bytes = buffer_before.len() - self.buffer.len();
                            let frame_bytes = buffer_before[..consumed_bytes].to_vec();

                            // Removed debug output

                            // Update timestamp tracking
                            let frame_timestamps = self
                                .byte_timestamps
                                .drain(..consumed_bytes)
                                .collect::<Vec<_>>();
                            let frame_errors =
                                self.byte_errors.drain(..consumed_bytes).collect::<Vec<_>>();

                            // Check if any bytes in this frame had framing errors - if so, silently reject
                            let has_errors = frame_errors.iter().any(|&e| e);
                            if !has_errors {
                                let frame = DecodedFrame::Command {
                                    timestamp: frame_timestamps
                                        .last()
                                        .copied()
                                        .unwrap_or(event.timestamp),
                                    command,
                                    raw_bytes: frame_bytes,
                                    has_errors: false,
                                    baud_rate,
                                };
                                results.push(frame);
                            }
                            // If frame has errors, silently discard it - don't report anything
                        }
                        Ok(None) => {
                            // Need more data - restore timestamp tracking
                            break;
                        }
                        Err(_e) => {
                            // Decoder advanced by 1 byte (standard behavior) - silently continue
                            let consumed_bytes = buffer_before.len() - self.buffer.len();
                            if consumed_bytes > 0 {
                                // CRITICAL: Must drain timestamps/errors to keep vectors synchronized with buffer
                                self.byte_timestamps.drain(..consumed_bytes);
                                self.byte_errors.drain(..consumed_bytes);
                            }
                        }
                    }

                    // Ensure tracking vectors stay synchronized with buffer length
                    let buffer_len = self.buffer.len();
                    if self.byte_errors.len() != buffer_len {
                        self.byte_errors.truncate(buffer_len);
                        self.byte_timestamps.truncate(buffer_len);
                    }
                }
                Direction::ChipToHost => {
                    // Use FrameCodec for response frames
                    match self.response_codec.decode(&mut self.buffer) {
                        Ok(Some(response)) => {
                            let consumed_bytes = buffer_before.len() - self.buffer.len();
                            let frame_bytes = buffer_before[..consumed_bytes].to_vec();

                            // Update timestamp tracking
                            let frame_timestamps = self
                                .byte_timestamps
                                .drain(..consumed_bytes)
                                .collect::<Vec<_>>();
                            let frame_errors =
                                self.byte_errors.drain(..consumed_bytes).collect::<Vec<_>>();

                            // Check if any bytes in this frame had framing errors - if so, silently reject
                            let has_errors = frame_errors.iter().any(|&e| e);
                            if !has_errors {
                                let frame = DecodedFrame::Response {
                                    timestamp: frame_timestamps
                                        .last()
                                        .copied()
                                        .unwrap_or(event.timestamp),
                                    response,
                                    raw_bytes: frame_bytes,
                                    has_errors: false,
                                    baud_rate,
                                };
                                results.push(frame);
                            }
                            // If frame has errors, silently discard it - don't report anything
                        }
                        Ok(None) => {
                            // Decoder either needs more data or advanced buffer
                            let consumed_bytes = buffer_before.len() - self.buffer.len();
                            if consumed_bytes > 0 {
                                // Decoder discarded bytes - silently consume tracking data
                                self.byte_timestamps.drain(..consumed_bytes);
                                self.byte_errors.drain(..consumed_bytes);
                            } else {
                                // Actually need more data
                                break;
                            }
                        }
                        Err(_e) => {
                            // Decoder advanced by 1 byte (standard behavior) - silently continue
                            let consumed_bytes = buffer_before.len() - self.buffer.len();
                            if consumed_bytes > 0 {
                                // Just consume the timestamps/errors and discard silently
                                self.byte_timestamps.drain(..consumed_bytes);
                                self.byte_errors.drain(..consumed_bytes);
                            }
                        }
                    }

                    // Ensure tracking vectors stay synchronized with buffer length
                    let buffer_len = self.buffer.len();
                    if self.byte_errors.len() != buffer_len {
                        self.byte_errors.truncate(buffer_len);
                        self.byte_timestamps.truncate(buffer_len);
                    }
                }
            }

            // Safety check - if buffer didn't change, break to avoid infinite loop
            if self.buffer.len() == buffer_before.len() {
                break;
            }
        }

        results
    }

    /// Flush any remaining data at end of stream
    pub fn flush(&mut self) -> Vec<DecodedFrame> {
        // Simply discard any remaining incomplete data - don't report anything
        self.buffer.clear();
        self.byte_timestamps.clear();
        self.byte_errors.clear();
        Vec::new()
    }
}

/// Command decoder for dissection purposes
///
/// Unlike FrameCodec which decodes responses, this decodes command frames
/// with variable lengths and proper broadcast write register parsing.
#[derive(Default)]
pub struct CommandDecoder {
    last_buffer_size: usize,
}

impl Decoder for CommandDecoder {
    type Item = Command;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        const PREAMBLE: [u8; 2] = [0x55, 0xaa];
        const MIN_FRAME_LEN: usize = 5; // Minimum command frame size
                                        // Return Ok(None) to request more data

        // Log significant buffer changes
        if src.len() != self.last_buffer_size {
            if src.len() > self.last_buffer_size + 5
                || (self.last_buffer_size >= MIN_FRAME_LEN && src.len() < MIN_FRAME_LEN)
            {
                trace!(
                    "Command decoder buffer: {} → {} bytes ({})",
                    self.last_buffer_size,
                    src.len(),
                    if src.len() > self.last_buffer_size {
                        "growing"
                    } else {
                        "shrinking"
                    }
                );
            }
            self.last_buffer_size = src.len();
        }

        if src.len() < MIN_FRAME_LEN {
            return Ok(None);
        }

        // Check preamble
        if src[0] != PREAMBLE[0] {
            src.advance(1);
            return Ok(None);
        }

        if src[1] != PREAMBLE[1] {
            src.advance(1);
            return Ok(None);
        }

        // Get frame length from length field
        if src.len() < 4 {
            return Ok(None);
        }

        let frame_length = src[3] as usize;
        let total_length = 2 + frame_length; // preamble + frame

        if src.len() < total_length {
            return Ok(None); // Need more data
        }

        // Extract frame data for parsing
        let frame_data = src[..total_length].to_vec();

        // Parse the command frame
        match self.parse_command_frame(&frame_data) {
            Ok(command) => {
                // Only advance if parse was successful
                src.advance(total_length);

                trace!(
                    "TX: {:?} ({} bytes) => {:02x?}",
                    command,
                    total_length,
                    frame_data
                );
                Ok(Some(command))
            }
            Err(err) => {
                trace!("Failed to decode command: {} => {:02x?}", err, frame_data);
                // Advance by 1 byte and continue looking (same pattern as FrameCodec)
                src.advance(1);
                Ok(None)
            }
        }
    }
}

impl CommandDecoder {
    /// Parse a command frame with proper broadcast write register handling
    fn parse_command_frame(&self, data: &[u8]) -> Result<Command, ProtocolError> {
        parse_bm13xx_command_frame(data)
    }
}

// Keep the old implementation for now
impl CommandDecoder {
    /// Legacy parse implementation
    fn parse_command_frame_old(&self, data: &[u8]) -> Result<Command, ProtocolError> {
        // Debug output removed
        if data.len() < 5 {
            return Err(ProtocolError::InvalidFrame);
        }

        // data[0..2] is preamble (already validated)
        let type_flags = data[2];
        let _length = data[3] as usize;

        // Parse type flags according to protocol documentation
        let is_work = (type_flags & 0x40) == 0;
        let is_broadcast = (type_flags & 0x10) != 0;
        let cmd = type_flags & 0x0f; // Bits 3-0, not 4-0!

        // Validate CRC
        let crc_valid = if is_work {
            // Work frames use CRC16
            if data.len() >= 4 {
                let payload_end = data.len() - 2;
                let crc_bytes = &data[payload_end..];
                let payload = &data[2..payload_end];
                let expected_crc = u16::from_be_bytes([crc_bytes[0], crc_bytes[1]]);
                let calculated_crc = crc16(payload);
                calculated_crc == expected_crc
            } else {
                false
            }
        } else {
            // Register frames use CRC5 - calculate and compare (not crc5_is_valid)
            if data.len() >= 3 {
                let payload = &data[2..data.len() - 1]; // Exclude preamble and CRC byte
                let expected_crc = data[data.len() - 1];
                let calculated_crc = crc5(payload);
                calculated_crc == expected_crc
            } else {
                false
            }
        };

        if !crc_valid {
            return Err(ProtocolError::InvalidFrame);
        }

        if is_work {
            // Parse work frame (JobFull)
            let job_data_len = _length - 4;
            if job_data_len == 82 && data.len() >= 2 + _length {
                let job_data_bytes = &data[4..(4 + 82)];
                let job_data = JobFullFormat {
                    job_id: job_data_bytes[0],
                    num_midstates: job_data_bytes[1],
                    starting_nonce: job_data_bytes[2..6].try_into().unwrap(),
                    nbits: job_data_bytes[6..10].try_into().unwrap(),
                    ntime: job_data_bytes[10..14].try_into().unwrap(),
                    merkle_root: job_data_bytes[14..46].try_into().unwrap(),
                    prev_block_hash: job_data_bytes[46..78].try_into().unwrap(),
                    version: job_data_bytes[78..82].try_into().unwrap(),
                };
                return Ok(Command::JobFull { job_data });
            } else {
                return Err(ProtocolError::InvalidFrame);
            }
        }

        // Parse register commands - with CORRECTED broadcast write register parsing
        let command = match (cmd, is_broadcast) {
            (0, false) => Command::SetChipAddress {
                chip_address: data[4],
            },
            (1, false) => {
                // Non-broadcast write register: chip_addr + reg_addr + data[4]
                if data.len() >= 10 {
                    let chip_address = data[4];
                    let reg_addr = RegisterAddress::from_repr(data[5])
                        .ok_or(ProtocolError::InvalidRegisterAddress(data[5]))?;
                    let value_bytes: [u8; 4] = data[6..10].try_into().unwrap();
                    let register = Register::decode(reg_addr, &value_bytes);
                    Command::WriteRegister {
                        all: false,
                        chip_address,
                        register,
                    }
                } else {
                    return Err(ProtocolError::InvalidFrame);
                }
            }
            (2, false) => {
                // Non-broadcast read register: chip_addr + reg_addr
                if data.len() >= 6 {
                    let chip_address = data[4];
                    let reg_addr = RegisterAddress::from_repr(data[5])
                        .ok_or(ProtocolError::InvalidRegisterAddress(data[5]))?;
                    Command::ReadRegister {
                        all: false,
                        chip_address,
                        register_address: reg_addr,
                    }
                } else {
                    return Err(ProtocolError::InvalidFrame);
                }
            }
            (1, true) => {
                // CORRECTED: Broadcast write register: chip_addr(0x00) + reg_addr + data[4]
                // Protocol doc: | 0x55 0xAA | Type/Flags | Length | Chip_Addr | Reg_Addr | Data[4] | CRC5 |
                // Broadcast write register case
                if data.len() >= 10 {
                    let chip_address = data[4]; // Should be 0x00 for broadcast
                    let reg_addr = RegisterAddress::from_repr(data[5])
                        .ok_or(ProtocolError::InvalidRegisterAddress(data[5]))?;
                    let value_bytes: [u8; 4] = data[6..10].try_into().unwrap();
                    let register = Register::decode(reg_addr, &value_bytes);
                    // Successfully parsed broadcast WriteRegister
                    Command::WriteRegister {
                        all: true,
                        chip_address,
                        register,
                    }
                } else {
                    // Frame too short for broadcast write register
                    return Err(ProtocolError::InvalidFrame);
                }
            }
            (2, true) => {
                // CORRECTED: Broadcast read register: chip_addr(0x00) + reg_addr
                if data.len() >= 6 {
                    let chip_address = data[4]; // Should be 0x00 for broadcast
                    let reg_addr = RegisterAddress::from_repr(data[5])
                        .ok_or(ProtocolError::InvalidRegisterAddress(data[5]))?;
                    Command::ReadRegister {
                        all: true,
                        chip_address,
                        register_address: reg_addr,
                    }
                } else {
                    return Err(ProtocolError::InvalidFrame);
                }
            }
            (3, false) => Command::ChainInactive,
            _ => return Err(ProtocolError::InvalidFrame),
        };

        Ok(command)
    }
}
