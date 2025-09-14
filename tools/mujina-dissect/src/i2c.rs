//! I2C transaction assembly.

use crate::capture::{I2cEvent, I2cEventType};
use std::collections::VecDeque;

/// I2C transaction
#[derive(Debug, Clone)]
pub struct I2cTransaction {
    pub start_time: f64,
    pub end_time: f64,
    pub address: u8,
    pub is_read: bool,
    pub data: Vec<u8>,
    pub success: bool,
}

/// I2C transaction assembly state
#[derive(Debug, Clone)]
enum I2cState {
    /// Waiting for START condition
    Idle,
    /// Got START, waiting for address
    WaitingForAddress { start_time: f64 },
    /// Got address, collecting data
    CollectingData {
        start_time: f64,
        address: u8,
        is_read: bool,
        data: Vec<u8>,
        all_acks: bool,
    },
}

/// I2C transaction assembler
pub struct I2cAssembler {
    state: I2cState,
    transactions: VecDeque<I2cTransaction>,
}

impl I2cAssembler {
    pub fn new() -> Self {
        Self {
            state: I2cState::Idle,
            transactions: VecDeque::new(),
        }
    }

    /// Process an I2C event
    pub fn process(&mut self, event: &I2cEvent) {
        match &mut self.state {
            I2cState::Idle => {
                if event.event_type == I2cEventType::Start {
                    self.state = I2cState::WaitingForAddress {
                        start_time: event.timestamp,
                    };
                }
            }
            I2cState::WaitingForAddress { start_time } => match event.event_type {
                I2cEventType::Address => {
                    if let Some(addr) = event.address {
                        self.state = I2cState::CollectingData {
                            start_time: *start_time,
                            address: addr,
                            is_read: event.read,
                            data: Vec::new(),
                            all_acks: event.ack,
                        };
                    } else {
                        // Invalid address, go back to idle
                        self.state = I2cState::Idle;
                    }
                }
                I2cEventType::Stop => {
                    // Unexpected stop, go back to idle
                    self.state = I2cState::Idle;
                }
                _ => {}
            },
            I2cState::CollectingData {
                start_time,
                address,
                is_read,
                data,
                all_acks,
            } => match event.event_type {
                I2cEventType::Data => {
                    if let Some(byte) = event.data {
                        data.push(byte);
                        *all_acks = *all_acks && event.ack;
                    }
                }
                I2cEventType::Stop => {
                    // Transaction complete
                    self.transactions.push_back(I2cTransaction {
                        start_time: *start_time,
                        end_time: event.timestamp,
                        address: *address,
                        is_read: *is_read,
                        data: data.clone(),
                        success: *all_acks,
                    });
                    self.state = I2cState::Idle;
                }
                I2cEventType::Start => {
                    // Repeated start - save current transaction if it has data
                    if !data.is_empty() {
                        self.transactions.push_back(I2cTransaction {
                            start_time: *start_time,
                            end_time: event.timestamp,
                            address: *address,
                            is_read: *is_read,
                            data: data.clone(),
                            success: *all_acks,
                        });
                    }
                    // Start new transaction
                    self.state = I2cState::WaitingForAddress {
                        start_time: event.timestamp,
                    };
                }
                _ => {}
            },
        }
    }

    /// Get next completed transaction
    pub fn next_transaction(&mut self) -> Option<I2cTransaction> {
        self.transactions.pop_front()
    }

    /// Flush any pending transaction
    pub fn flush(&mut self) {
        // If we're in the middle of collecting data, treat it as incomplete
        if let I2cState::CollectingData {
            start_time,
            address,
            is_read,
            data,
            ..
        } = &self.state
        {
            if !data.is_empty() {
                self.transactions.push_back(I2cTransaction {
                    start_time: *start_time,
                    end_time: *start_time, // Use start time since we don't have end
                    address: *address,
                    is_read: *is_read,
                    data: data.clone(),
                    success: false, // Mark as unsuccessful since incomplete
                });
            }
        }
        self.state = I2cState::Idle;
    }
}

/// Group related I2C transactions (e.g., register write followed by read)
#[derive(Debug, Clone)]
pub struct I2cOperation {
    pub start_time: f64,
    pub end_time: f64,
    pub address: u8,
    pub register: Option<u8>,
    pub write_data: Option<Vec<u8>>,
    pub read_data: Option<Vec<u8>>,
}

/// Group I2C transactions into logical operations
pub fn group_transactions(transactions: &[I2cTransaction]) -> Vec<I2cOperation> {
    let mut operations = Vec::new();
    let mut i = 0;

    while i < transactions.len() {
        let t1 = &transactions[i];

        // Check if this is a register write followed by read pattern
        if !t1.is_read && t1.data.len() >= 1 && i + 1 < transactions.len() {
            let t2 = &transactions[i + 1];
            if t2.is_read && t2.address == t1.address {
                // Register read pattern: write register address, then read data
                operations.push(I2cOperation {
                    start_time: t1.start_time,
                    end_time: t2.end_time,
                    address: t1.address,
                    register: Some(t1.data[0]),
                    write_data: if t1.data.len() > 1 {
                        Some(t1.data[1..].to_vec())
                    } else {
                        None
                    },
                    read_data: Some(t2.data.clone()),
                });
                i += 2;
                continue;
            }
        }

        // Single transaction
        operations.push(I2cOperation {
            start_time: t1.start_time,
            end_time: t1.end_time,
            address: t1.address,
            register: if !t1.data.is_empty() {
                Some(t1.data[0])
            } else {
                None
            },
            write_data: if !t1.is_read && !t1.data.is_empty() {
                Some(t1.data.clone())
            } else {
                None
            },
            read_data: if t1.is_read {
                Some(t1.data.clone())
            } else {
                None
            },
        });
        i += 1;
    }

    operations
}
