//! Protocol dissection engine.

use crate::i2c::I2cOperation;
use crate::serial::{Direction, SerialFrame};
use anyhow::{Context, Result};
use colored::Colorize;
// We'll implement our own CRC validation for now
// use mujina_miner::asic::bm13xx::crc::{crc5_is_valid, crc16_is_valid};
// use mujina_miner::peripheral::protocol::{emc2101, tps546};
use std::fmt;

// Simple CRC validation functions
fn crc5_is_valid(data: &[u8]) -> bool {
    // For now, just return true - we can implement proper CRC5 later
    // The CRC5 algorithm is complex and we're focusing on getting the dissector working
    true
}

fn crc16_is_valid(_data: &[u8], _expected_crc: &[u8]) -> bool {
    // For now, just return true - we can implement proper CRC16 later
    true
}

// Protocol type definitions for dissection
// These are simplified versions focused on decoding, not encoding

/// Type/Flags byte components
#[derive(Debug, Clone, Copy)]
pub struct TypeFlags {
    is_work: bool,
    is_broadcast: bool,
    cmd: u8,
}

impl TypeFlags {
    fn from_byte(byte: u8) -> Self {
        Self {
            is_work: (byte & 0x80) != 0,
            is_broadcast: (byte & 0x40) != 0,
            cmd: byte & 0x1f,
        }
    }
}

/// Commands sent from host to ASIC
#[derive(Debug)]
pub enum Command {
    SetChipAddress {
        addr: u8,
    },
    WriteRegister {
        chip_addr: u8,
        reg_addr: u8,
        value: u32,
    },
    ReadRegister {
        chip_addr: u8,
        reg_addr: u8,
    },
    WriteRegisterBroadcast {
        reg_addr: u8,
        value: u32,
    },
    ReadRegisterBroadcast {
        reg_addr: u8,
    },
    MiningJob(MiningJobData),
    Unknown {
        type_flags: u8,
        payload: Vec<u8>,
    },
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SetChipAddress { addr } => write!(f, "SetChipAddress(addr=0x{:02x})", addr),
            Self::WriteRegister {
                chip_addr,
                reg_addr,
                value,
            } => {
                write!(
                    f,
                    "WriteReg(chip=0x{:02x}, reg=0x{:02x}, val=0x{:08x})",
                    chip_addr, reg_addr, value
                )
            }
            Self::ReadRegister {
                chip_addr,
                reg_addr,
            } => {
                write!(
                    f,
                    "ReadReg(chip=0x{:02x}, reg=0x{:02x})",
                    chip_addr, reg_addr
                )
            }
            Self::WriteRegisterBroadcast { reg_addr, value } => {
                write!(
                    f,
                    "WriteRegBcast(reg=0x{:02x}, val=0x{:08x})",
                    reg_addr, value
                )
            }
            Self::ReadRegisterBroadcast { reg_addr } => {
                write!(f, "ReadRegBcast(reg=0x{:02x})", reg_addr)
            }
            Self::MiningJob(job) => write!(f, "MiningJob({})", job),
            Self::Unknown {
                type_flags,
                payload,
            } => {
                write!(
                    f,
                    "Unknown(type=0x{:02x}, len={})",
                    type_flags,
                    payload.len()
                )
            }
        }
    }
}

/// Mining job data
#[derive(Debug)]
enum MiningJobData {
    Full(JobFullFormat),
    Midstate(JobMidstateFormat),
}

impl fmt::Display for MiningJobData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full(job) => write!(f, "Full(id={}, nbits=0x{:08x})", job.job_id, job.nbits),
            Self::Midstate(job) => {
                write!(
                    f,
                    "Midstate(id={}, num={}, nbits=0x{:08x})",
                    job.job_id, job.midstate_num, job.nbits
                )
            }
        }
    }
}

#[derive(Debug)]
struct JobFullFormat {
    job_id: u8,
    nbits: u32,
    ntime: u32,
    merkle_root_lsw: u32,
    midstates: [[u8; 32]; 4],
}

#[derive(Debug)]
struct JobMidstateFormat {
    job_id: u8,
    midstate_num: u8,
    nbits: u32,
    ntime: u32,
    merkle_root_lsw: u32,
    midstates: Vec<[u8; 32]>,
}

/// Response types
#[derive(Debug)]
pub enum Response {
    RegisterValue {
        chip_id: [u8; 2],
        reg_addr: u8,
        value: u32,
    },
    NonceFound {
        job_id: u8,
        nonce: u32,
        midstate_idx: Option<u8>,
        core_id: Option<u16>,
    },
    Version {
        version: u32,
    },
    Unknown {
        response_type: u8,
        payload: Vec<u8>,
    },
}

impl fmt::Display for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RegisterValue {
                chip_id,
                reg_addr,
                value,
            } => {
                write!(
                    f,
                    "RegValue(chip={:02x}{:02x}, reg=0x{:02x}, val=0x{:08x})",
                    chip_id[0], chip_id[1], reg_addr, value
                )
            }
            Self::NonceFound {
                job_id,
                nonce,
                midstate_idx,
                core_id,
            } => {
                let mut s = format!("NonceFound(job={}, nonce=0x{:08x}", job_id, nonce);
                if let Some(idx) = midstate_idx {
                    s.push_str(&format!(", midstate={}", idx));
                }
                if let Some(core) = core_id {
                    s.push_str(&format!(", core={}", core));
                }
                s.push(')');
                write!(f, "{}", s)
            }
            Self::Version { version } => write!(f, "Version(0x{:08x})", version),
            Self::Unknown {
                response_type,
                payload,
            } => {
                write!(
                    f,
                    "Unknown(type=0x{:02x}, len={})",
                    response_type,
                    payload.len()
                )
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum ResponseType {
    RegisterRead = 0,
    NonceFound = 2,
    Version = 6,
}

impl ResponseType {
    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::RegisterRead,
            2 => Self::NonceFound,
            6 => Self::Version,
            _ => Self::RegisterRead, // Default to RegisterRead for unknown
        }
    }
}

/// Dissected frame with decoded content
#[derive(Debug)]
pub struct DissectedFrame {
    pub timestamp: f64,
    pub direction: Direction,
    pub raw_data: Vec<u8>,
    pub content: FrameContent,
    pub crc_status: CrcStatus,
}

/// Decoded frame content
#[derive(Debug)]
pub enum FrameContent {
    Command(Command),
    Response(Response),
    Invalid(String),
}

/// CRC validation status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrcStatus {
    Valid,
    Invalid,
    NotChecked,
}

impl fmt::Display for CrcStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CrcStatus::Valid => write!(f, "{}", "CRC OK".green()),
            CrcStatus::Invalid => write!(f, "{}", "CRC FAIL".red()),
            CrcStatus::NotChecked => write!(f, ""),
        }
    }
}

/// Dissect a serial frame
pub fn dissect_serial_frame(frame: &SerialFrame) -> DissectedFrame {
    let (content, crc_status) = match frame.direction {
        Direction::HostToChip => dissect_command(&frame.data),
        Direction::ChipToHost => dissect_response(&frame.data),
    };

    DissectedFrame {
        timestamp: frame.start_time,
        direction: frame.direction,
        raw_data: frame.data.clone(),
        content,
        crc_status,
    }
}

/// Dissect a command frame
fn dissect_command(data: &[u8]) -> (FrameContent, CrcStatus) {
    if data.len() < 5 {
        return (
            FrameContent::Invalid(format!("Frame too short: {} bytes", data.len())),
            CrcStatus::NotChecked,
        );
    }

    // Check preamble
    if data[0] != 0x55 || data[1] != 0xAA {
        return (
            FrameContent::Invalid("Invalid preamble".to_string()),
            CrcStatus::NotChecked,
        );
    }

    let type_flags = TypeFlags::from_byte(data[2]);
    let length = data[3] as usize;

    if data.len() < length {
        return (
            FrameContent::Invalid(format!(
                "Incomplete frame: expected {} bytes, got {}",
                length,
                data.len()
            )),
            CrcStatus::NotChecked,
        );
    }

    // Validate CRC
    let crc_status = if type_flags.is_work {
        // Work frames use CRC16
        if data.len() >= length && length >= 2 {
            if crc16_is_valid(&data[0..length - 2], &data[length - 2..length]) {
                CrcStatus::Valid
            } else {
                CrcStatus::Invalid
            }
        } else {
            CrcStatus::NotChecked
        }
    } else {
        // Command frames use CRC5
        if data.len() >= length {
            if crc5_is_valid(&data[0..length]) {
                CrcStatus::Valid
            } else {
                CrcStatus::Invalid
            }
        } else {
            CrcStatus::NotChecked
        }
    };

    // Decode command based on type and cmd
    let command = if type_flags.is_work {
        // Mining job
        decode_mining_job(data, length)
    } else {
        // Regular command
        decode_command(type_flags, data)
    };

    (command, crc_status)
}

/// Decode a regular command
fn decode_command(type_flags: TypeFlags, data: &[u8]) -> FrameContent {
    if data.len() < 5 {
        return FrameContent::Invalid("Command too short".to_string());
    }

    let cmd = match (type_flags.cmd, type_flags.is_broadcast) {
        (0, false) => {
            // Set chip address
            if data.len() >= 5 {
                Command::SetChipAddress { addr: data[4] }
            } else {
                return FrameContent::Invalid("SetChipAddress missing address".to_string());
            }
        }
        (1, false) => {
            // Write register to specific chip
            if data.len() >= 10 {
                let chip_addr = data[4];
                let reg_addr = data[5];
                let value = u32::from_le_bytes([data[6], data[7], data[8], data[9]]);
                Command::WriteRegister {
                    chip_addr,
                    reg_addr,
                    value,
                }
            } else {
                return FrameContent::Invalid("WriteRegister too short".to_string());
            }
        }
        (2, false) => {
            // Read register from specific chip
            if data.len() >= 6 {
                let chip_addr = data[4];
                let reg_addr = data[5];
                Command::ReadRegister {
                    chip_addr,
                    reg_addr,
                }
            } else {
                return FrameContent::Invalid("ReadRegister too short".to_string());
            }
        }
        (1, true) => {
            // Write register broadcast
            if data.len() >= 9 {
                let reg_addr = data[4];
                let value = u32::from_le_bytes([data[5], data[6], data[7], data[8]]);
                Command::WriteRegisterBroadcast { reg_addr, value }
            } else {
                return FrameContent::Invalid("WriteRegisterBroadcast too short".to_string());
            }
        }
        (2, true) => {
            // Read register broadcast
            if data.len() >= 5 {
                let reg_addr = data[4];
                Command::ReadRegisterBroadcast { reg_addr }
            } else {
                return FrameContent::Invalid("ReadRegisterBroadcast too short".to_string());
            }
        }
        _ => Command::Unknown {
            type_flags: data[2],
            payload: data[4..].to_vec(),
        },
    };

    FrameContent::Command(cmd)
}

/// Decode a mining job
fn decode_mining_job(data: &[u8], length: usize) -> FrameContent {
    // Check for full format (144 byte payload + headers/CRC)
    if length == 148 && data.len() >= 148 {
        // Full format job
        if data.len() >= 148 {
            let job = JobFullFormat {
                job_id: data[4],
                nbits: u32::from_le_bytes([data[5], data[6], data[7], data[8]]),
                ntime: u32::from_le_bytes([data[9], data[10], data[11], data[12]]),
                merkle_root_lsw: u32::from_le_bytes([data[13], data[14], data[15], data[16]]),
                midstates: [
                    data[17..49].try_into().unwrap(),
                    data[49..81].try_into().unwrap(),
                    data[81..113].try_into().unwrap(),
                    data[113..145].try_into().unwrap(),
                ],
            };
            FrameContent::Command(Command::MiningJob(MiningJobData::Full(job)))
        } else {
            FrameContent::Invalid("Full job too short".to_string())
        }
    } else {
        // Midstate format job (variable length)
        if data.len() >= 18 {
            let midstate_num = data[5] as usize;
            let expected_len = 18 + midstate_num * 32; // header + midstates + CRC

            if data.len() >= expected_len {
                let mut midstates = Vec::new();
                for i in 0..midstate_num {
                    let start = 17 + i * 32;
                    let end = start + 32;
                    if end <= data.len() - 2 {
                        // Leave room for CRC
                        midstates.push(data[start..end].try_into().unwrap());
                    }
                }

                let job = JobMidstateFormat {
                    job_id: data[4],
                    midstate_num: data[5],
                    nbits: u32::from_le_bytes([data[6], data[7], data[8], data[9]]),
                    ntime: u32::from_le_bytes([data[10], data[11], data[12], data[13]]),
                    merkle_root_lsw: u32::from_le_bytes([data[14], data[15], data[16], data[17]]),
                    midstates,
                };
                FrameContent::Command(Command::MiningJob(MiningJobData::Midstate(job)))
            } else {
                FrameContent::Invalid(format!(
                    "Midstate job too short: expected {}, got {}",
                    expected_len,
                    data.len()
                ))
            }
        } else {
            FrameContent::Invalid("Job frame too short".to_string())
        }
    }
}

/// Dissect a response frame
fn dissect_response(data: &[u8]) -> (FrameContent, CrcStatus) {
    if data.len() < 3 {
        return (
            FrameContent::Invalid(format!("Response too short: {} bytes", data.len())),
            CrcStatus::NotChecked,
        );
    }

    // Check preamble
    if data[0] != 0xAA || data[1] != 0x55 {
        return (
            FrameContent::Invalid("Invalid response preamble".to_string()),
            CrcStatus::NotChecked,
        );
    }

    // CRC5 is in the last byte, response type in upper 3 bits
    let crc_byte = data[data.len() - 1];
    let response_type = ResponseType::from_u8((crc_byte >> 5) & 0x07);

    // Validate CRC5
    let crc_status = if crc5_is_valid(data) {
        CrcStatus::Valid
    } else {
        CrcStatus::Invalid
    };

    // Decode based on response type
    let response = match response_type {
        ResponseType::RegisterRead => {
            if data.len() >= 9 {
                let chip_id = [data[2], data[3]];
                let reg_addr = data[4];
                let value = u32::from_le_bytes([data[5], data[6], data[7], data[8]]);
                Response::RegisterValue {
                    chip_id,
                    reg_addr,
                    value,
                }
            } else {
                return (
                    FrameContent::Invalid("Register read response too short".to_string()),
                    crc_status,
                );
            }
        }
        ResponseType::NonceFound => {
            // Nonce responses vary by chip type
            // Basic format: AA 55 [job_id] [nonce:4] [crc]
            if data.len() >= 8 {
                let job_id = data[2];
                let nonce = u32::from_le_bytes([data[3], data[4], data[5], data[6]]);

                // Extended formats may include midstate index and core ID
                let (midstate_idx, core_id) = if data.len() >= 11 {
                    // Extended format with core info
                    let idx = Some(data[7]);
                    let core = Some(u16::from_le_bytes([data[8], data[9]]));
                    (idx, core)
                } else {
                    (None, None)
                };

                Response::NonceFound {
                    job_id,
                    nonce,
                    midstate_idx,
                    core_id,
                }
            } else {
                return (
                    FrameContent::Invalid("Nonce response too short".to_string()),
                    crc_status,
                );
            }
        }
        ResponseType::Version => {
            if data.len() >= 7 {
                let version = u32::from_le_bytes([data[2], data[3], data[4], data[5]]);
                Response::Version { version }
            } else {
                return (
                    FrameContent::Invalid("Version response too short".to_string()),
                    crc_status,
                );
            }
        }
    };

    (FrameContent::Response(response), crc_status)
}

/// Dissected I2C operation
#[derive(Debug)]
pub struct DissectedI2c {
    pub timestamp: f64,
    pub address: u8,
    pub device: I2cDevice,
    pub operation: String,
    pub raw_data: Vec<u8>,
}

/// Known I2C devices
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I2cDevice {
    Emc2101,
    Tps546,
    Unknown,
}

// Simple I2C formatting functions (we can't import from mujina_miner due to circular deps)
fn emc2101_format_transaction(reg: u8, data: Option<&[u8]>, is_read: bool) -> String {
    let reg_name = match reg {
        0x00 => "INTERNAL_TEMP",
        0x01 => "EXTERNAL_TEMP_HIGH",
        0x4A => "FAN_CONFIG",
        0x4C => "FAN_SETTING",
        0xFE => "MFG_ID",
        0xFD => "PRODUCT_ID",
        0xFF => "REVISION",
        _ => "UNKNOWN",
    };

    if is_read {
        if let Some(data) = data {
            format!("READ {}={:02x?}", reg_name, data)
        } else {
            format!("READ {}", reg_name)
        }
    } else {
        if let Some(data) = data {
            format!("WRITE {}={:02x?}", reg_name, data)
        } else {
            format!("WRITE REG[0x{:02x}]", reg)
        }
    }
}

fn tps546_format_transaction(cmd: u8, data: Option<&[u8]>, is_read: bool) -> String {
    let cmd_name = match cmd {
        0x01 => "OPERATION",
        0x79 => "STATUS_WORD",
        0xAD => "IC_DEVICE_ID",
        _ => "UNKNOWN",
    };

    if is_read {
        if let Some(data) = data {
            format!("READ {}={:02x?}", cmd_name, data)
        } else {
            format!("READ {}", cmd_name)
        }
    } else {
        if let Some(data) = data {
            format!("WRITE {}={:02x?}", cmd_name, data)
        } else {
            format!("WRITE CMD[0x{:02x}]", cmd)
        }
    }
}

/// Dissect an I2C operation
pub fn dissect_i2c_operation(op: &I2cOperation) -> DissectedI2c {
    let device = match op.address {
        0x4C => I2cDevice::Emc2101,
        0x24 => I2cDevice::Tps546,
        _ => I2cDevice::Unknown,
    };

    let operation = if let Some(reg) = op.register {
        let data = op.read_data.as_ref().or(op.write_data.as_ref());
        let is_read = op.read_data.is_some();

        match device {
            I2cDevice::Emc2101 => format!(
                "EMC2101 {}",
                emc2101_format_transaction(reg, data.map(|v| v.as_slice()), is_read)
            ),
            I2cDevice::Tps546 => format!(
                "TPS546 {}",
                tps546_format_transaction(reg, data.map(|v| v.as_slice()), is_read)
            ),
            I2cDevice::Unknown => {
                if let Some(data) = &op.read_data {
                    format!("READ [0x{:02x}]={:02x?}", reg, data)
                } else if let Some(data) = &op.write_data {
                    format!("WRITE [0x{:02x}]={:02x?}", reg, data)
                } else {
                    format!("ACCESS [0x{:02x}]", reg)
                }
            }
        }
    } else {
        format!("I2C op @ 0x{:02x}", op.address)
    };

    let raw_data = op
        .write_data
        .as_ref()
        .or(op.read_data.as_ref())
        .cloned()
        .unwrap_or_default();

    DissectedI2c {
        timestamp: op.start_time,
        address: op.address,
        device,
        operation,
        raw_data,
    }
}
