//! Output formatting for dissected frames.

use crate::dissect::{CrcStatus, DissectedFrame, DissectedI2c, FrameContent, I2cDevice};
use crate::serial::Direction;
use colored::Colorize;

/// Output formatter configuration
#[derive(Debug, Clone)]
pub struct OutputConfig {
    pub show_raw_hex: bool,
    pub use_relative_time: bool,
    pub start_time: Option<f64>,
    pub use_color: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            show_raw_hex: false,
            use_relative_time: false,
            start_time: None,
            use_color: true,
        }
    }
}

/// Format a dissected serial frame
pub fn format_serial_frame(frame: &DissectedFrame, config: &OutputConfig) -> String {
    let timestamp = format_timestamp(frame.timestamp, config);

    let direction_str = match frame.direction {
        Direction::HostToChip => "CI → ASIC",
        Direction::ChipToHost => "RO ← ASIC",
    };

    let content_str = match &frame.content {
        FrameContent::Command(cmd) => format!("{:?}", cmd), // Use Debug for now since we added Display to main lib
        FrameContent::Unknown(msg) => msg.clone(),
        FrameContent::Invalid(msg) => {
            if config.use_color {
                format!("{}", msg.red())
            } else {
                msg.clone()
            }
        }
    };

    let mut result = format!("[{}] {}: {}", timestamp, direction_str, content_str);

    if config.show_raw_hex {
        result.push_str(&format!(" [{}]", format_hex(&frame.raw_data)));
    }

    if frame.crc_status != CrcStatus::NotChecked {
        result.push_str(&format!(" [{}]", frame.crc_status));
    }

    result
}

/// Format an I2C operation
pub fn format_i2c_operation(op: &DissectedI2c, config: &OutputConfig) -> String {
    let timestamp = format_timestamp(op.timestamp, config);

    let device_str = match op.device {
        I2cDevice::Emc2101 => format!("EMC2101@0x{:02x}", op.address),
        I2cDevice::Tps546 => format!("TPS546@0x{:02x}", op.address),
        I2cDevice::Unknown => format!("Device@0x{:02x}", op.address),
    };

    let mut result = format!("[{}] I2C: {} {}", timestamp, device_str, op.operation);

    if config.show_raw_hex && !op.raw_data.is_empty() {
        result.push_str(&format!(" [{}]", format_hex(&op.raw_data)));
    }

    result
}

/// Format timestamp
fn format_timestamp(timestamp: f64, config: &OutputConfig) -> String {
    if config.use_relative_time {
        let relative = if let Some(start) = config.start_time {
            timestamp - start
        } else {
            timestamp
        };
        format!("{:10.6}", relative)
    } else {
        format!("{:10.6}", timestamp)
    }
}

/// Format hex bytes
fn format_hex(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Event type for unified output
#[derive(Debug)]
pub enum OutputEvent {
    Serial(DissectedFrame),
    I2c(DissectedI2c),
}

impl OutputEvent {
    pub fn timestamp(&self) -> f64 {
        match self {
            OutputEvent::Serial(frame) => frame.timestamp,
            OutputEvent::I2c(op) => op.timestamp,
        }
    }

    pub fn format(&self, config: &OutputConfig) -> String {
        match self {
            OutputEvent::Serial(frame) => format_serial_frame(frame, config),
            OutputEvent::I2c(op) => format_i2c_operation(op, config),
        }
    }
}
