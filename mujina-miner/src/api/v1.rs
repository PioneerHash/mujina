//! API version 1 endpoints.

use axum::{
    extract::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};

/// Echo request payload.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EchoRequest {
    /// The message to echo back.
    pub message: String,
}

/// Echo response payload.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EchoResponse {
    /// The echoed message.
    pub message: String,
}

/// Build the v1 API routes.
pub fn routes() -> Router {
    Router::new()
        .route("/echo", post(echo))
        .route("/health", get(health))
}

/// Echo endpoint handler.
///
/// Echoes back the provided message. Useful for testing API connectivity.
async fn echo(Json(req): Json<EchoRequest>) -> Json<EchoResponse> {
    Json(EchoResponse {
        message: req.message,
    })
}

/// Health check endpoint handler.
///
/// Returns a simple OK status to verify the API is running.
async fn health() -> &'static str {
    "OK"
}
