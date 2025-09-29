use std::{convert::Infallible, sync::Arc};

use axum::{
    Router,
    extract::{Extension, Json, State},
    http::HeaderMap,
    response::{IntoResponse, Sse, sse::Event},
    routing::{get, post},
};
use axum_serde::Sonic;
use futures::StreamExt;
use messages::{anthropic, openai};

mod error;
mod messages;
pub mod provider;
mod request;
mod server;
pub mod token_counter;

pub use error::{AnthropicResult, LlmError, LlmResult as Result};
use server::{LlmHandler, LlmServerBuilder};

use crate::messages::unified;

/// Creates an axum router for LLM endpoints.
pub async fn router(config: &config::Config) -> anyhow::Result<Router> {
    let server = Arc::new(
        LlmServerBuilder::new(config)
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize LLM server: {e}"))?,
    );

    let mut router = Router::new();

    if config.llm.protocols.openai.enabled {
        let openai_routes = Router::new()
            .route("/v1/chat/completions", post(chat_completions))
            .route("/v1/models", get(list_models))
            .with_state(server.clone());

        router = router.nest(&config.llm.protocols.openai.path, openai_routes);
    }

    if config.llm.protocols.anthropic.enabled {
        let anthropic_routes = Router::new()
            .route("/v1/messages", post(anthropic_messages))
            .route("/v1/models", get(anthropic_list_models))
            .with_state(server.clone());

        router = router.nest(&config.llm.protocols.anthropic.path, anthropic_routes);
    }

    Ok(router)
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
    Sonic(request): Sonic<openai::ChatCompletionRequest>,
) -> Result<impl IntoResponse> {
    log::debug!("OpenAI chat completions handler called for model: {}", request.model);
    log::debug!("Request has {} messages", request.messages.len());
    log::debug!("Streaming: {}", request.stream.unwrap_or(false));

    // Extract request context including client identity
    let context = request::extract_context(&headers, client_identity.map(|ext| ext.0));

    // Check if streaming is requested
    if request.stream.unwrap_or(false) {
        // Convert OpenAI request to unified format
        let unified_request = unified::UnifiedRequest::from(request);
        let stream = server.completions_stream(unified_request, &context).await?;

        let event_stream = stream.map(move |result| {
            let event = match result {
                Ok(unified_chunk) => {
                    // Convert UnifiedChunk to OpenAI format for OpenAI protocol
                    let openai_chunk = openai::ChatCompletionChunk::from(unified_chunk);
                    let json = sonic_rs::to_string(&openai_chunk).unwrap_or_else(|e| {
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
        // Convert OpenAI request to unified format
        let unified_request = unified::UnifiedRequest::from(request);
        let unified_response = server.completions(unified_request, &context).await?;

        // Convert back to OpenAI format
        let response = openai::ChatCompletionResponse::from(unified_response);

        log::debug!(
            "Chat completion successful, returning response with {} choices",
            response.choices.len()
        );

        Ok(Json(response).into_response())
    }
}

/// Handle list models requests.
async fn list_models(State(server): State<Arc<LlmHandler>>) -> Result<impl IntoResponse> {
    let response = server.models().await;

    log::debug!("Returning {} models", response.data.len());
    Ok(Json(response))
}

/// Handle Anthropic messages requests.
///
/// This endpoint supports both streaming and non-streaming responses.
/// When `stream: true` is set in the request, the response is sent as
/// Server-Sent Events (SSE). Otherwise, a standard JSON response is returned.
async fn anthropic_messages(
    State(server): State<Arc<LlmHandler>>,
    headers: HeaderMap,
    client_identity: Option<Extension<config::ClientIdentity>>,
    Sonic(request): Sonic<anthropic::AnthropicChatRequest>,
) -> AnthropicResult<impl IntoResponse> {
    log::debug!("Anthropic messages handler called for model: {}", request.model);
    log::debug!("Request has {} messages", request.messages.len());
    log::debug!("Streaming: {}", request.stream.unwrap_or(false));

    // Extract request context including client identity
    let context = request::extract_context(&headers, client_identity.map(|ext| ext.0));

    // Convert Anthropic request to unified format
    let unified_request = unified::UnifiedRequest::from(request);

    // Check if streaming is requested
    if unified_request.stream.unwrap_or(false) {
        let stream = server.completions_stream(unified_request, &context).await?;

        let event_stream = stream.map(move |result| {
            let event = match result {
                Ok(chunk) => {
                    // Convert unified chunk to Anthropic streaming event format
                    let anthropic_event = anthropic::AnthropicStreamEvent::from(chunk);
                    let json = sonic_rs::to_string(&anthropic_event).unwrap_or_else(|e| {
                        log::error!("Failed to serialize Anthropic streaming event: {e}");
                        r#"{"error":"serialization failed"}"#.to_string()
                    });

                    Event::default().data(json)
                }
                Err(e) => {
                    log::error!("Stream error: {e}");
                    let anthropic_error = anthropic::AnthropicError::from(e);
                    let error_event = anthropic::AnthropicStreamEvent::Error {
                        error: anthropic_error.error,
                    };
                    let json = sonic_rs::to_string(&error_event).unwrap_or_else(|se| {
                        log::error!("Failed to serialize Anthropic stream error event: {se}");
                        r#"{"type":"error","error":{"type":"internal_error","message":"serialization failed"}}"#
                            .to_string()
                    });

                    Event::default().data(json)
                }
            };

            Ok::<_, Infallible>(event)
        });

        // Anthropic doesn't use [DONE] marker, just end the stream
        log::debug!("Returning Anthropic streaming response");

        Ok(Sse::new(event_stream).into_response())
    } else {
        // Non-streaming response - use unified types directly!
        let unified_response = server.completions(unified_request, &context).await?;
        let anthropic_response = anthropic::AnthropicChatResponse::from(unified_response);

        log::debug!("Anthropic messages completion successful");

        Ok(Json(anthropic_response).into_response())
    }
}

/// Handle Anthropic list models requests.
async fn anthropic_list_models(State(server): State<Arc<LlmHandler>>) -> AnthropicResult<impl IntoResponse> {
    let openai_response = server.models().await;

    // Convert OpenAI models response to Anthropic format
    let anthropic_response = anthropic::AnthropicModelsResponse::from(openai_response);

    log::debug!("Returning {} models for Anthropic", anthropic_response.data.len());
    Ok(Json(anthropic_response))
}
