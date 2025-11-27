# Building Mujina Miner for eHash Development

This directory contains the Mujina miner as a git submodule, used in the JDC BOLT12 development environment as an optional mining device for testing the complete mining-to-payment flow.

## Overview

Mujina is an open-source Bitcoin mining software written in Rust for ASIC mining hardware. In the eHash development environment, it serves as a **test mining device** that can:

1. Connect to the TProxy (Translator Proxy) via Stratum V1
2. Receive mining work from the JDC through TProxy
3. Submit shares that trigger eHash token minting
4. Test the complete flow: Mining → Share Submission → Token Minting → BOLT12 Payout

## Role in BOLT12 Testing

Mujina is **optional** in the JDC BOLT12 development environment. It's used when you want to:

- Test with real mining hardware (Bitaxe Gamma or similar)
- Generate actual shares to trigger eHash minting
- Verify the complete end-to-end flow from mining to payment
- Test share validation and difficulty thresholds

Without mujina, you can still test BOLT12 payments by:
- Manually triggering keyset transitions
- Using the `--with-miner` flag with ehashimint's built-in test miner
- Directly calling the mint API

## Prerequisites

### System Dependencies

#### Ubuntu/Debian
```bash
sudo apt-get update
sudo apt-get install libudev-dev build-essential
```

The `libudev-dev` package is required for USB device discovery and communication with mining hardware.

#### macOS
macOS support is planned but not yet implemented for USB discovery.

### Rust Toolchain

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Ensure you have the latest stable Rust
rustup update stable
```

### Hardware (Optional)

Mujina supports:
- **Bitaxe Gamma** with BM1370 ASIC (currently supported)
- **EmberOne** boards (planned)
- Other ASIC mining hardware (planned)

You can also run mujina without hardware using its dummy job source for testing.

## Build Instructions

### 1. Build the Binary

From the `deps/mujina` directory:

```bash
cargo build --release
```

This will create the binary at `target/release/mujina-miner`.

### 2. Verify the Build

Check that the binary was created:

```bash
ls -lh target/release/mujina-miner
```

### 3. Test Without Hardware (Optional)

You can test the build without mining hardware:

```bash
# Run with dummy job source (no pool connection)
cargo run

# Or with the release binary
./target/release/mujina-miner
```

## Usage in eHash Development Environment

### With ehashimint bolt12-dev

When running `ehashimint bolt12-dev --with-miner`, the built-in test miner is used by default. To use mujina instead:

1. Build mujina (see above)
2. Start the bolt12-dev environment:
   ```bash
   just jdc-bolt12-start
   ```
3. In a separate terminal, run mujina pointing to TProxy:
   ```bash
   cd deps/mujina
   MUJINA_POOL_URL="stratum+tcp://localhost:34255" \
   MUJINA_POOL_USER="dev_miner.mujina" \
   MUJINA_POOL_PASS="x" \
   cargo run --release
   ```

### Configuration

When connecting to the eHash development environment:

- **Pool URL**: `stratum+tcp://localhost:34255` (TProxy Stratum V1 port)
- **Pool User**: `dev_miner.mujina` (or any identifier)
- **Pool Password**: `x` (default, not used for authentication)

### Log Levels

Control mujina's output verbosity:

```bash
# Info level (default) - shows pool connection, shares, errors
cargo run --release

# Debug level - adds job distribution, hardware state changes
RUST_LOG=mujina_miner=debug cargo run --release

# Trace level - shows all protocol traffic
RUST_LOG=mujina_miner=trace cargo run --release
```

## Testing Flow

When mujina is connected to the eHash development environment:

```
1. Mujina connects to TProxy (Stratum V1, port 34255)
2. TProxy translates to Stratum V2 and forwards to JDC
3. JDC provides mining work from Template Provider
4. Mujina submits shares back through TProxy
5. JDC validates shares and mints eHash tokens if difficulty threshold is met
6. Tokens are associated with the miner's locking pubkey
7. Developer can trigger BOLT12 payment to test payout flow
```

## Architecture in Development Environment

```
┌──────────────────┐
│  Mujina Miner    │
│  (Optional)      │
└────────┬─────────┘
         │ Stratum V1
         ▼
┌──────────────────┐
│     TProxy       │
│   (Port 34255)   │
└────────┬─────────┘
         │ Stratum V2
         ▼
┌──────────────────┐
│   JDC (Mint)     │
│  + LDK Node      │
└──────────────────┘
```

## Troubleshooting

### Build Fails with libudev Error

Ensure `libudev-dev` is installed:
```bash
sudo apt-get install libudev-dev
```

### Cannot Connect to TProxy

Ensure the JDC BOLT12 development environment is running:
```bash
just jdc-bolt12-status
```

Check that TProxy is listening on port 34255:
```bash
netstat -tuln | grep 34255
```

### No Shares Being Submitted

This is normal if:
- You're running without hardware (dummy job source doesn't submit shares)
- The mining difficulty is too high for your hardware
- The hardware is still warming up

Check mujina logs for connection status and share submission attempts.

### Shares Not Triggering eHash Minting

Check the JDC configuration for `min_leading_zeros`. For testing, this should be set low (e.g., 10) to ensure shares meet the minting threshold.

Check JDC logs to see if shares are being received and validated:
```bash
just jdc-bolt12-logs-jdc
```

## Alternative: Built-in Test Miner

If you don't have mining hardware or don't want to build mujina, use ehashimint's built-in test miner:

```bash
just jdc-bolt12-start --with-miner
```

This will start a software miner that generates shares for testing purposes.

## References

- [Mujina Repository](https://github.com/PioneerHash/mujina)
- [Mujina Architecture Documentation](docs/architecture.md)
- [Bitaxe Hardware](https://github.com/bitaxeorg)
- [Stratum V1 Protocol](https://en.bitcoin.it/wiki/Stratum_mining_protocol)
- [eHash Development Guide](../../DEVELOPMENT.md)

## Version Information

- **Branch**: main/master (default)
- **Language**: Rust
- **Hardware Support**: Bitaxe Gamma (BM1370 ASIC)
- **Protocol**: Stratum V1
- **Role**: Optional test mining device for JDC BOLT12 development
- **Used by**: Manual testing of complete mining-to-payment flow
- **Alternative**: ehashimint built-in test miner (`--with-miner` flag)

## Development Status

> **Developer Preview**: Mujina is under heavy development and not ready for production use. It's included in the eHash development environment for testing purposes only.

For production mining operations, use established mining software. Mujina is provided as a development tool for testing the eHash ecosystem.
