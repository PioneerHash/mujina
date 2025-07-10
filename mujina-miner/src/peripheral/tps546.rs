//! TPS546D24A Power Management Controller Driver
//!
//! This module provides a driver for the Texas Instruments TPS546D24A
//! synchronous buck converter with PMBus interface.

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
    pub const STACK_CONFIG: u8 = 0xEC;
    pub const PIN_DETECT_OVERRIDE: u8 = 0xEE;
}

/// OPERATION command values
const OPERATION_OFF: u8 = 0x00;
const OPERATION_ON: u8 = 0x80;

/// ON_OFF_CONFIG bits
const ON_OFF_CONFIG_PU: u8 = 0x10;
const ON_OFF_CONFIG_CMD: u8 = 0x08;
const ON_OFF_CONFIG_CP: u8 = 0x04;
const ON_OFF_CONFIG_POLARITY: u8 = 0x02;
const ON_OFF_CONFIG_DELAY: u8 = 0x01;

/// STATUS_WORD bits
mod status {
    pub const VOUT: u16 = 0x8000;
    pub const IOUT: u16 = 0x4000;
    pub const INPUT: u16 = 0x2000;
    pub const MFR: u16 = 0x1000;
    pub const PGOOD: u16 = 0x0800;
    pub const OTHER: u16 = 0x0200;
    pub const BUSY: u16 = 0x0080;
    pub const OFF: u16 = 0x0040;
    pub const VOUT_OV: u16 = 0x0020;
    pub const IOUT_OC: u16 = 0x0010;
    pub const VIN_UV: u16 = 0x0008;
    pub const TEMP: u16 = 0x0004;
    pub const CML: u16 = 0x0002;
    pub const NONE: u16 = 0x0001;
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
            vout_command: 1.2,
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

        // First just try to read the status to see if device is responsive
        match self.read_word(pmbus::STATUS_WORD).await {
            Ok(status) => info!("TPS546 STATUS_WORD: 0x{:04X}", status),
            Err(e) => {
                error!("Failed to read TPS546 status: {}", e);
                return Err(e);
            }
        }

        // Turn off output during configuration
        self.write_byte(pmbus::OPERATION, OPERATION_OFF).await?;
        debug!("Power output turned off");

        // Clear any existing faults before configuring
        self.clear_faults().await?;

        // Configure ON_OFF_CONFIG
        let on_off_config = ON_OFF_CONFIG_DELAY
            | ON_OFF_CONFIG_POLARITY
            | ON_OFF_CONFIG_CP
            | ON_OFF_CONFIG_CMD
            | ON_OFF_CONFIG_PU;
        self.write_byte(pmbus::ON_OFF_CONFIG, on_off_config).await?;
        debug!("ON_OFF_CONFIG set to 0x{:02X}", on_off_config);

        // Read VOUT_MODE to verify data format
        let vout_mode = self.read_byte(pmbus::VOUT_MODE).await?;
        debug!("VOUT_MODE: 0x{:02X}", vout_mode);
        
        // For now, skip full configuration and just set the output voltage
        // self.write_config().await?;
        
        info!("TPS546D24A initialization complete (minimal config)");
        Ok(())
    }

    /// Write all configuration parameters
    async fn write_config(&mut self) -> Result<()> {
        info!("Writing TPS546 configuration");

        // Phase configuration
        self.write_byte(pmbus::PHASE, 0x00).await?;

        // Switching frequency (650 kHz)
        self.write_word(pmbus::FREQUENCY_SWITCH, self.int_to_slinear11(650))
            .await?;

        // Input voltage thresholds
        if self.config.vin_uv_warn_limit > 0.0 {
            self.write_word(
                pmbus::VIN_UV_WARN_LIMIT,
                self.float_to_slinear11(self.config.vin_uv_warn_limit),
            )
            .await?;
        }

        self.write_word(pmbus::VIN_ON, self.float_to_slinear11(self.config.vin_on))
            .await?;
        self.write_word(
            pmbus::VIN_OFF,
            self.float_to_slinear11(self.config.vin_off),
        )
        .await?;
        self.write_word(
            pmbus::VIN_OV_FAULT_LIMIT,
            self.float_to_slinear11(self.config.vin_ov_fault_limit),
        )
        .await?;

        // VIN_OV_FAULT_RESPONSE
        const VIN_OV_FAULT_RESPONSE: u8 = 0xB7;
        self.write_byte(pmbus::VIN_OV_FAULT_RESPONSE, VIN_OV_FAULT_RESPONSE)
            .await?;

        // Output voltage configuration
        self.write_word(
            pmbus::VOUT_SCALE_LOOP,
            self.float_to_slinear11(self.config.vout_scale_loop),
        )
        .await?;
        
        let vout_command = self.float_to_ulinear16(self.config.vout_command).await?;
        self.write_word(pmbus::VOUT_COMMAND, vout_command).await?;
        
        let vout_max = self.float_to_ulinear16(self.config.vout_max).await?;
        self.write_word(pmbus::VOUT_MAX, vout_max).await?;
        
        let vout_min = self.float_to_ulinear16(self.config.vout_min).await?;
        self.write_word(pmbus::VOUT_MIN, vout_min).await?;

        // Output voltage protection
        const VOUT_OV_FAULT_LIMIT: f32 = 1.25; // 125% of VOUT_COMMAND
        const VOUT_OV_WARN_LIMIT: f32 = 1.16; // 116% of VOUT_COMMAND
        const VOUT_UV_WARN_LIMIT: f32 = 0.90; // 90% of VOUT_COMMAND
        const VOUT_UV_FAULT_LIMIT: f32 = 0.75; // 75% of VOUT_COMMAND

        let vout_ov_fault = self.float_to_ulinear16(VOUT_OV_FAULT_LIMIT).await?;
        self.write_word(pmbus::VOUT_OV_FAULT_LIMIT, vout_ov_fault).await?;
        
        let vout_ov_warn = self.float_to_ulinear16(VOUT_OV_WARN_LIMIT).await?;
        self.write_word(pmbus::VOUT_OV_WARN_LIMIT, vout_ov_warn).await?;
        
        let vout_uv_warn = self.float_to_ulinear16(VOUT_UV_WARN_LIMIT).await?;
        self.write_word(pmbus::VOUT_UV_WARN_LIMIT, vout_uv_warn).await?;
        
        let vout_uv_fault = self.float_to_ulinear16(VOUT_UV_FAULT_LIMIT).await?;
        self.write_word(pmbus::VOUT_UV_FAULT_LIMIT, vout_uv_fault).await?;

        // Output current protection
        self.write_word(
            pmbus::IOUT_OC_WARN_LIMIT,
            self.float_to_slinear11(self.config.iout_oc_warn_limit),
        )
        .await?;
        self.write_word(
            pmbus::IOUT_OC_FAULT_LIMIT,
            self.float_to_slinear11(self.config.iout_oc_fault_limit),
        )
        .await?;

        // IOUT_OC_FAULT_RESPONSE
        const IOUT_OC_FAULT_RESPONSE: u8 = 0xC0; // Shutdown immediately, no retries
        self.write_byte(pmbus::IOUT_OC_FAULT_RESPONSE, IOUT_OC_FAULT_RESPONSE)
            .await?;

        // Temperature protection
        const OT_WARN_LIMIT: i32 = 105; // °C
        const OT_FAULT_LIMIT: i32 = 145; // °C
        const OT_FAULT_RESPONSE: u8 = 0xFF; // Wait for cooling and retry

        self.write_word(pmbus::OT_WARN_LIMIT, self.int_to_slinear11(OT_WARN_LIMIT))
            .await?;
        self.write_word(pmbus::OT_FAULT_LIMIT, self.int_to_slinear11(OT_FAULT_LIMIT))
            .await?;
        self.write_byte(pmbus::OT_FAULT_RESPONSE, OT_FAULT_RESPONSE)
            .await?;

        // Timing configuration
        const TON_DELAY: i32 = 0;
        const TON_RISE: i32 = 3;
        const TON_MAX_FAULT_LIMIT: i32 = 0;
        const TON_MAX_FAULT_RESPONSE: u8 = 0x3B;
        const TOFF_DELAY: i32 = 0;
        const TOFF_FALL: i32 = 0;

        self.write_word(pmbus::TON_DELAY, self.int_to_slinear11(TON_DELAY))
            .await?;
        self.write_word(pmbus::TON_RISE, self.int_to_slinear11(TON_RISE))
            .await?;
        self.write_word(
            pmbus::TON_MAX_FAULT_LIMIT,
            self.int_to_slinear11(TON_MAX_FAULT_LIMIT),
        )
        .await?;
        self.write_byte(pmbus::TON_MAX_FAULT_RESPONSE, TON_MAX_FAULT_RESPONSE)
            .await?;
        self.write_word(pmbus::TOFF_DELAY, self.int_to_slinear11(TOFF_DELAY))
            .await?;
        self.write_word(pmbus::TOFF_FALL, self.int_to_slinear11(TOFF_FALL))
            .await?;

        // Pin detect override
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
            self.write_byte(pmbus::OPERATION, OPERATION_OFF).await?;
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
            self.write_byte(pmbus::OPERATION, OPERATION_ON).await?;

            // Verify operation
            let operation = self.read_byte(pmbus::OPERATION).await?;
            if operation != OPERATION_ON {
                error!("Failed to turn on output, OPERATION = 0x{:02X}", operation);
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
            warn!("VOUT status error: 0x{:02X}", vout_status);
        }

        if status & status::IOUT != 0 {
            let iout_status = self.read_byte(pmbus::STATUS_IOUT).await?;
            warn!("IOUT status error: 0x{:02X}", iout_status);
        }

        if status & status::INPUT != 0 {
            let input_status = self.read_byte(pmbus::STATUS_INPUT).await?;
            warn!("INPUT status error: 0x{:02X}", input_status);
        }

        if status & status::TEMP != 0 {
            let temp_status = self.read_byte(pmbus::STATUS_TEMPERATURE).await?;
            warn!("TEMPERATURE status error: 0x{:02X}", temp_status);
        }

        if status & status::CML != 0 {
            let cml_status = self.read_byte(pmbus::STATUS_CML).await?;
            warn!("CML status error: 0x{:02X}", cml_status);
        }

        Ok(())
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

    // SLINEAR11 format converters

    fn slinear11_to_float(&self, value: u16) -> f32 {
        let exponent = if value & 0x8000 != 0 {
            // Negative exponent (two's complement)
            -(((!value >> 11) & 0x001F) as i32 + 1)
        } else {
            (value >> 11) as i32
        };

        let mantissa = if value & 0x0400 != 0 {
            // Negative mantissa (two's complement)
            -(((!(value & 0x03FF)) & 0x03FF) as i32 + 1)
        } else {
            (value & 0x03FF) as i32
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

        // For negative exponents (small positive values)
        for i in 0..=15 {
            let mantissa = (value * 2.0_f32.powi(i)) as i32;
            if mantissa < 1024 {
                let exponent = i;
                // Encode negative exponent in two's complement
                let exp_bits = ((((!exponent) + 1) & 0x1F) as u16) << 11;
                return exp_bits | (mantissa as u16 & 0x03FF);
            }
        }

        error!("Could not encode {} as SLINEAR11", value);
        0
    }

    fn int_to_slinear11(&self, value: i32) -> u16 {
        if value == 0 {
            return 0;
        }

        // For positive integers
        for i in 0..=15 {
            let mantissa = value / 2_i32.pow(i as u32);
            if mantissa < 1024 {
                let exponent = i as u16;
                return ((exponent << 11) & 0xF800) | (mantissa as u16);
            }
        }

        error!("Could not encode {} as SLINEAR11", value);
        0
    }

    // ULINEAR16 format converters

    async fn ulinear16_to_float(&mut self, value: u16) -> Result<f32> {
        let vout_mode = self.read_byte(pmbus::VOUT_MODE).await?;
        
        let exponent = if vout_mode & 0x10 != 0 {
            // Negative exponent
            -(((!vout_mode) & 0x1F) as i32 + 1)
        } else {
            (vout_mode & 0x1F) as i32
        };

        Ok(value as f32 * 2.0_f32.powi(exponent))
    }

    async fn float_to_ulinear16(&mut self, value: f32) -> Result<u16> {
        let vout_mode = self.read_byte(pmbus::VOUT_MODE).await?;
        
        let exponent = if vout_mode & 0x10 != 0 {
            // Negative exponent
            -(((!vout_mode) & 0x1F) as i32 + 1)
        } else {
            (vout_mode & 0x1F) as i32
        };

        Ok((value / 2.0_f32.powi(exponent)) as u16)
    }
}