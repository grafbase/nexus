use std::{convert::Infallible, sync::Arc};

use axum::{
    Router,
    extract::{Extension, Json, State},
    http::HeaderMap,
    response::{IntoResponse, Sse, sse::Event},
    routing::{get, post},
};
use futures::StreamExt;
use messages::ChatCompletionRequest;

mod error;
mod messages;
pub mod provider;
mod request;
mod server;
pub mod token_counter;

pub use error::LlmError;
use server::{LlmHandler, LlmServerBuilder};

pub type Result<T> = std::result::Result<T, LlmError>;

/// Creates an axum router for LLM endpoints.
pub async fn router(config: &config::Config) -> anyhow::Result<Router> {
    let server = Arc::new(
        LlmServerBuilder::new(config)
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize LLM server: {e}"))?,
    );

    let ai_routes = Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/models", get(list_models))
        .with_state(server);

    Ok(Router::new().nest(&config.llm.path, ai_routes))
}

/// Handle chat completion requests.
///
/// This endpoint supports both streaming and non-streaming responses.
/// When `stream: true` is set in the request, the response is sent as
/// Server-Sent Events (SSE). Otherwise, a standard JSON response is returned.
async fn chat_completions(
    State(server): State<Arc<LlmHandler>>,
    headers: HeaderMap,
    client_identity: Option<Extension<config::ClientIdentity>>,
    Json(request): Json<ChatCompletionRequest>,
) -> Result<impl IntoResponse> {
    log::info!("LLM chat completions handler called for model: {}", request.model);
    log::debug!("Request has {} messages", request.messages.len());
    log::debug!("Streaming: {}", request.stream.unwrap_or(false));

    // Extract request context including client identity
    let context = request::extract_context(&headers, client_identity.as_ref().map(|ext| &ext.0));

    if let Some(ref client_id) = context.client_id {
        log::debug!(
            "Client identity extracted: client_id={}, group={:?}",
            client_id,
            context.group
        );
    } else {
        log::debug!("No client identity found in request extensions");
    }

    // Check token rate limits
    if let Some(wait_duration) = server.check_token_rate_limit(&request, &context).await {
        // Duration::MAX is used as a sentinel value to indicate the request can never succeed
        // (requires more tokens than the rate limit allows)
        if wait_duration == std::time::Duration::MAX {
            log::debug!("Request requires more tokens than rate limit allows - cannot be fulfilled");

            return Err(LlmError::RateLimitExceeded {
                message: "Token rate limit exceeded. Request requires more tokens than the configured limit allows and cannot be fulfilled.".to_string(),
            });
        } else {
            log::debug!("Request rate limited, need to wait {:?}", wait_duration);

            return Err(LlmError::RateLimitExceeded {
                message: "Token rate limit exceeded. Please try again later.".to_string(),
            });
        }
    } else {
        log::debug!(
            "Token rate limit check passed for client_id={:?}, group={:?}",
            context.client_id,
            context.group
        );
    }

    // Check if streaming is requested
    if request.stream.unwrap_or(false) {
        let stream = server.completions_stream(request, &context).await?;

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
        let response = server.completions(request, &context).await?;

        log::debug!(
            "Chat completion successful, returning response with {} choices",
            response.choices.len()
        );

        Ok(Json(response).into_response())
    }
}

/// Handle list models requests.
async fn list_models(State(server): State<Arc<LlmHandler>>) -> Result<impl IntoResponse> {
    let response = server.models();

    log::debug!("Returning {} models", response.data.len());
    Ok(Json(response))
}
