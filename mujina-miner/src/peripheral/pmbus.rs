//! PMBus Protocol Support
//!
//! This module provides generic PMBus protocol definitions and utilities
//! that can be used by PMBus-compliant device drivers.
//!
//! PMBus is a variant of SMBus with extensions for power management.
//! Specification: <https://pmbus.org/specification-documents/>

use thiserror::Error;

/// PMBus standard command codes
pub mod commands {
    pub const OPERATION: u8 = 0x01;
    pub const ON_OFF_CONFIG: u8 = 0x02;
    pub const CLEAR_FAULTS: u8 = 0x03;
    pub const PHASE: u8 = 0x04;
    pub const CAPABILITY: u8 = 0x19;
    pub const VOUT_MODE: u8 = 0x20;
    pub const VOUT_COMMAND: u8 = 0x21;
    pub const VOUT_MAX: u8 = 0x24;
    pub const VOUT_MARGIN_HIGH: u8 = 0x25;
    pub const VOUT_MARGIN_LOW: u8 = 0x26;
    pub const VOUT_SCALE_LOOP: u8 = 0x29;
    pub const VOUT_MIN: u8 = 0x2B;
    pub const FREQUENCY_SWITCH: u8 = 0x33;
    pub const VIN_ON: u8 = 0x35;
    pub const VIN_OFF: u8 = 0x36;
    pub const VOUT_OV_FAULT_LIMIT: u8 = 0x40;
    pub const VOUT_OV_WARN_LIMIT: u8 = 0x42;
    pub const VOUT_UV_WARN_LIMIT: u8 = 0x43;
    pub const VOUT_UV_FAULT_LIMIT: u8 = 0x44;
    pub const IOUT_OC_FAULT_LIMIT: u8 = 0x46;
    pub const IOUT_OC_FAULT_RESPONSE: u8 = 0x47;
    pub const IOUT_OC_WARN_LIMIT: u8 = 0x4A;
    pub const OT_FAULT_LIMIT: u8 = 0x4F;
    pub const OT_FAULT_RESPONSE: u8 = 0x50;
    pub const OT_WARN_LIMIT: u8 = 0x51;
    pub const VIN_OV_FAULT_LIMIT: u8 = 0x55;
    pub const VIN_OV_FAULT_RESPONSE: u8 = 0x56;
    pub const VIN_UV_WARN_LIMIT: u8 = 0x58;
    pub const TON_DELAY: u8 = 0x60;
    pub const TON_RISE: u8 = 0x61;
    pub const TON_MAX_FAULT_LIMIT: u8 = 0x62;
    pub const TON_MAX_FAULT_RESPONSE: u8 = 0x63;
    pub const TOFF_DELAY: u8 = 0x64;
    pub const TOFF_FALL: u8 = 0x65;
    pub const STATUS_WORD: u8 = 0x79;
    pub const STATUS_VOUT: u8 = 0x7A;
    pub const STATUS_IOUT: u8 = 0x7B;
    pub const STATUS_INPUT: u8 = 0x7C;
    pub const STATUS_TEMPERATURE: u8 = 0x7D;
    pub const STATUS_CML: u8 = 0x7E;
    pub const STATUS_OTHER: u8 = 0x7F;
    pub const STATUS_MFR_SPECIFIC: u8 = 0x80;
    pub const READ_VIN: u8 = 0x88;
    pub const READ_VOUT: u8 = 0x8B;
    pub const READ_IOUT: u8 = 0x8C;
    pub const READ_TEMPERATURE_1: u8 = 0x8D;
    pub const MFR_ID: u8 = 0x99;
    pub const MFR_MODEL: u8 = 0x9A;
    pub const MFR_REVISION: u8 = 0x9B;
    pub const IC_DEVICE_ID: u8 = 0xAD;
    pub const COMPENSATION_CONFIG: u8 = 0xB1;
    pub const SYNC_CONFIG: u8 = 0xE4;
    pub const STACK_CONFIG: u8 = 0xEC;
    pub const PIN_DETECT_OVERRIDE: u8 = 0xEE;
    pub const INTERLEAVE: u8 = 0x37;
}

/// STATUS_WORD bits (PMBus specification section 17.2)
pub mod status_word {
    pub const VOUT: u16 = 0x8000;      // Bit 15: Output voltage fault/warning
    pub const IOUT: u16 = 0x4000;      // Bit 14: Output current fault/warning
    pub const INPUT: u16 = 0x2000;     // Bit 13: Input voltage fault/warning
    pub const MFR: u16 = 0x1000;       // Bit 12: Manufacturer specific fault/warning
    pub const PGOOD: u16 = 0x0800;     // Bit 11: Power good (not a fault)
    pub const FANS: u16 = 0x0400;      // Bit 10: One or more fans fault/warning
    pub const OTHER: u16 = 0x0200;     // Bit 9: Other fault/warning
    pub const UNKNOWN: u16 = 0x0100;   // Bit 8: Unknown fault/warning
    pub const BUSY: u16 = 0x0080;      // Bit 7: Busy - unable to respond
    pub const OFF: u16 = 0x0040;       // Bit 6: Unit is off
    pub const VOUT_OV: u16 = 0x0020;   // Bit 5: Output overvoltage fault
    pub const IOUT_OC: u16 = 0x0010;   // Bit 4: Output overcurrent fault
    pub const VIN_UV: u16 = 0x0008;    // Bit 3: Input undervoltage fault
    pub const TEMP: u16 = 0x0004;      // Bit 2: Temperature fault/warning
    pub const CML: u16 = 0x0002;       // Bit 1: Communication/Logic/Memory fault
    pub const NONE: u16 = 0x0001;      // Bit 0: No faults (NONE_OF_THE_ABOVE)
}

/// STATUS_VOUT bits (PMBus specification section 17.7)
pub mod status_vout {
    pub const VOUT_OV_FAULT: u8 = 0x80;    // Bit 7: Output overvoltage fault
    pub const VOUT_OV_WARN: u8 = 0x40;     // Bit 6: Output overvoltage warning
    pub const VOUT_UV_WARN: u8 = 0x20;     // Bit 5: Output undervoltage warning
    pub const VOUT_UV_FAULT: u8 = 0x10;    // Bit 4: Output undervoltage fault
    pub const VOUT_MAX: u8 = 0x08;         // Bit 3: VOUT at max (tracking or margin)
    pub const TON_MAX_FAULT: u8 = 0x02;    // Bit 1: Unit did not power up
    pub const VOUT_MIN: u8 = 0x01;         // Bit 0: VOUT at min (tracking)
}

/// STATUS_IOUT bits (PMBus specification section 17.8)
pub mod status_iout {
    pub const IOUT_OC_FAULT: u8 = 0x80;    // Bit 7: Output overcurrent fault
    pub const IOUT_OC_LV_FAULT: u8 = 0x40; // Bit 6: Output OC and low voltage fault
    pub const IOUT_OC_WARN: u8 = 0x20;     // Bit 5: Output overcurrent warning
    pub const IOUT_UC_FAULT: u8 = 0x10;    // Bit 4: Output undercurrent fault
    pub const CURR_SHARE_FAULT: u8 = 0x08; // Bit 3: Current share fault
    pub const IN_PWR_LIM: u8 = 0x04;       // Bit 2: Unit in power limiting mode
    pub const POUT_OP_FAULT: u8 = 0x02;    // Bit 1: Output overpower fault
    pub const POUT_OP_WARN: u8 = 0x01;     // Bit 0: Output overpower warning
}

/// STATUS_INPUT bits (PMBus specification section 17.9)
pub mod status_input {
    pub const VIN_OV_FAULT: u8 = 0x80;     // Bit 7: Input overvoltage fault
    pub const VIN_OV_WARN: u8 = 0x40;      // Bit 6: Input overvoltage warning
    pub const VIN_UV_WARN: u8 = 0x20;      // Bit 5: Input undervoltage warning
    pub const VIN_UV_FAULT: u8 = 0x10;     // Bit 4: Input undervoltage fault
    pub const UNIT_OFF_VIN_LOW: u8 = 0x08; // Bit 3: Unit off for insufficient input
    pub const IIN_OC_FAULT: u8 = 0x04;     // Bit 2: Input overcurrent fault
    pub const IIN_OC_WARN: u8 = 0x02;      // Bit 1: Input overcurrent warning
    pub const PIN_OP_WARN: u8 = 0x01;      // Bit 0: Input overpower warning
}

/// STATUS_TEMPERATURE bits (PMBus specification section 17.10)
pub mod status_temperature {
    pub const OT_FAULT: u8 = 0x80;         // Bit 7: Overtemperature fault
    pub const OT_WARN: u8 = 0x40;          // Bit 6: Overtemperature warning
    pub const UT_WARN: u8 = 0x20;          // Bit 5: Undertemperature warning
    pub const UT_FAULT: u8 = 0x10;         // Bit 4: Undertemperature fault
}

/// STATUS_CML bits (PMBus specification section 17.11)
pub mod status_cml {
    pub const INVALID_CMD: u8 = 0x80;      // Bit 7: Invalid/unsupported command
    pub const INVALID_DATA: u8 = 0x40;     // Bit 6: Invalid/unsupported data
    pub const PEC_FAULT: u8 = 0x20;        // Bit 5: Packet Error Check failed
    pub const MEMORY_FAULT: u8 = 0x10;     // Bit 4: Memory fault detected
    pub const PROCESSOR_FAULT: u8 = 0x08;  // Bit 3: Processor fault detected
    pub const OTHER_COMM_FAULT: u8 = 0x02; // Bit 1: Other communication fault
    pub const OTHER_MEM_LOGIC: u8 = 0x01;  // Bit 0: Other memory or logic fault
}

/// OPERATION command values (PMBus specification section 12.1)
pub mod operation {
    pub const OFF_IMMEDIATE: u8 = 0x00;    // Turn off immediately
    pub const SOFT_OFF: u8 = 0x40;         // Soft off (using programmed delays)
    pub const ON_MARGIN_LOW: u8 = 0x98;    // On with margin low
    pub const ON_MARGIN_HIGH: u8 = 0xA8;   // On with margin high
    pub const ON: u8 = 0x80;               // Turn on
}

/// ON_OFF_CONFIG bits (PMBus specification section 12.2)
pub mod on_off_config {
    pub const PU: u8 = 0x10;               // Bit 4: Power-up from CONTROL pin
    pub const CMD: u8 = 0x08;              // Bit 3: Respond to OPERATION command
    pub const CP: u8 = 0x04;               // Bit 2: Control pin present
    pub const POLARITY: u8 = 0x02;         // Bit 1: Control pin polarity (1=active high)
    pub const DELAY: u8 = 0x01;            // Bit 0: Turn off delay (0=disabled)
}

/// PMBus error types
#[derive(Error, Debug)]
pub enum PMBusError {
    #[error("Invalid data format")]
    InvalidDataFormat,
    #[error("Value out of range")]
    ValueOutOfRange,
    #[error("Command not supported")]
    CommandNotSupported,
    #[error("Communication error")]
    CommunicationError,
}

/// SLINEAR11 data format conversion utilities
///
/// Format: [5-bit two's complement exponent][11-bit two's complement mantissa]
/// Value = mantissa × 2^exponent
pub struct Linear11;

impl Linear11 {
    /// Convert SLINEAR11 format to floating point
    pub fn to_float(value: u16) -> f32 {
        // Extract 5-bit exponent (bits 15-11) as two's complement
        let exp_raw = ((value >> 11) & 0x1F) as i8;
        let exponent = if exp_raw & 0x10 != 0 {
            // Sign extend for negative exponent
            (exp_raw as u8 | 0xE0) as i8 as i32
        } else {
            exp_raw as i32
        };

        // Extract 11-bit mantissa (bits 10-0) as two's complement
        let mant_raw = (value & 0x7FF) as i16;
        let mantissa = if mant_raw & 0x400 != 0 {
            // Sign extend for negative mantissa
            ((mant_raw as u16 | 0xF800) as i16) as i32
        } else {
            mant_raw as i32
        };

        mantissa as f32 * 2.0_f32.powi(exponent)
    }

    /// Convert SLINEAR11 format to integer
    pub fn to_int(value: u16) -> i32 {
        Self::to_float(value) as i32
    }

    /// Convert floating point to SLINEAR11 format
    pub fn from_float(value: f32) -> u16 {
        if value == 0.0 {
            return 0;
        }

        // Find best exponent to keep mantissa in 11-bit range
        let mut best_exp = 0i8;
        let mut best_error = f32::MAX;

        // Try exponents from -16 to +15 (5-bit two's complement range)
        for exp in -16i8..=15 {
            let mantissa_f = value / 2.0_f32.powi(exp as i32);

            // Check if mantissa fits in 11-bit two's complement (-1024 to 1023)
            if (-1024.0..1024.0).contains(&mantissa_f) {
                let mantissa = mantissa_f.round() as i32;
                let reconstructed = mantissa as f32 * 2.0_f32.powi(exp as i32);
                let error = (reconstructed - value).abs();

                if error < best_error {
                    best_error = error;
                    best_exp = exp;
                }
            }
        }

        let mantissa = (value / 2.0_f32.powi(best_exp as i32)).round() as i32;

        // Pack into SLINEAR11 format
        let exp_bits = (best_exp as u16) & 0x1F;
        let mant_bits = (mantissa as u16) & 0x7FF;

        (exp_bits << 11) | mant_bits
    }

    /// Convert integer to SLINEAR11 format
    pub fn from_int(value: i32) -> u16 {
        Self::from_float(value as f32)
    }
}

/// ULINEAR16 data format conversion utilities
///
/// Format: 16-bit unsigned mantissa, exponent from VOUT_MODE
/// Value = mantissa × 2^exponent
pub struct Linear16;

impl Linear16 {
    /// Convert ULINEAR16 format to floating point
    ///
    /// The exponent must be extracted from VOUT_MODE register
    pub fn to_float(value: u16, vout_mode: u8) -> f32 {
        // Extract 5-bit two's complement exponent from VOUT_MODE
        let exp_raw = (vout_mode & 0x1F) as i8;
        let exponent = if exp_raw & 0x10 != 0 {
            // Sign extend for negative exponent
            (exp_raw as u8 | 0xE0) as i8 as i32
        } else {
            exp_raw as i32
        };

        value as f32 * 2.0_f32.powi(exponent)
    }

    /// Convert floating point to ULINEAR16 format
    ///
    /// The exponent must be extracted from VOUT_MODE register
    pub fn from_float(value: f32, vout_mode: u8) -> Result<u16, PMBusError> {
        // Extract 5-bit two's complement exponent from VOUT_MODE
        let exp_raw = (vout_mode & 0x1F) as i8;
        let exponent = if exp_raw & 0x10 != 0 {
            // Sign extend for negative exponent
            (exp_raw as u8 | 0xE0) as i8 as i32
        } else {
            exp_raw as i32
        };

        let mantissa = (value / 2.0_f32.powi(exponent)).round() as u32;
        if mantissa > 0xFFFF {
            return Err(PMBusError::ValueOutOfRange);
        }

        Ok(mantissa as u16)
    }
}

/// Helper functions for decoding status registers
pub struct StatusDecoder;

impl StatusDecoder {
    /// Decode STATUS_WORD bits into human-readable descriptions
    pub fn decode_status_word(status: u16) -> Vec<&'static str> {
        let mut desc = Vec::new();
        if status & status_word::VOUT != 0 { desc.push("VOUT fault/warning"); }
        if status & status_word::IOUT != 0 { desc.push("IOUT fault/warning"); }
        if status & status_word::INPUT != 0 { desc.push("INPUT fault/warning"); }
        if status & status_word::MFR != 0 { desc.push("MFR specific"); }
        if status & status_word::PGOOD != 0 { desc.push("PGOOD"); }
        if status & status_word::FANS != 0 { desc.push("FAN fault/warning"); }
        if status & status_word::OTHER != 0 { desc.push("OTHER"); }
        if status & status_word::UNKNOWN != 0 { desc.push("UNKNOWN"); }
        if status & status_word::BUSY != 0 { desc.push("BUSY"); }
        if status & status_word::OFF != 0 { desc.push("OFF"); }
        if status & status_word::VOUT_OV != 0 { desc.push("VOUT_OV fault"); }
        if status & status_word::IOUT_OC != 0 { desc.push("IOUT_OC fault"); }
        if status & status_word::VIN_UV != 0 { desc.push("VIN_UV fault"); }
        if status & status_word::TEMP != 0 { desc.push("TEMP fault/warning"); }
        if status & status_word::CML != 0 { desc.push("CML fault"); }
        if status & status_word::NONE != 0 && desc.is_empty() { desc.push("NONE_OF_THE_ABOVE"); }
        desc
    }

    /// Decode STATUS_VOUT bits into human-readable descriptions
    pub fn decode_status_vout(status: u8) -> Vec<&'static str> {
        let mut desc = Vec::new();
        if status & status_vout::VOUT_OV_FAULT != 0 { desc.push("OV fault"); }
        if status & status_vout::VOUT_OV_WARN != 0 { desc.push("OV warning"); }
        if status & status_vout::VOUT_UV_WARN != 0 { desc.push("UV warning"); }
        if status & status_vout::VOUT_UV_FAULT != 0 { desc.push("UV fault"); }
        if status & status_vout::VOUT_MAX != 0 { desc.push("at MAX"); }
        if status & status_vout::TON_MAX_FAULT != 0 { desc.push("failed to start"); }
        if status & status_vout::VOUT_MIN != 0 { desc.push("at MIN"); }
        desc
    }

    /// Decode STATUS_IOUT bits into human-readable descriptions
    pub fn decode_status_iout(status: u8) -> Vec<&'static str> {
        let mut desc = Vec::new();
        if status & status_iout::IOUT_OC_FAULT != 0 { desc.push("OC fault"); }
        if status & status_iout::IOUT_OC_LV_FAULT != 0 { desc.push("OC+LV fault"); }
        if status & status_iout::IOUT_OC_WARN != 0 { desc.push("OC warning"); }
        if status & status_iout::IOUT_UC_FAULT != 0 { desc.push("UC fault"); }
        if status & status_iout::CURR_SHARE_FAULT != 0 { desc.push("current share fault"); }
        if status & status_iout::IN_PWR_LIM != 0 { desc.push("power limiting"); }
        if status & status_iout::POUT_OP_FAULT != 0 { desc.push("overpower fault"); }
        if status & status_iout::POUT_OP_WARN != 0 { desc.push("overpower warning"); }
        desc
    }

    /// Decode STATUS_INPUT bits into human-readable descriptions
    pub fn decode_status_input(status: u8) -> Vec<&'static str> {
        let mut desc = Vec::new();
        if status & status_input::VIN_OV_FAULT != 0 { desc.push("VIN OV fault"); }
        if status & status_input::VIN_OV_WARN != 0 { desc.push("VIN OV warning"); }
        if status & status_input::VIN_UV_WARN != 0 { desc.push("VIN UV warning"); }
        if status & status_input::VIN_UV_FAULT != 0 { desc.push("VIN UV fault"); }
        if status & status_input::UNIT_OFF_VIN_LOW != 0 { desc.push("off due to low VIN"); }
        if status & status_input::IIN_OC_FAULT != 0 { desc.push("IIN OC fault"); }
        if status & status_input::IIN_OC_WARN != 0 { desc.push("IIN OC warning"); }
        if status & status_input::PIN_OP_WARN != 0 { desc.push("input overpower warning"); }
        desc
    }

    /// Decode STATUS_TEMPERATURE bits into human-readable descriptions
    pub fn decode_status_temp(status: u8) -> Vec<&'static str> {
        let mut desc = Vec::new();
        if status & status_temperature::OT_FAULT != 0 { desc.push("overtemp fault"); }
        if status & status_temperature::OT_WARN != 0 { desc.push("overtemp warning"); }
        if status & status_temperature::UT_WARN != 0 { desc.push("undertemp warning"); }
        if status & status_temperature::UT_FAULT != 0 { desc.push("undertemp fault"); }
        desc
    }

    /// Decode STATUS_CML bits into human-readable descriptions
    pub fn decode_status_cml(status: u8) -> Vec<&'static str> {
        let mut desc = Vec::new();
        if status & status_cml::INVALID_CMD != 0 { desc.push("invalid command"); }
        if status & status_cml::INVALID_DATA != 0 { desc.push("invalid data"); }
        if status & status_cml::PEC_FAULT != 0 { desc.push("PEC error"); }
        if status & status_cml::MEMORY_FAULT != 0 { desc.push("memory fault"); }
        if status & status_cml::PROCESSOR_FAULT != 0 { desc.push("processor fault"); }
        if status & status_cml::OTHER_COMM_FAULT != 0 { desc.push("other comm fault"); }
        if status & status_cml::OTHER_MEM_LOGIC != 0 { desc.push("other mem/logic fault"); }
        desc
    }

    /// Decode fault response byte into human-readable description
    ///
    /// Fault response format:
    /// - Bits 7-5: Response type
    /// - Bits 4-3: Number of retries
    /// - Bits 2-0: Retry delay time
    pub fn decode_fault_response(response: u8) -> String {
        let response_type = (response >> 5) & 0x07;
        let retry_count = (response >> 3) & 0x03;
        let delay_time = response & 0x07;

        let response_desc = match response_type {
            0b000 => "ignore fault",
            0b001 => "shutdown, retry indefinitely",
            0b010 => "shutdown, no retry",
            0b011 => "shutdown with retries",
            0b100 => "continue, retry indefinitely",
            0b101 => "continue, no retry",
            0b110 => "continue with retries",
            0b111 => "shutdown with delay and retries",
            _ => "unknown",
        };

        let retries_desc = match retry_count {
            0b00 => "no retries",
            0b01 => "1 retry",
            0b10 => "2 retries",
            0b11 => match response_type {
                0b001 | 0b100 => "infinite retries",
                _ => "3 retries",
            },
            _ => "unknown",
        };

        let delay_desc = match delay_time {
            0b000 => "0ms",
            0b001 => "22.7ms",
            0b010 => "45.4ms",
            0b011 => "91ms",
            0b100 => "182ms",
            0b101 => "364ms",
            0b110 => "728ms",
            0b111 => "1456ms",
            _ => "unknown",
        };

        // Special cases for common values
        match response {
            0x00 => "ignore fault".to_string(),
            0xC0 => "shutdown immediately, no retries".to_string(),
            0xFF => "infinite retries, wait for recovery".to_string(),
            _ => {
                if retry_count == 0 || response_type == 0b010 || response_type == 0b101 {
                    response_desc.to_string()
                } else {
                    format!("{}, {}, {} delay", response_desc, retries_desc, delay_desc)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear11_conversion() {
        // Test zero
        assert_eq!(Linear11::to_float(0), 0.0);
        assert_eq!(Linear11::from_float(0.0), 0);

        // Test positive values
        let value = 650.0; // Typical frequency value
        let encoded = Linear11::from_float(value);
        let decoded = Linear11::to_float(encoded);
        assert!((decoded - value).abs() < 1.0); // Allow small rounding error

        // Test negative values
        let value = -10.5;
        let encoded = Linear11::from_float(value);
        let decoded = Linear11::to_float(encoded);
        assert!((decoded - value).abs() < 0.1);
    }

    #[test]
    fn test_linear11_common_voltage_values() {
        // Common voltage values in power management
        let test_cases = [
            (1.2, "BM1370 core voltage"),
            (3.3, "IO voltage"),
            (5.0, "USB voltage"),
            (12.0, "Input voltage"),
            (48.0, "Telecom voltage"),
        ];
        
        for (voltage, description) in test_cases {
            let encoded = Linear11::from_float(voltage);
            let decoded = Linear11::to_float(encoded);
            assert!(
                (decoded - voltage).abs() < 0.01,
                "{} conversion failed: expected {}, got {}",
                description, voltage, decoded
            );
        }
    }

    #[test]
    fn test_linear11_temperature_values() {
        // Common temperature values
        let temperatures = [
            -40.0,  // Minimum operating temperature
            0.0,    // Freezing point
            25.0,   // Room temperature
            85.0,   // Maximum commercial temperature
            105.0,  // Warning threshold
            125.0,  // Maximum junction temperature
        ];
        
        for temp in temperatures {
            let encoded = Linear11::from_float(temp);
            let decoded = Linear11::to_float(encoded);
            assert!(
                (decoded - temp).abs() < 0.1,
                "Temperature {} conversion failed: got {}",
                temp, decoded
            );
        }
    }

    #[test]
    fn test_linear11_current_values() {
        // Common current values for mining hardware
        let currents = [
            0.5,   // Low current
            10.0,  // Moderate current
            25.0,  // Bitaxe warning threshold
            30.0,  // Bitaxe fault threshold
            100.0, // High-power ASIC current
        ];
        
        for current in currents {
            let encoded = Linear11::from_float(current);
            let decoded = Linear11::to_float(encoded);
            // Current measurements can tolerate slightly more error
            assert!(
                (decoded - current).abs() < 0.1,
                "Current {} conversion failed: got {}",
                current, decoded
            );
        }
    }

    #[test]
    fn test_linear11_edge_cases() {
        // Maximum positive mantissa with maximum exponent
        // 1023 * 2^15 = 33521664
        let max_positive = 1023.0 * (1 << 15) as f32;
        let encoded = Linear11::from_float(max_positive);
        let decoded = Linear11::to_float(encoded);
        // Large values will have more rounding error
        assert!((decoded - max_positive).abs() / max_positive < 0.001);

        // Maximum negative mantissa with minimum exponent
        // -1024 * 2^-16 = -0.015625
        let min_negative = -1024.0 / (1 << 16) as f32;
        let encoded = Linear11::from_float(min_negative);
        let decoded = Linear11::to_float(encoded);
        assert!((decoded - min_negative).abs() < 0.0001);
        
        // Test very small positive value
        let tiny = 0.001;
        let encoded = Linear11::from_float(tiny);
        let decoded = Linear11::to_float(encoded);
        assert!((decoded - tiny).abs() < 0.0001);
    }

    #[test]
    fn test_linear11_round_trip() {
        // Test that multiple encode/decode cycles are stable
        let values = [1.234, -5.678, 100.0, 0.01];
        
        for original in values {
            let mut value = original;
            for _ in 0..3 {
                let encoded = Linear11::from_float(value);
                value = Linear11::to_float(encoded);
            }
            // After multiple round trips, should still be close to original
            assert!(
                (value - original).abs() < 0.01,
                "Round trip failed for {}: got {}",
                original, value
            );
        }
    }

    #[test]
    fn test_linear16_conversion() {
        let vout_mode = 0x17; // Example exponent (-9)

        // Test typical voltage value
        let value = 1.15; // Typical VOUT value
        let encoded = Linear16::from_float(value, vout_mode).unwrap();
        let decoded = Linear16::to_float(encoded, vout_mode);
        assert!((decoded - value).abs() < 0.01);
    }

    #[test]
    fn test_linear16_different_exponents() {
        // Test different VOUT_MODE exponents commonly used
        let test_cases = [
            (0x17, 1.15, "millivolt precision (-9)"),  // 2^-9 = ~0.00195V resolution
            (0x14, 1.15, "higher precision (-12)"),     // 2^-12 = ~0.000244V resolution
            (0x00, 12.0, "volt precision (0)"),         // 2^0 = 1V resolution
            (0x1B, 0.9, "microvolt precision (-5)"),    // 2^-5 = ~0.03125V resolution
        ];
        
        for (vout_mode, voltage, description) in test_cases {
            match Linear16::from_float(voltage, vout_mode) {
                Ok(encoded) => {
                    let decoded = Linear16::to_float(encoded, vout_mode);
                    // Tolerance is one LSB of the format
                    let exp_raw = (vout_mode & 0x1F) as i8;
                    let exponent = if exp_raw & 0x10 != 0 {
                        (exp_raw as u8 | 0xE0) as i8 as i32
                    } else {
                        exp_raw as i32
                    };
                    let tolerance = 2.0_f32.powi(exponent);
                    assert!(
                        (decoded - voltage).abs() <= tolerance,
                        "{} failed: expected {}, got {}, tolerance {}",
                        description, voltage, decoded, tolerance
                    );
                }
                Err(_) => panic!("{} encoding failed for value {}", description, voltage),
            }
        }
    }

    #[test]
    fn test_linear16_boundary_values() {
        let vout_mode = 0x17; // -9 exponent
        
        // Test maximum mantissa value
        let max_decoded = Linear16::to_float(0xFFFF, vout_mode);
        assert_eq!(max_decoded, 65535.0 * 2.0_f32.powi(-9));
        
        // Test minimum non-zero value
        let min_decoded = Linear16::to_float(0x0001, vout_mode);
        assert_eq!(min_decoded, 1.0 * 2.0_f32.powi(-9));
        
        // Test zero
        let zero_decoded = Linear16::to_float(0x0000, vout_mode);
        assert_eq!(zero_decoded, 0.0);
    }

    #[test]
    fn test_linear16_overflow_detection() {
        // With exponent 0, maximum representable value is 65535
        let vout_mode = 0x00;
        
        // This should succeed
        assert!(Linear16::from_float(65535.0, vout_mode).is_ok());
        
        // This should fail - value too large
        assert!(Linear16::from_float(65536.0, vout_mode).is_err());
        
        // With positive exponent, smaller values can be represented with less precision
        let vout_mode_pos = 0x05; // +5 exponent, larger resolution steps
        // Value 1000 with exponent +5: mantissa = 1000 / 32 = 31.25
        assert!(Linear16::from_float(1000.0, vout_mode_pos).is_ok());
    }

    #[test]
    fn test_status_decoder() {
        // Test STATUS_WORD decoding
        let status = status_word::VOUT | status_word::TEMP;
        let desc = StatusDecoder::decode_status_word(status);
        assert!(desc.contains(&"VOUT fault/warning"));
        assert!(desc.contains(&"TEMP fault/warning"));

        // Test fault response decoding
        assert_eq!(
            StatusDecoder::decode_fault_response(0xC0),
            "shutdown immediately, no retries"
        );
        assert_eq!(
            StatusDecoder::decode_fault_response(0xFF),
            "infinite retries, wait for recovery"
        );
    }

    #[test]
    fn test_fault_response_combinations() {
        // Test various fault response byte combinations
        // Format: [response_type:3][retry_count:2][delay:3]
        
        // Test all delay times with shutdown and 1 retry
        // 0x58 = 0101 1000 = response_type=010 (shutdown no retry), retry=11, delay=000
        // For shutdown with retries and 1 retry, we need 0x68-0x6F
        let delays = [
            (0x68, "shutdown with retries, 1 retry, 0ms delay"),
            (0x69, "shutdown with retries, 1 retry, 22.7ms delay"),
            (0x6A, "shutdown with retries, 1 retry, 45.4ms delay"),
            (0x6B, "shutdown with retries, 1 retry, 91ms delay"),
            (0x6C, "shutdown with retries, 1 retry, 182ms delay"),
            (0x6D, "shutdown with retries, 1 retry, 364ms delay"),
            (0x6E, "shutdown with retries, 1 retry, 728ms delay"),
            (0x6F, "shutdown with retries, 1 retry, 1456ms delay"),
        ];
        
        for (response, expected) in delays {
            assert_eq!(
                StatusDecoder::decode_fault_response(response),
                expected,
                "Failed for response byte 0x{:02X}",
                response
            );
        }
        
        // Test different retry counts
        // 0x73 = 0111 0011 = type=011 (shutdown with retries), retry=10 (2), delay=011 (91ms)
        assert_eq!(
            StatusDecoder::decode_fault_response(0x73),
            "shutdown with retries, 2 retries, 91ms delay"
        );
        // 0x7B = 0111 1011 = type=011 (shutdown with retries), retry=11 (3), delay=011 (91ms) 
        assert_eq!(
            StatusDecoder::decode_fault_response(0x7B),
            "shutdown with retries, 3 retries, 91ms delay"
        );
        
        // Test continue mode
        assert_eq!(
            StatusDecoder::decode_fault_response(0xA0),
            "continue, no retry"
        );
        assert_eq!(
            StatusDecoder::decode_fault_response(0xCB),
            "continue with retries, 1 retry, 91ms delay"
        );
    }

    #[test]
    fn test_status_word_combinations() {
        // Test no faults - should include NONE
        let no_faults = status_word::NONE;
        let desc = StatusDecoder::decode_status_word(no_faults);
        assert_eq!(desc.len(), 1);
        assert!(desc.contains(&"NONE_OF_THE_ABOVE"));
        
        // Test multiple simultaneous faults
        let multi_fault = status_word::VOUT_OV | status_word::IOUT_OC | status_word::TEMP;
        let desc = StatusDecoder::decode_status_word(multi_fault);
        assert!(desc.contains(&"VOUT_OV fault"));
        assert!(desc.contains(&"IOUT_OC fault"));
        assert!(desc.contains(&"TEMP fault/warning"));
        assert_eq!(desc.len(), 3);
        
        // Test all voltage-related faults
        let voltage_faults = status_word::VOUT | status_word::INPUT | status_word::VIN_UV;
        let desc = StatusDecoder::decode_status_word(voltage_faults);
        assert!(desc.contains(&"VOUT fault/warning"));
        assert!(desc.contains(&"INPUT fault/warning"));
        assert!(desc.contains(&"VIN_UV fault"));
        
        // Test status bits (not faults)
        let status_bits = status_word::PGOOD | status_word::OFF | status_word::BUSY;
        let desc = StatusDecoder::decode_status_word(status_bits);
        assert!(desc.contains(&"PGOOD"));
        assert!(desc.contains(&"OFF"));
        assert!(desc.contains(&"BUSY"));
    }

    #[test]
    fn test_status_vout_all_conditions() {
        // Test each STATUS_VOUT bit individually
        let conditions = [
            (status_vout::VOUT_OV_FAULT, "OV fault"),
            (status_vout::VOUT_OV_WARN, "OV warning"),
            (status_vout::VOUT_UV_WARN, "UV warning"),
            (status_vout::VOUT_UV_FAULT, "UV fault"),
            (status_vout::VOUT_MAX, "at MAX"),
            (status_vout::TON_MAX_FAULT, "failed to start"),
            (status_vout::VOUT_MIN, "at MIN"),
        ];
        
        for (bit, expected) in conditions {
            let desc = StatusDecoder::decode_status_vout(bit);
            assert_eq!(desc.len(), 1);
            assert_eq!(desc[0], expected);
        }
        
        // Test combination of overvoltage and undervoltage
        let ov_uv = status_vout::VOUT_OV_WARN | status_vout::VOUT_UV_WARN;
        let desc = StatusDecoder::decode_status_vout(ov_uv);
        assert_eq!(desc.len(), 2);
        assert!(desc.contains(&"OV warning"));
        assert!(desc.contains(&"UV warning"));
    }

    #[test]
    fn test_status_input_conditions() {
        // Test undervoltage + overvoltage combination (shouldn't happen but test anyway)
        let uv_ov = status_input::VIN_UV_FAULT | status_input::VIN_OV_FAULT;
        let desc = StatusDecoder::decode_status_input(uv_ov);
        assert!(desc.contains(&"VIN OV fault"));
        assert!(desc.contains(&"VIN UV fault"));
        
        // Test unit off due to low input
        let off_low = status_input::UNIT_OFF_VIN_LOW | status_input::VIN_UV_FAULT;
        let desc = StatusDecoder::decode_status_input(off_low);
        assert!(desc.contains(&"off due to low VIN"));
        assert!(desc.contains(&"VIN UV fault"));
    }

    #[test]
    fn test_status_temperature_all_bits() {
        // Test all temperature warning/fault combinations
        let all_temp = status_temperature::OT_FAULT | status_temperature::OT_WARN 
                     | status_temperature::UT_WARN | status_temperature::UT_FAULT;
        let desc = StatusDecoder::decode_status_temp(all_temp);
        assert_eq!(desc.len(), 4);
        assert!(desc.contains(&"overtemp fault"));
        assert!(desc.contains(&"overtemp warning"));
        assert!(desc.contains(&"undertemp warning"));
        assert!(desc.contains(&"undertemp fault"));
    }
}