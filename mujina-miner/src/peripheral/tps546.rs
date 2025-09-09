//! TPS546D24A Power Management Controller Driver
//!
//! This module provides a driver for the Texas Instruments TPS546D24A
//! synchronous buck converter with PMBus interface.
//!
//! Datasheet: <https://www.ti.com/lit/ds/symlink/tps546d24a.pdf>

use crate::hw_trait::I2c;
use anyhow::{bail, Result};
use thiserror::Error;
use tracing::{debug, error, info, warn};

/// TPS546 I2C address
const TPS546_I2C_ADDR: u8 = 0x24;

/// PMBus Commands
mod pmbus {
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
    #[expect(dead_code, reason = "Part of PMBus standard, may be used in future")]
    pub const STATUS_OTHER: u8 = 0x7F;
    #[expect(dead_code, reason = "Part of PMBus standard, may be used in future")]
    pub const STATUS_MFR_SPECIFIC: u8 = 0x80;
    pub const READ_VIN: u8 = 0x88;
    pub const READ_VOUT: u8 = 0x8B;
    pub const READ_IOUT: u8 = 0x8C;
    pub const READ_TEMPERATURE_1: u8 = 0x8D;
    #[expect(dead_code, reason = "Manufacturer ID register, may be used for device identification")]
    pub const MFR_ID: u8 = 0x99;
    #[expect(dead_code, reason = "Manufacturer model register, may be used for device identification")]
    pub const MFR_MODEL: u8 = 0x9A;
    #[expect(dead_code, reason = "Manufacturer revision register, may be used for device identification")]
    pub const MFR_REVISION: u8 = 0x9B;
    pub const IC_DEVICE_ID: u8 = 0xAD;
    pub const COMPENSATION_CONFIG: u8 = 0xB1;
    pub const SYNC_CONFIG: u8 = 0xE4;
    pub const STACK_CONFIG: u8 = 0xEC;
    pub const PIN_DETECT_OVERRIDE: u8 = 0xEE;
    pub const INTERLEAVE: u8 = 0x37;
}


/// STATUS_WORD bits (Table 8-13 in datasheet)
mod status {
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
    pub const NONE: u16 = 0x0001;      // Bit 0: No faults (datasheet calls this NONE_OF_THE_ABOVE)
}

/// STATUS_VOUT bits (Table 8-17 in datasheet)
mod status_vout {
    pub const VOUT_OV_FAULT: u8 = 0x80;    // Bit 7: Output overvoltage fault
    pub const VOUT_OV_WARN: u8 = 0x40;     // Bit 6: Output overvoltage warning
    pub const VOUT_UV_WARN: u8 = 0x20;     // Bit 5: Output undervoltage warning
    pub const VOUT_UV_FAULT: u8 = 0x10;    // Bit 4: Output undervoltage fault
    pub const VOUT_MAX: u8 = 0x08;         // Bit 3: VOUT at max (tracking or margin)
    pub const TON_MAX_FAULT: u8 = 0x02;    // Bit 1: Unit did not power up
    pub const VOUT_MIN: u8 = 0x01;         // Bit 0: VOUT at min (tracking)
}

/// STATUS_IOUT bits (Table 8-18 in datasheet)
mod status_iout {
    pub const IOUT_OC_FAULT: u8 = 0x80;    // Bit 7: Output overcurrent fault
    pub const IOUT_OC_LV_FAULT: u8 = 0x40; // Bit 6: Output OC and low voltage fault
    pub const IOUT_OC_WARN: u8 = 0x20;     // Bit 5: Output overcurrent warning
    pub const IOUT_UC_FAULT: u8 = 0x10;    // Bit 4: Output undercurrent fault
    pub const CURR_SHARE_FAULT: u8 = 0x08; // Bit 3: Current share fault
    pub const IN_PWR_LIM: u8 = 0x04;       // Bit 2: Unit in power limiting mode
    pub const POUT_OP_FAULT: u8 = 0x02;    // Bit 1: Output overpower fault
    pub const POUT_OP_WARN: u8 = 0x01;     // Bit 0: Output overpower warning
}

/// STATUS_INPUT bits (Table 8-19 in datasheet)
mod status_input {
    pub const VIN_OV_FAULT: u8 = 0x80;     // Bit 7: Input overvoltage fault
    pub const VIN_OV_WARN: u8 = 0x40;      // Bit 6: Input overvoltage warning
    pub const VIN_UV_WARN: u8 = 0x20;      // Bit 5: Input undervoltage warning
    pub const VIN_UV_FAULT: u8 = 0x10;     // Bit 4: Input undervoltage fault
    pub const UNIT_OFF_VIN_LOW: u8 = 0x08; // Bit 3: Unit off for insufficient input
    pub const IIN_OC_FAULT: u8 = 0x04;     // Bit 2: Input overcurrent fault
    pub const IIN_OC_WARN: u8 = 0x02;      // Bit 1: Input overcurrent warning
    pub const PIN_OP_WARN: u8 = 0x01;      // Bit 0: Input overpower warning
}

/// STATUS_TEMPERATURE bits (Table 8-20 in datasheet)
mod status_temp {
    pub const OT_FAULT: u8 = 0x80;         // Bit 7: Overtemperature fault
    pub const OT_WARN: u8 = 0x40;          // Bit 6: Overtemperature warning
    pub const UT_WARN: u8 = 0x20;          // Bit 5: Undertemperature warning
    pub const UT_FAULT: u8 = 0x10;         // Bit 4: Undertemperature fault
}

/// STATUS_CML bits (Table 8-21 in datasheet)
mod status_cml {
    pub const INVALID_CMD: u8 = 0x80;      // Bit 7: Invalid/unsupported command
    pub const INVALID_DATA: u8 = 0x40;     // Bit 6: Invalid/unsupported data
    pub const PEC_FAULT: u8 = 0x20;        // Bit 5: Packet Error Check failed
    pub const MEMORY_FAULT: u8 = 0x10;     // Bit 4: Memory fault detected
    pub const PROCESSOR_FAULT: u8 = 0x08;  // Bit 3: Processor fault detected
    pub const OTHER_COMM_FAULT: u8 = 0x02; // Bit 1: Other communication fault
    pub const OTHER_MEM_LOGIC: u8 = 0x01;  // Bit 0: Other memory or logic fault
}

/// OPERATION command values (Table 8-2 in datasheet)
mod operation {
    pub const OFF_IMMEDIATE: u8 = 0x00;    // Turn off immediately
    pub const SOFT_OFF: u8 = 0x40;         // Soft off (using programmed delays)
    pub const ON_MARGIN_LOW: u8 = 0x98;    // On with margin low
    pub const ON_MARGIN_HIGH: u8 = 0xA8;   // On with margin high  
    pub const ON: u8 = 0x80;               // Turn on
}

/// ON_OFF_CONFIG bits (Table 8-3 in datasheet)
mod on_off_config {
    pub const PU: u8 = 0x10;               // Bit 4: Power-up from CONTROL pin
    pub const CMD: u8 = 0x08;              // Bit 3: Respond to OPERATION command
    pub const CP: u8 = 0x04;               // Bit 2: Control pin present
    pub const POLARITY: u8 = 0x02;         // Bit 1: Control pin polarity (1=active high)
    pub const DELAY: u8 = 0x01;            // Bit 0: Turn off delay (0=disabled)
}

/// Expected device IDs for TPS546D24A variants
const DEVICE_ID1: [u8; 6] = [0x54, 0x49, 0x54, 0x6B, 0x24, 0x41]; // TPS546D24A
const DEVICE_ID2: [u8; 6] = [0x54, 0x49, 0x54, 0x6D, 0x24, 0x41]; // TPS546D24A
const DEVICE_ID3: [u8; 6] = [0x54, 0x49, 0x54, 0x6D, 0x24, 0x62]; // TPS546D24S

/// TPS546 configuration parameters
#[derive(Debug, Clone)]
pub struct Tps546Config {
    /// Input voltage turn-on threshold (V)
    pub vin_on: f32,
    /// Input voltage turn-off threshold (V)
    pub vin_off: f32,
    /// Input undervoltage warning limit (V)
    pub vin_uv_warn_limit: f32,
    /// Input overvoltage fault limit (V)
    pub vin_ov_fault_limit: f32,
    /// Output voltage scale factor
    pub vout_scale_loop: f32,
    /// Minimum output voltage (V)
    pub vout_min: f32,
    /// Maximum output voltage (V)
    pub vout_max: f32,
    /// Initial output voltage command (V)
    pub vout_command: f32,
    /// Output current overcurrent warning limit (A)
    pub iout_oc_warn_limit: f32,
    /// Output current overcurrent fault limit (A)
    pub iout_oc_fault_limit: f32,
}

impl Tps546Config {
    /// Configuration for Bitaxe Gamma (single ASIC)
    pub fn bitaxe_gamma() -> Self {
        Self {
            vin_on: 4.8,
            vin_off: 4.5,
            vin_uv_warn_limit: 0.0, // Disabled due to TI bug
            vin_ov_fault_limit: 6.5,
            vout_scale_loop: 0.25,
            vout_min: 1.0,
            vout_max: 2.0,
            vout_command: 1.15,  // BM1370 default voltage
            iout_oc_warn_limit: 25.0,
            iout_oc_fault_limit: 30.0,
        }
    }
}

/// TPS546 error types
#[derive(Error, Debug)]
pub enum Tps546Error {
    #[error("Device ID mismatch")]
    DeviceIdMismatch,
    #[error("Voltage out of range: {0:.2}V (min: {1:.2}V, max: {2:.2}V)")]
    VoltageOutOfRange(f32, f32, f32),
    #[error("PMBus fault detected: {0}")]
    FaultDetected(String),
}

/// TPS546D24A driver
pub struct Tps546<I2C> {
    i2c: I2C,
    config: Tps546Config,
}

impl<I2C: I2c> Tps546<I2C> {
    /// Create a new TPS546 instance
    pub fn new(i2c: I2C, config: Tps546Config) -> Self {
        Self { i2c, config }
    }

    /// Initialize the TPS546
    pub async fn init(&mut self) -> Result<()> {
        info!("Initializing TPS546D24A power regulator");

        // First verify device ID to ensure I2C communication is working
        self.verify_device_id().await?;
        
        // Turn off output during configuration (esp-miner does this first)
        self.write_byte(pmbus::OPERATION, operation::OFF_IMMEDIATE).await?;
        info!("Power output turned off");

        // Configure ON_OFF_CONFIG immediately after turning off (esp-miner sequence)
        let on_off_val = on_off_config::DELAY
            | on_off_config::POLARITY
            | on_off_config::CP
            | on_off_config::CMD
            | on_off_config::PU;
        self.write_byte(pmbus::ON_OFF_CONFIG, on_off_val).await?;
        let mut config_desc = Vec::new();
        if on_off_val & on_off_config::PU != 0 { config_desc.push("PowerUp from CONTROL"); }
        if on_off_val & on_off_config::CMD != 0 { config_desc.push("OPERATION cmd enabled"); }
        if on_off_val & on_off_config::CP != 0 { config_desc.push("CONTROL pin present"); }
        if on_off_val & on_off_config::POLARITY != 0 { config_desc.push("Active high"); }
        if on_off_val & on_off_config::DELAY != 0 { config_desc.push("Turn-off delay enabled"); }
        info!("ON_OFF_CONFIG set to 0x{:02X} ({})", on_off_val, config_desc.join(", "));

        // Read VOUT_MODE to verify data format (esp-miner does this)
        let vout_mode = self.read_byte(pmbus::VOUT_MODE).await?;
        info!("VOUT_MODE: 0x{:02X}", vout_mode);
        
        // Write entire configuration like esp-miner does
        self.write_config().await?;
        
        // Read back STATUS_WORD for verification like esp-miner does
        let status = self.read_word(pmbus::STATUS_WORD).await?;
        let status_desc = self.decode_status_word(status);
        if status_desc.is_empty() {
            info!("STATUS_WORD after config: 0x{:04X}", status);
        } else {
            info!("STATUS_WORD after config: 0x{:04X} ({})", status, status_desc.join(", "));
        }
        
        // Log current readings like esp-miner does
        self.log_initial_readings().await?;
        
        info!("TPS546D24A initialization complete");
        Ok(())
    }

    /// Write all configuration parameters
    async fn write_config(&mut self) -> Result<()> {
        info!("---Writing new config values to TPS546---");

        // Phase configuration
        info!("Setting PHASE: 00");
        self.write_byte(pmbus::PHASE, 0x00).await?;

        // Switching frequency (650 kHz)
        info!("Setting FREQUENCY: 650kHz");
        self.write_word(pmbus::FREQUENCY_SWITCH, self.int_to_slinear11(650))
            .await?;

        // Input voltage thresholds (handle UV_WARN_LIMIT bug like esp-miner)
        if self.config.vin_uv_warn_limit > 0.0 {
            info!("Setting VIN_UV_WARN_LIMIT: {:.2}V", self.config.vin_uv_warn_limit);
            self.write_word(
                pmbus::VIN_UV_WARN_LIMIT,
                self.float_to_slinear11(self.config.vin_uv_warn_limit),
            )
            .await?;
        }

        info!("Setting VIN_ON: {:.2}V", self.config.vin_on);
        self.write_word(pmbus::VIN_ON, self.float_to_slinear11(self.config.vin_on))
            .await?;
            
        info!("Setting VIN_OFF: {:.2}V", self.config.vin_off);
        self.write_word(
            pmbus::VIN_OFF,
            self.float_to_slinear11(self.config.vin_off),
        )
        .await?;
        
        info!("Setting VIN_OV_FAULT_LIMIT: {:.2}V", self.config.vin_ov_fault_limit);
        self.write_word(
            pmbus::VIN_OV_FAULT_LIMIT,
            self.float_to_slinear11(self.config.vin_ov_fault_limit),
        )
        .await?;

        // VIN_OV_FAULT_RESPONSE (0xB7 = shutdown with 4 retries, 182ms delay)
        const VIN_OV_FAULT_RESPONSE: u8 = 0xB7;
        info!("Setting VIN_OV_FAULT_RESPONSE: 0x{:02X} (shutdown, 4 retries, 182ms delay)", VIN_OV_FAULT_RESPONSE);
        self.write_byte(pmbus::VIN_OV_FAULT_RESPONSE, VIN_OV_FAULT_RESPONSE)
            .await?;

        // Output voltage configuration
        info!("Setting VOUT SCALE: {:.2}", self.config.vout_scale_loop);
        self.write_word(
            pmbus::VOUT_SCALE_LOOP,
            self.float_to_slinear11(self.config.vout_scale_loop),
        )
        .await?;
        
        info!("Setting VOUT_COMMAND: {:.2}V", self.config.vout_command);
        let vout_command = self.float_to_ulinear16(self.config.vout_command).await?;
        self.write_word(pmbus::VOUT_COMMAND, vout_command).await?;
        
        info!("Setting VOUT_MAX: {:.2}V", self.config.vout_max);
        let vout_max = self.float_to_ulinear16(self.config.vout_max).await?;
        self.write_word(pmbus::VOUT_MAX, vout_max).await?;
        
        info!("Setting VOUT_MIN: {:.2}V", self.config.vout_min);
        let vout_min = self.float_to_ulinear16(self.config.vout_min).await?;
        self.write_word(pmbus::VOUT_MIN, vout_min).await?;

        // Output voltage protection
        const VOUT_OV_FAULT_LIMIT: f32 = 1.25; // 125% of VOUT_COMMAND
        const VOUT_OV_WARN_LIMIT: f32 = 1.16; // 116% of VOUT_COMMAND
        const VOUT_MARGIN_HIGH: f32 = 1.10; // 110% of VOUT_COMMAND  
        const VOUT_MARGIN_LOW: f32 = 0.90; // 90% of VOUT_COMMAND
        const VOUT_UV_WARN_LIMIT: f32 = 0.90; // 90% of VOUT_COMMAND
        const VOUT_UV_FAULT_LIMIT: f32 = 0.75; // 75% of VOUT_COMMAND

        info!("Setting VOUT_OV_FAULT_LIMIT: {:.2}", VOUT_OV_FAULT_LIMIT);
        let vout_ov_fault = self.float_to_ulinear16(VOUT_OV_FAULT_LIMIT).await?;
        self.write_word(pmbus::VOUT_OV_FAULT_LIMIT, vout_ov_fault).await?;
        
        info!("Setting VOUT_OV_WARN_LIMIT: {:.2}", VOUT_OV_WARN_LIMIT);
        let vout_ov_warn = self.float_to_ulinear16(VOUT_OV_WARN_LIMIT).await?;
        self.write_word(pmbus::VOUT_OV_WARN_LIMIT, vout_ov_warn).await?;
        
        info!("Setting VOUT_MARGIN_HIGH: {:.2}", VOUT_MARGIN_HIGH);
        let vout_margin_high = self.float_to_ulinear16(VOUT_MARGIN_HIGH).await?;
        self.write_word(pmbus::VOUT_MARGIN_HIGH, vout_margin_high).await?;
        
        info!("Setting VOUT_MARGIN_LOW: {:.2}", VOUT_MARGIN_LOW);
        let vout_margin_low = self.float_to_ulinear16(VOUT_MARGIN_LOW).await?;
        self.write_word(pmbus::VOUT_MARGIN_LOW, vout_margin_low).await?;
        
        info!("Setting VOUT_UV_WARN_LIMIT: {:.2}", VOUT_UV_WARN_LIMIT);
        let vout_uv_warn = self.float_to_ulinear16(VOUT_UV_WARN_LIMIT).await?;
        self.write_word(pmbus::VOUT_UV_WARN_LIMIT, vout_uv_warn).await?;
        
        info!("Setting VOUT_UV_FAULT_LIMIT: {:.2}", VOUT_UV_FAULT_LIMIT);
        let vout_uv_fault = self.float_to_ulinear16(VOUT_UV_FAULT_LIMIT).await?;
        self.write_word(pmbus::VOUT_UV_FAULT_LIMIT, vout_uv_fault).await?;

        // Output current protection
        info!("----- IOUT");
        info!("Setting IOUT_OC_WARN_LIMIT: {:.2}A", self.config.iout_oc_warn_limit);
        self.write_word(
            pmbus::IOUT_OC_WARN_LIMIT,
            self.float_to_slinear11(self.config.iout_oc_warn_limit),
        )
        .await?;
        
        info!("Setting IOUT_OC_FAULT_LIMIT: {:.2}A", self.config.iout_oc_fault_limit);
        self.write_word(
            pmbus::IOUT_OC_FAULT_LIMIT,
            self.float_to_slinear11(self.config.iout_oc_fault_limit),
        )
        .await?;

        // IOUT_OC_FAULT_RESPONSE (0xC0 = shutdown immediately, no retries)
        const IOUT_OC_FAULT_RESPONSE: u8 = 0xC0;
        info!("Setting IOUT_OC_FAULT_RESPONSE: 0x{:02X} (shutdown immediately, no retries)", IOUT_OC_FAULT_RESPONSE);
        self.write_byte(pmbus::IOUT_OC_FAULT_RESPONSE, IOUT_OC_FAULT_RESPONSE)
            .await?;

        // Temperature protection
        info!("----- TEMPERATURE");
        const OT_WARN_LIMIT: i32 = 105; // °C
        const OT_FAULT_LIMIT: i32 = 145; // °C
        const OT_FAULT_RESPONSE: u8 = 0xFF; // Infinite retries

        info!("Setting OT_WARN_LIMIT: {}°C", OT_WARN_LIMIT);
        self.write_word(pmbus::OT_WARN_LIMIT, self.int_to_slinear11(OT_WARN_LIMIT))
            .await?;
        info!("Setting OT_FAULT_LIMIT: {}°C", OT_FAULT_LIMIT);
        self.write_word(pmbus::OT_FAULT_LIMIT, self.int_to_slinear11(OT_FAULT_LIMIT))
            .await?;
        info!("Setting OT_FAULT_RESPONSE: 0x{:02X} (infinite retries, wait for cooling)", OT_FAULT_RESPONSE);
        self.write_byte(pmbus::OT_FAULT_RESPONSE, OT_FAULT_RESPONSE)
            .await?;

        // Timing configuration
        info!("----- TIMING");
        const TON_DELAY: i32 = 0;
        const TON_RISE: i32 = 3;
        const TON_MAX_FAULT_LIMIT: i32 = 0;
        const TON_MAX_FAULT_RESPONSE: u8 = 0x3B; // 3 retries, 91ms delay
        const TOFF_DELAY: i32 = 0;
        const TOFF_FALL: i32 = 0;

        info!("Setting TON_DELAY: {}ms", TON_DELAY);
        self.write_word(pmbus::TON_DELAY, self.int_to_slinear11(TON_DELAY))
            .await?;
        info!("Setting TON_RISE: {}ms", TON_RISE);
        self.write_word(pmbus::TON_RISE, self.int_to_slinear11(TON_RISE))
            .await?;
        info!("Setting TON_MAX_FAULT_LIMIT: {}ms", TON_MAX_FAULT_LIMIT);
        self.write_word(
            pmbus::TON_MAX_FAULT_LIMIT,
            self.int_to_slinear11(TON_MAX_FAULT_LIMIT),
        )
        .await?;
        info!("Setting TON_MAX_FAULT_RESPONSE: 0x{:02X} (3 retries, 91ms delay)", TON_MAX_FAULT_RESPONSE);
        self.write_byte(pmbus::TON_MAX_FAULT_RESPONSE, TON_MAX_FAULT_RESPONSE)
            .await?;
        info!("Setting TOFF_DELAY: {}ms", TOFF_DELAY);
        self.write_word(pmbus::TOFF_DELAY, self.int_to_slinear11(TOFF_DELAY))
            .await?;
        info!("Setting TOFF_FALL: {}ms", TOFF_FALL);
        self.write_word(pmbus::TOFF_FALL, self.int_to_slinear11(TOFF_FALL))
            .await?;

        // Pin detect override
        info!("Setting PIN_DETECT_OVERRIDE");
        const PIN_DETECT_OVERRIDE: u16 = 0xFFFF;
        self.write_word(pmbus::PIN_DETECT_OVERRIDE, PIN_DETECT_OVERRIDE)
            .await?;

        info!("TPS546 configuration written successfully");
        Ok(())
    }

    /// Verify the device ID
    async fn verify_device_id(&mut self) -> Result<()> {
        let mut id_data = vec![0u8; 7]; // Length byte + 6 ID bytes
        self.i2c
            .write_read(TPS546_I2C_ADDR, &[pmbus::IC_DEVICE_ID], &mut id_data)
            .await?;

        // First byte is length, actual ID starts at byte 1
        let device_id = &id_data[1..7];
        debug!(
            "Device ID: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
            device_id[0], device_id[1], device_id[2], device_id[3], device_id[4], device_id[5]
        );

        if device_id != DEVICE_ID1 && device_id != DEVICE_ID2 && device_id != DEVICE_ID3 {
            error!("Device ID mismatch");
            bail!(Tps546Error::DeviceIdMismatch);
        }

        Ok(())
    }

    /// Log initial readings like esp-miner does for verification
    async fn log_initial_readings(&mut self) -> Result<()> {
        info!("-----------VOLTAGE/CURRENT---------------------");
        
        // Read input voltage
        match self.read_word(pmbus::READ_VIN).await {
            Ok(value) => {
                let vin = self.slinear11_to_float(value);
                info!("read READ_VIN: {:.2}V", vin);
            }
            Err(e) => warn!("Failed to read VIN: {}", e),
        }
        
        // Read output current
        match self.read_word(pmbus::READ_IOUT).await {
            Ok(value) => {
                let iout = self.slinear11_to_float(value);
                info!("read READ_IOUT: {:.2}A", iout);
            }
            Err(e) => warn!("Failed to read IOUT: {}", e),
        }
        
        // Read output voltage
        match self.read_word(pmbus::READ_VOUT).await {
            Ok(value) => {
                match self.ulinear16_to_float(value).await {
                    Ok(vout) => info!("read READ_VOUT: {:.2}V", vout),
                    Err(e) => warn!("Failed to convert VOUT: {}", e),
                }
            }
            Err(e) => warn!("Failed to read VOUT: {}", e),
        }
        
        Ok(())
    }

    /// Clear all faults
    pub async fn clear_faults(&mut self) -> Result<()> {
        self.i2c
            .write(TPS546_I2C_ADDR, &[pmbus::CLEAR_FAULTS])
            .await?;
        Ok(())
    }

    /// Set output voltage
    pub async fn set_vout(&mut self, volts: f32) -> Result<()> {
        if volts == 0.0 {
            // Turn off output
            self.write_byte(pmbus::OPERATION, operation::OFF_IMMEDIATE).await?;
            info!("Output voltage turned off");
        } else {
            // Check voltage range
            if volts < self.config.vout_min || volts > self.config.vout_max {
                bail!(Tps546Error::VoltageOutOfRange(
                    volts,
                    self.config.vout_min,
                    self.config.vout_max
                ));
            }

            // Set voltage
            let value = self.float_to_ulinear16(volts).await?;
            self.write_word(pmbus::VOUT_COMMAND, value).await?;
            info!("Output voltage set to {:.2}V", volts);

            // Turn on output
            self.write_byte(pmbus::OPERATION, operation::ON).await?;

            // Verify operation
            let op_val = self.read_byte(pmbus::OPERATION).await?;
            if op_val != operation::ON {
                error!("Failed to turn on output, OPERATION = 0x{:02X}", op_val);
            }
        }
        Ok(())
    }

    /// Read input voltage in millivolts
    pub async fn get_vin(&mut self) -> Result<u32> {
        let value = self.read_word(pmbus::READ_VIN).await?;
        let volts = self.slinear11_to_float(value);
        Ok((volts * 1000.0) as u32)
    }

    /// Read output voltage in millivolts
    pub async fn get_vout(&mut self) -> Result<u32> {
        let value = self.read_word(pmbus::READ_VOUT).await?;
        let volts = self.ulinear16_to_float(value).await?;
        Ok((volts * 1000.0) as u32)
    }

    /// Read output current in milliamps
    pub async fn get_iout(&mut self) -> Result<u32> {
        // Set phase to 0xFF to read all phases
        self.write_byte(pmbus::PHASE, 0xFF).await?;
        
        let value = self.read_word(pmbus::READ_IOUT).await?;
        let amps = self.slinear11_to_float(value);
        Ok((amps * 1000.0) as u32)
    }

    /// Read temperature in degrees Celsius
    pub async fn get_temperature(&mut self) -> Result<i32> {
        let value = self.read_word(pmbus::READ_TEMPERATURE_1).await?;
        Ok(self.slinear11_to_int(value))
    }

    /// Calculate power in milliwatts
    pub async fn get_power(&mut self) -> Result<u32> {
        let vout_mv = self.get_vout().await?;
        let iout_ma = self.get_iout().await?;
        let power_mw = (vout_mv as u64 * iout_ma as u64) / 1000;
        Ok(power_mw as u32)
    }

    /// Check and report status
    pub async fn check_status(&mut self) -> Result<()> {
        let status = self.read_word(pmbus::STATUS_WORD).await?;
        
        if status == 0 {
            return Ok(());
        }

        // Check for faults
        if status & status::VOUT != 0 {
            let vout_status = self.read_byte(pmbus::STATUS_VOUT).await?;
            let desc = self.decode_status_vout(vout_status);
            warn!("VOUT status error: 0x{:02X} ({})", vout_status, desc.join(", "));
        }

        if status & status::IOUT != 0 {
            let iout_status = self.read_byte(pmbus::STATUS_IOUT).await?;
            let desc = self.decode_status_iout(iout_status);
            warn!("IOUT status error: 0x{:02X} ({})", iout_status, desc.join(", "));
        }

        if status & status::INPUT != 0 {
            let input_status = self.read_byte(pmbus::STATUS_INPUT).await?;
            let desc = self.decode_status_input(input_status);
            warn!("INPUT status error: 0x{:02X} ({})", input_status, desc.join(", "));
        }

        if status & status::TEMP != 0 {
            let temp_status = self.read_byte(pmbus::STATUS_TEMPERATURE).await?;
            let desc = self.decode_status_temp(temp_status);
            warn!("TEMPERATURE status error: 0x{:02X} ({})", temp_status, desc.join(", "));
        }

        if status & status::CML != 0 {
            let cml_status = self.read_byte(pmbus::STATUS_CML).await?;
            let desc = self.decode_status_cml(cml_status);
            warn!("CML status error: 0x{:02X} ({})", cml_status, desc.join(", "));
        }

        Ok(())
    }

    /// Dump the complete TPS546 configuration for debugging
    pub async fn dump_configuration(&mut self) -> Result<()> {
        debug!("=== TPS546D24A Configuration Dump ===");
        
        // Voltage Configuration
        debug!("--- Voltage Configuration ---");
        
        // VIN settings
        let vin_on = self.read_word(pmbus::VIN_ON).await?;
        debug!("VIN_ON: {:.2}V (raw: 0x{:04X})", 
            self.slinear11_to_float(vin_on), vin_on);
        
        let vin_off = self.read_word(pmbus::VIN_OFF).await?;
        debug!("VIN_OFF: {:.2}V (raw: 0x{:04X})", 
            self.slinear11_to_float(vin_off), vin_off);
        
        let vin_ov_fault = self.read_word(pmbus::VIN_OV_FAULT_LIMIT).await?;
        debug!("VIN_OV_FAULT_LIMIT: {:.2}V (raw: 0x{:04X})", 
            self.slinear11_to_float(vin_ov_fault), vin_ov_fault);
        
        let vin_uv_warn = self.read_word(pmbus::VIN_UV_WARN_LIMIT).await?;
        debug!("VIN_UV_WARN_LIMIT: {:.2}V (raw: 0x{:04X})", 
            self.slinear11_to_float(vin_uv_warn), vin_uv_warn);
        
        let vin_ov_response = self.read_byte(pmbus::VIN_OV_FAULT_RESPONSE).await?;
        let vin_ov_desc = self.decode_fault_response(vin_ov_response);
        debug!("VIN_OV_FAULT_RESPONSE: 0x{:02X} ({})", vin_ov_response, vin_ov_desc);
        
        // VOUT settings
        let vout_max = self.read_word(pmbus::VOUT_MAX).await?;
        debug!("VOUT_MAX: {:.2}V (raw: 0x{:04X})", 
            self.ulinear16_to_float(vout_max).await?, vout_max);
        
        let vout_ov_fault = self.read_word(pmbus::VOUT_OV_FAULT_LIMIT).await?;
        let vout_ov_fault_v = self.ulinear16_to_float(vout_ov_fault).await?;
        debug!("VOUT_OV_FAULT_LIMIT: {:.2}V (raw: 0x{:04X})", 
            vout_ov_fault_v * self.config.vout_command, vout_ov_fault);
        
        let vout_ov_warn = self.read_word(pmbus::VOUT_OV_WARN_LIMIT).await?;
        let vout_ov_warn_v = self.ulinear16_to_float(vout_ov_warn).await?;
        debug!("VOUT_OV_WARN_LIMIT: {:.2}V (raw: 0x{:04X})", 
            vout_ov_warn_v * self.config.vout_command, vout_ov_warn);
        
        let vout_margin_high = self.read_word(pmbus::VOUT_MARGIN_HIGH).await?;
        let vout_margin_high_v = self.ulinear16_to_float(vout_margin_high).await?;
        debug!("VOUT_MARGIN_HIGH: {:.2}V (raw: 0x{:04X})", 
            vout_margin_high_v * self.config.vout_command, vout_margin_high);
        
        let vout_command = self.read_word(pmbus::VOUT_COMMAND).await?;
        debug!("VOUT_COMMAND: {:.2}V (raw: 0x{:04X})", 
            self.ulinear16_to_float(vout_command).await?, vout_command);
        
        let vout_margin_low = self.read_word(pmbus::VOUT_MARGIN_LOW).await?;
        let vout_margin_low_v = self.ulinear16_to_float(vout_margin_low).await?;
        debug!("VOUT_MARGIN_LOW: {:.2}V (raw: 0x{:04X})", 
            vout_margin_low_v * self.config.vout_command, vout_margin_low);
        
        let vout_uv_warn = self.read_word(pmbus::VOUT_UV_WARN_LIMIT).await?;
        let vout_uv_warn_v = self.ulinear16_to_float(vout_uv_warn).await?;
        debug!("VOUT_UV_WARN_LIMIT: {:.2}V (raw: 0x{:04X})", 
            vout_uv_warn_v * self.config.vout_command, vout_uv_warn);
        
        let vout_uv_fault = self.read_word(pmbus::VOUT_UV_FAULT_LIMIT).await?;
        let vout_uv_fault_v = self.ulinear16_to_float(vout_uv_fault).await?;
        debug!("VOUT_UV_FAULT_LIMIT: {:.2}V (raw: 0x{:04X})", 
            vout_uv_fault_v * self.config.vout_command, vout_uv_fault);
        
        let vout_min = self.read_word(pmbus::VOUT_MIN).await?;
        debug!("VOUT_MIN: {:.2}V (raw: 0x{:04X})", 
            self.ulinear16_to_float(vout_min).await?, vout_min);
        
        // Current Configuration and Limits
        debug!("--- Current Configuration ---");
        
        let iout_oc_warn = self.read_word(pmbus::IOUT_OC_WARN_LIMIT).await?;
        debug!("IOUT_OC_WARN_LIMIT: {:.2}A (raw: 0x{:04X})", 
            self.slinear11_to_float(iout_oc_warn), iout_oc_warn);
        
        let iout_oc_fault = self.read_word(pmbus::IOUT_OC_FAULT_LIMIT).await?;
        debug!("IOUT_OC_FAULT_LIMIT: {:.2}A (raw: 0x{:04X})", 
            self.slinear11_to_float(iout_oc_fault), iout_oc_fault);
        
        let iout_oc_response = self.read_byte(pmbus::IOUT_OC_FAULT_RESPONSE).await?;
        let iout_oc_desc = self.decode_fault_response(iout_oc_response);
        debug!("IOUT_OC_FAULT_RESPONSE: 0x{:02X} ({})", iout_oc_response, iout_oc_desc);
        
        // Temperature Configuration
        debug!("--- Temperature Configuration ---");
        
        let ot_warn = self.read_word(pmbus::OT_WARN_LIMIT).await?;
        debug!("OT_WARN_LIMIT: {}°C (raw: 0x{:04X})", 
            self.slinear11_to_int(ot_warn), ot_warn);
        
        let ot_fault = self.read_word(pmbus::OT_FAULT_LIMIT).await?;
        debug!("OT_FAULT_LIMIT: {}°C (raw: 0x{:04X})", 
            self.slinear11_to_int(ot_fault), ot_fault);
        
        let ot_response = self.read_byte(pmbus::OT_FAULT_RESPONSE).await?;
        let ot_desc = self.decode_fault_response(ot_response);
        debug!("OT_FAULT_RESPONSE: 0x{:02X} ({})", ot_response, ot_desc);
        
        // Current Readings
        debug!("--- Current Readings ---");
        
        let read_vin = self.read_word(pmbus::READ_VIN).await?;
        debug!("READ_VIN: {:.2}V", self.slinear11_to_float(read_vin));
        
        let read_vout = self.read_word(pmbus::READ_VOUT).await?;
        debug!("READ_VOUT: {:.2}V", self.ulinear16_to_float(read_vout).await?);
        
        let read_iout = self.read_word(pmbus::READ_IOUT).await?;
        debug!("READ_IOUT: {:.2}A", self.slinear11_to_float(read_iout));
        
        let read_temp = self.read_word(pmbus::READ_TEMPERATURE_1).await?;
        debug!("READ_TEMPERATURE_1: {}°C", self.slinear11_to_int(read_temp));
        
        // Timing Configuration
        debug!("--- Timing Configuration ---");
        
        let ton_delay = self.read_word(pmbus::TON_DELAY).await?;
        debug!("TON_DELAY: {}ms", self.slinear11_to_int(ton_delay));
        
        let ton_rise = self.read_word(pmbus::TON_RISE).await?;
        debug!("TON_RISE: {}ms", self.slinear11_to_int(ton_rise));
        
        let ton_max_fault = self.read_word(pmbus::TON_MAX_FAULT_LIMIT).await?;
        debug!("TON_MAX_FAULT_LIMIT: {}ms", self.slinear11_to_int(ton_max_fault));
        
        let ton_max_response = self.read_byte(pmbus::TON_MAX_FAULT_RESPONSE).await?;
        let ton_max_desc = self.decode_fault_response(ton_max_response);
        debug!("TON_MAX_FAULT_RESPONSE: 0x{:02X} ({})", ton_max_response, ton_max_desc);
        
        let toff_delay = self.read_word(pmbus::TOFF_DELAY).await?;
        debug!("TOFF_DELAY: {}ms", self.slinear11_to_int(toff_delay));
        
        let toff_fall = self.read_word(pmbus::TOFF_FALL).await?;
        debug!("TOFF_FALL: {}ms", self.slinear11_to_int(toff_fall));
        
        // Operational Configuration
        debug!("--- Operational Configuration ---");
        
        let phase = self.read_byte(pmbus::PHASE).await?;
        let phase_desc = if phase == 0xFF { 
            "all phases".to_string() 
        } else { 
            format!("phase {}", phase) 
        };
        debug!("PHASE: 0x{:02X} ({})", phase, phase_desc);
        
        let stack_config = self.read_word(pmbus::STACK_CONFIG).await?;
        debug!("STACK_CONFIG: 0x{:04X}", stack_config);
        
        let sync_config = self.read_byte(pmbus::SYNC_CONFIG).await?;
        debug!("SYNC_CONFIG: 0x{:02X}", sync_config);
        
        let interleave = self.read_word(pmbus::INTERLEAVE).await?;
        debug!("INTERLEAVE: 0x{:04X}", interleave);
        
        let capability = self.read_byte(pmbus::CAPABILITY).await?;
        let mut cap_desc = Vec::new();
        if capability & 0x80 != 0 { cap_desc.push("PEC supported"); }
        if capability & 0x40 != 0 { cap_desc.push("400kHz max"); }
        if capability & 0x20 != 0 { cap_desc.push("Alert supported"); }
        debug!("CAPABILITY: 0x{:02X} ({})", capability, 
            if cap_desc.is_empty() { "none".to_string() } else { cap_desc.join(", ") });
        
        let op_val = self.read_byte(pmbus::OPERATION).await?;
        let op_desc = match op_val {
            operation::OFF_IMMEDIATE => "OFF (immediate)",
            operation::SOFT_OFF => "SOFT OFF",
            operation::ON => "ON",
            operation::ON_MARGIN_LOW => "ON (margin low)",
            operation::ON_MARGIN_HIGH => "ON (margin high)",
            _ => "unknown",
        };
        debug!("OPERATION: 0x{:02X} ({})", op_val, op_desc);
        
        let on_off_val = self.read_byte(pmbus::ON_OFF_CONFIG).await?;
        let mut on_off_desc = Vec::new();
        if on_off_val & on_off_config::PU != 0 { on_off_desc.push("PowerUp from CONTROL"); }
        if on_off_val & on_off_config::CMD != 0 { on_off_desc.push("CMD enabled"); }
        if on_off_val & on_off_config::CP != 0 { on_off_desc.push("CONTROL present"); }
        if on_off_val & on_off_config::POLARITY != 0 { on_off_desc.push("Active high"); }
        if on_off_val & on_off_config::DELAY != 0 { on_off_desc.push("Turn-off delay"); }
        debug!("ON_OFF_CONFIG: 0x{:02X} ({})", on_off_val, on_off_desc.join(", "));
        
        // Compensation Configuration
        match self.read_block(pmbus::COMPENSATION_CONFIG, 5).await {
            Ok(comp_config) => {
                debug!("COMPENSATION_CONFIG: {:02X?}", comp_config);
            }
            Err(e) => {
                debug!("Failed to read COMPENSATION_CONFIG: {}", e);
            }
        }
        
        // Status Information
        debug!("--- Status Information ---");
        
        let status_word = self.read_word(pmbus::STATUS_WORD).await?;
        let status_desc = self.decode_status_word(status_word);
        
        if status_desc.is_empty() {
            debug!("STATUS_WORD: 0x{:04X} (no flags set)", status_word);
        } else {
            debug!("STATUS_WORD: 0x{:04X} ({})", status_word, status_desc.join(", "));
        }
        
        // Read detailed status registers if main status indicates issues
        if status_word & status::VOUT != 0 {
            let vout_status = self.read_byte(pmbus::STATUS_VOUT).await?;
            let desc = self.decode_status_vout(vout_status);
            debug!("STATUS_VOUT: 0x{:02X} ({})", vout_status, desc.join(", "));
        }
        
        if status_word & status::IOUT != 0 {
            let iout_status = self.read_byte(pmbus::STATUS_IOUT).await?;
            let desc = self.decode_status_iout(iout_status);
            debug!("STATUS_IOUT: 0x{:02X} ({})", iout_status, desc.join(", "));
        }
        
        if status_word & status::INPUT != 0 {
            let input_status = self.read_byte(pmbus::STATUS_INPUT).await?;
            let desc = self.decode_status_input(input_status);
            debug!("STATUS_INPUT: 0x{:02X} ({})", input_status, desc.join(", "));
        }
        
        if status_word & status::TEMP != 0 {
            let temp_status = self.read_byte(pmbus::STATUS_TEMPERATURE).await?;
            let desc = self.decode_status_temp(temp_status);
            debug!("STATUS_TEMPERATURE: 0x{:02X} ({})", temp_status, desc.join(", "));
        }
        
        if status_word & status::CML != 0 {
            let cml_status = self.read_byte(pmbus::STATUS_CML).await?;
            let desc = self.decode_status_cml(cml_status);
            debug!("STATUS_CML: 0x{:02X} ({})", cml_status, desc.join(", "));
        }
        
        debug!("=== End Configuration Dump ===");
        Ok(())
    }

    // Helper methods for decoding status registers
    
    fn decode_status_word(&self, status: u16) -> Vec<&'static str> {
        let mut desc = Vec::new();
        if status & status::VOUT != 0 { desc.push("VOUT fault/warning"); }
        if status & status::IOUT != 0 { desc.push("IOUT fault/warning"); }
        if status & status::INPUT != 0 { desc.push("INPUT fault/warning"); }
        if status & status::MFR != 0 { desc.push("MFR specific"); }
        if status & status::PGOOD != 0 { desc.push("PGOOD"); }
        if status & status::FANS != 0 { desc.push("FAN fault/warning"); }
        if status & status::OTHER != 0 { desc.push("OTHER"); }
        if status & status::UNKNOWN != 0 { desc.push("UNKNOWN"); }
        if status & status::BUSY != 0 { desc.push("BUSY"); }
        if status & status::OFF != 0 { desc.push("OFF"); }
        if status & status::VOUT_OV != 0 { desc.push("VOUT_OV fault"); }
        if status & status::IOUT_OC != 0 { desc.push("IOUT_OC fault"); }
        if status & status::VIN_UV != 0 { desc.push("VIN_UV fault"); }
        if status & status::TEMP != 0 { desc.push("TEMP fault/warning"); }
        if status & status::CML != 0 { desc.push("CML fault"); }
        if status & status::NONE != 0 && desc.is_empty() { desc.push("NONE_OF_THE_ABOVE"); }
        desc
    }
    
    fn decode_status_vout(&self, status: u8) -> Vec<&'static str> {
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
    
    fn decode_status_iout(&self, status: u8) -> Vec<&'static str> {
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
    
    fn decode_status_input(&self, status: u8) -> Vec<&'static str> {
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
    
    fn decode_status_temp(&self, status: u8) -> Vec<&'static str> {
        let mut desc = Vec::new();
        if status & status_temp::OT_FAULT != 0 { desc.push("overtemp fault"); }
        if status & status_temp::OT_WARN != 0 { desc.push("overtemp warning"); }
        if status & status_temp::UT_WARN != 0 { desc.push("undertemp warning"); }
        if status & status_temp::UT_FAULT != 0 { desc.push("undertemp fault"); }
        desc
    }
    
    fn decode_status_cml(&self, status: u8) -> Vec<&'static str> {
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
    
    fn decode_fault_response(&self, response: u8) -> String {
        // Fault response format (Section 10.2 in datasheet):
        // Bits 7-5: Response type
        // Bits 4-3: Number of retries
        // Bits 2-0: Retry delay time
        
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
                    format!("{}", response_desc)
                } else {
                    format!("{}, {}, {} delay", response_desc, retries_desc, delay_desc)
                }
            }
        }
    }

    // Helper methods for I2C operations

    async fn read_byte(&mut self, command: u8) -> Result<u8> {
        let mut data = [0u8; 1];
        self.i2c
            .write_read(TPS546_I2C_ADDR, &[command], &mut data)
            .await?;
        Ok(data[0])
    }

    async fn write_byte(&mut self, command: u8, data: u8) -> Result<()> {
        self.i2c
            .write(TPS546_I2C_ADDR, &[command, data])
            .await?;
        Ok(())
    }

    async fn read_word(&mut self, command: u8) -> Result<u16> {
        let mut data = [0u8; 2];
        self.i2c
            .write_read(TPS546_I2C_ADDR, &[command], &mut data)
            .await?;
        Ok(u16::from_le_bytes(data))
    }

    async fn write_word(&mut self, command: u8, data: u16) -> Result<()> {
        let bytes = data.to_le_bytes();
        self.i2c
            .write(TPS546_I2C_ADDR, &[command, bytes[0], bytes[1]])
            .await?;
        Ok(())
    }

    async fn read_block(&mut self, command: u8, length: usize) -> Result<Vec<u8>> {
        // PMBus block read: first byte is length, then data
        let mut buffer = vec![0u8; length + 1];
        self.i2c
            .write_read(TPS546_I2C_ADDR, &[command], &mut buffer)
            .await?;
        
        // First byte is the length, verify it matches what we expect
        let reported_length = buffer[0] as usize;
        if reported_length != length {
            warn!("Block read length mismatch: expected {}, got {}", length, reported_length);
        }
        
        // Return just the data portion (skip length byte)
        Ok(buffer[1..=length].to_vec())
    }

    // SLINEAR11 format converters (Section 8.5.4 in datasheet)
    // Format: [5-bit two's complement exponent][11-bit two's complement mantissa]
    // Value = mantissa × 2^exponent

    fn slinear11_to_float(&self, value: u16) -> f32 {
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

    fn slinear11_to_int(&self, value: u16) -> i32 {
        self.slinear11_to_float(value) as i32
    }

    fn float_to_slinear11(&self, value: f32) -> u16 {
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
            if mantissa_f >= -1024.0 && mantissa_f < 1024.0 {
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

    fn int_to_slinear11(&self, value: i32) -> u16 {
        self.float_to_slinear11(value as f32)
    }

    // ULINEAR16 format converters (Section 8.5.3 in datasheet)
    // Format: 16-bit unsigned mantissa, exponent from VOUT_MODE
    // Value = mantissa × 2^exponent

    async fn ulinear16_to_float(&mut self, value: u16) -> Result<f32> {
        let vout_mode = self.read_byte(pmbus::VOUT_MODE).await?;
        
        // Extract 5-bit two's complement exponent from VOUT_MODE
        let exp_raw = (vout_mode & 0x1F) as i8;
        let exponent = if exp_raw & 0x10 != 0 {
            // Sign extend for negative exponent
            (exp_raw as u8 | 0xE0) as i8 as i32
        } else {
            exp_raw as i32
        };

        Ok(value as f32 * 2.0_f32.powi(exponent))
    }

    async fn float_to_ulinear16(&mut self, value: f32) -> Result<u16> {
        let vout_mode = self.read_byte(pmbus::VOUT_MODE).await?;
        
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
            error!("Value {} with exponent {} exceeds ULINEAR16 range", value, exponent);
            return Ok(0xFFFF);
        }
        
        Ok(mantissa as u16)
    }
}