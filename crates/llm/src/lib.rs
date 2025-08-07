use std::{convert::Infallible, sync::Arc};

use axum::{
    Router,
    extract::{Json, State},
    response::{IntoResponse, Sse, sse::Event},
    routing::{get, post},
};
use config::LlmConfig;
use futures::StreamExt;
use messages::ChatCompletionRequest;

mod error;
mod messages;
mod provider;
mod server;

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
///
/// This endpoint supports both streaming and non-streaming responses.
/// When `stream: true` is set in the request, the response is sent as
/// Server-Sent Events (SSE). Otherwise, a standard JSON response is returned.
async fn chat_completions(
    State(server): State<Arc<LlmServer>>,
    Json(request): Json<ChatCompletionRequest>,
) -> Result<impl IntoResponse> {
    log::debug!("Received chat completion request for model: {}", request.model);
    log::debug!("Request has {} messages", request.messages.len());
    log::debug!("Streaming: {}", request.stream.unwrap_or(false));

    // Check if streaming is requested
    if request.stream.unwrap_or(false) {
        let stream = server.completions_stream(request).await?;

        let event_stream = stream.map(move |result| {
            let event = match result {
                Ok(chunk) => {
                    let json = sonic_rs::to_string(&chunk).unwrap_or_else(|e| {
                        log::error!("Failed to serialize chunk: {e}");
                        r#"{"error":"serialization failed"}"#.to_string()
                    });

                    Event::default().data(json)
                }
                Err(e) => {
                    log::error!("Stream error: {e}");
                    Event::default().data(format!(r#"{{"error":"{e}"}}"#))
                }
            };

            Ok::<_, Infallible>(event)
        });

        let with_done = event_stream.chain(futures::stream::once(async {
            Ok::<_, Infallible>(Event::default().data("[DONE]"))
        }));

        log::debug!("Returning streaming response");
        Ok(Sse::new(with_done).into_response())
    } else {
        // Non-streaming response
        let response = server.completions(request).await?;

        log::debug!(
            "Chat completion successful, returning response with {} choices",
            response.choices.len()
        );

        Ok(Json(response).into_response())
    }
}

/// Handle list models requests.
async fn list_models(State(server): State<Arc<LlmServer>>) -> Result<impl IntoResponse> {
    let response = server.list_models().await?;
    log::debug!("Returning {} models", response.data.len());
    Ok(Json(response))
}
