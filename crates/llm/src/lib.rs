use std::sync::Arc;

use anyhow::Result;
use axum::{
    Router,
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use config::LlmConfig;
use messages::ChatCompletionRequest;

mod messages;
mod server;
use server::LlmServer;

/// Creates an axum router for LLM endpoints.
pub async fn router(config: LlmConfig) -> Result<Router> {
    let server = Arc::new(LlmServer::new(config.clone()).await?);

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
) -> Result<impl IntoResponse, StatusCode> {
    server
        .completions(request)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Handle list models requests.
async fn list_models(State(server): State<Arc<LlmServer>>) -> Result<impl IntoResponse, StatusCode> {
    server
        .list_models()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
