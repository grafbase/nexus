use std::{convert::Infallible, sync::Arc};

use axum::{
    Router,
    extract::{Extension, Json, State},
    http::HeaderMap,
    response::{IntoResponse, Sse, sse::Event},
    routing::{get, post},
};
use futures::StreamExt;
use messages::openai;

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
            // TODO: Implement anthropic_messages handler in Phase 2
            // .route("/v1/messages", post(anthropic_messages))
            // .route("/v1/models", get(anthropic_list_models))
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
    span_context: Option<Extension<fastrace::collector::SpanContext>>,
    Json(request): Json<openai::ChatCompletionRequest>,
) -> Result<impl IntoResponse> {
    log::info!("LLM chat completions handler called for model: {}", request.model);
    log::debug!("Request has {} messages", request.messages.len());
    log::debug!("Streaming: {}", request.stream.unwrap_or(false));

    // Extract request context including clien it identity and span context
    let context = request::extract_context(
        &headers,
        client_identity.map(|ext| ext.0),
        span_context.as_ref().map(|ext| ext.0),
    );

    if let Some(ref client_identity) = context.client_identity {
        log::debug!(
            "Client identity extracted: client_id={}, group={:?}",
            client_identity.client_id,
            client_identity.group
        );
    } else {
        log::debug!("No client identity found in request extensions");
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
