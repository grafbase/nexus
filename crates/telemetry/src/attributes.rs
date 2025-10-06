//! GenAI telemetry attributes following OpenTelemetry semantic conventions.
//! Derived from the OpenTelemetry GenAI semantic conventions specification.
//! Each constant maps to the attribute key emitted by Nexus telemetry.
//! https://opentelemetry.io/docs/specs/semconv/registry/attributes/gen-ai/ (2025-10-07)

/// Free-form description of the GenAI agent provided by the application.
pub const GEN_AI_AGENT_DESCRIPTION: &str = "gen_ai.agent.description";

/// Unique identifier of the GenAI agent.
pub const GEN_AI_AGENT_ID: &str = "gen_ai.agent.id";

/// Human-readable name of the GenAI agent provided by the application.
pub const GEN_AI_AGENT_NAME: &str = "gen_ai.agent.name";

/// Unique identifier for a conversation (session or thread) used to correlate messages.
pub const GEN_AI_CONVERSATION_ID: &str = "gen_ai.conversation.id";

/// Identifier of the data source backing the agent or RAG workflow; should match the system identifier.
/// Additional db.* attributes may be used alongside this attribute when they apply.
pub const GEN_AI_DATA_SOURCE_ID: &str = "gen_ai.data_source.id";

/// Chat history provided to the model as input.
/// Instrumentations must follow the GenAI Input messages JSON schema and preserve message order.
/// Prefer structured recording; if spans cannot store structured data, a JSON string form is permitted.
/// This attribute often contains sensitive information, including potential PII.
pub const GEN_AI_INPUT_MESSAGES: &str = "gen_ai.input.messages";

/// Name of the GenAI operation being performed.
/// Known values: `chat`, `create_agent`, `embeddings`, `execute_tool`, `generate_content`, `invoke_agent`, `text_completion`.
pub const GEN_AI_OPERATION_NAME: &str = "gen_ai.operation.name";

/// Messages returned by the model, where each entry represents a single response choice or candidate.
/// Instrumentations must follow the GenAI Output messages JSON schema and record messages in structured form when possible.
/// This attribute may contain sensitive information and should be handled accordingly.
pub const GEN_AI_OUTPUT_MESSAGES: &str = "gen_ai.output.messages";

/// Requested output modality for the response content.
/// Known values: `image`, `json`, `speech`, `text`.
pub const GEN_AI_OUTPUT_TYPE: &str = "gen_ai.output.type";

/// Provider name identified by the instrumentation; acts as a discriminator for provider-specific telemetry.
/// Known values include: `anthropic`, `aws.bedrock`, `azure.ai.inference`, `azure.ai.openai`, `cohere`, `deepseek`, `gcp.gemini`,
/// `gcp.gen_ai`, `gcp.vertex_ai`, `groq`, `ibm.watsonx.ai`, `mistral_ai`, `openai`, `perplexity`, `x_ai`.
pub const GEN_AI_PROVIDER_NAME: &str = "gen_ai.provider.name";

/// Target number of candidate completions requested from the model.
pub const GEN_AI_REQUEST_CHOICE_COUNT: &str = "gen_ai.request.choice.count";

/// Encoding formats requested for an embeddings operation.
/// Some systems refer to this as embedding types; certain APIs allow only a single format per request.
pub const GEN_AI_REQUEST_ENCODING_FORMATS: &str = "gen_ai.request.encoding_formats";

/// Frequency penalty setting applied to the request.
pub const GEN_AI_REQUEST_FREQUENCY_PENALTY: &str = "gen_ai.request.frequency_penalty";

/// Maximum number of tokens the model should generate for the request.
pub const GEN_AI_REQUEST_MAX_TOKENS: &str = "gen_ai.request.max_tokens";

/// Name of the model that the request targets.
pub const GEN_AI_REQUEST_MODEL: &str = "gen_ai.request.model";

/// Presence penalty setting applied to the request.
pub const GEN_AI_REQUEST_PRESENCE_PENALTY: &str = "gen_ai.request.presence_penalty";

/// Seed value that increases determinism across identical requests.
pub const GEN_AI_REQUEST_SEED: &str = "gen_ai.request.seed";

/// Sequences that cause the model to stop generating additional tokens.
pub const GEN_AI_REQUEST_STOP_SEQUENCES: &str = "gen_ai.request.stop_sequences";

/// Temperature sampling setting applied to the request.
pub const GEN_AI_REQUEST_TEMPERATURE: &str = "gen_ai.request.temperature";

/// Top-k sampling setting applied to the request.
pub const GEN_AI_REQUEST_TOP_K: &str = "gen_ai.request.top_k";

/// Top-p (nucleus) sampling setting applied to the request.
pub const GEN_AI_REQUEST_TOP_P: &str = "gen_ai.request.top_p";

/// Array of reasons describing why the model stopped generating tokens; aligns with the returned choices.
pub const GEN_AI_RESPONSE_FINISH_REASONS: &str = "gen_ai.response.finish_reasons";

/// Unique identifier for the completion returned by the provider.
pub const GEN_AI_RESPONSE_ID: &str = "gen_ai.response.id";

/// Name of the model that generated the response.
pub const GEN_AI_RESPONSE_MODEL: &str = "gen_ai.response.model";

/// System instructions provided separately from the chat history.
/// Instrumentations must follow the GenAI System instructions JSON schema and prefer structured recording.
/// This attribute may contain sensitive information and should be handled with care.
pub const GEN_AI_SYSTEM_INSTRUCTIONS: &str = "gen_ai.system_instructions";

/// Type of token being counted by a usage metric.
/// Known values: `input`, `output`.
pub const GEN_AI_TOKEN_TYPE: &str = "gen_ai.token.type";

/// Identifier of a tool call issued by the agent or model.
pub const GEN_AI_TOOL_CALL_ID: &str = "gen_ai.tool.call.id";

/// Human-readable description of the tool invoked by the agent.
pub const GEN_AI_TOOL_DESCRIPTION: &str = "gen_ai.tool.description";

/// Name of the tool utilized by the agent.
pub const GEN_AI_TOOL_NAME: &str = "gen_ai.tool.name";

/// Type of tool utilized by the agent.
/// Known values: `function`, `extension`, `datastore`.
pub const GEN_AI_TOOL_TYPE: &str = "gen_ai.tool.type";

/// Number of tokens consumed in the GenAI input (prompt).
pub const GEN_AI_USAGE_INPUT_TOKENS: &str = "gen_ai.usage.input_tokens";

/// Number of tokens produced in the GenAI output (completion).
pub const GEN_AI_USAGE_OUTPUT_TOKENS: &str = "gen_ai.usage.output_tokens";
