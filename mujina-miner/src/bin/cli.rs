//! Command-line interface for mujina-miner.
//!
//! This binary provides a CLI for controlling and monitoring the miner
//! daemon via the HTTP API.

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use std::io::{self, Read};

/// Echo request payload.
#[derive(Debug, Serialize)]
struct EchoRequest {
    message: String,
}

/// Echo response payload.
#[derive(Debug, Deserialize)]
struct EchoResponse {
    message: String,
}

/// Default API base URL.
/// Port 7785 represents ASCII 'M' (77) and 'U' (85).
const DEFAULT_API_URL: &str = "http://127.0.0.1:7785";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: mujina-cli <command> [args...]");
        eprintln!();
        eprintln!("Commands:");
        eprintln!("  echo [message]    Echo a message (reads from stdin if no args)");
        std::process::exit(1);
    }

    let command = &args[1];

    match command.as_str() {
        "echo" => cmd_echo(&args[2..]).await?,
        _ => {
            eprintln!("Unknown command: {}", command);
            eprintln!("Run without arguments to see usage.");
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Execute the echo command.
async fn cmd_echo(args: &[String]) -> Result<()> {
    let message = if args.is_empty() {
        // Read from stdin
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .context("Failed to read from stdin")?;
        buffer.trim().to_string()
    } else {
        // Join args with spaces
        args.join(" ")
    };

    let api_url = env::var("MUJINA_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_string());
    let url = format!("{}/api/v1/echo", api_url);

    let client = Client::new();
    let request = EchoRequest { message };

    let response = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .context("Failed to send request to API")?;

    if !response.status().is_success() {
        anyhow::bail!("API request failed: {}", response.status());
    }

    let echo_response: EchoResponse = response.json().await.context("Failed to parse response")?;

    println!("{}", echo_response.message);

    Ok(())
}
