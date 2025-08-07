use std::sync::Arc;

use axum::{
    Router,
    extract::{Json, State},
    response::IntoResponse,
    routing::{get, post},
};
use config::LlmConfig;
use messages::ChatCompletionRequest;

mod error;
mod messages;
mod provider;
mod server;
mod sse;

use error::LlmError;
use server::LlmServer;

pub(crate) type Result<T> = std::result::Result<T, LlmError>;

/// Creates an axum router for LLM endpoints.
pub async fn router(config: LlmConfig) -> anyhow::Result<Router> {
    let server = Arc::new(
        LlmServer::new(config.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize LLM server: {e}"))?,
    );

    let ai_routes = Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/models", get(list_models))
        .with_state(server);

    Ok(Router::new().nest(&config.path, ai_routes))
}

/// Handle chat completion requests.
async fn chat_completions(
    State(server): State<Arc<LlmServer>>,
    Json(request): Json<ChatCompletionRequest>,
) -> Result<impl IntoResponse> {
    log::debug!("Received chat completion request for model: {}", request.model);
    log::debug!("Request has {} messages", request.messages.len());

    let response = server.completions(request).await?;

    log::debug!(
        "Chat completion successful, returning response with {} choices",
        response.choices.len()
    );

    Ok(Json(response))
}

/// Handle list models requests.
async fn list_models(State(server): State<Arc<LlmServer>>) -> Result<impl IntoResponse> {
    let response = server.list_models().await?;
    log::debug!("Returning {} models", response.data.len());
    Ok(Json(response))
}
