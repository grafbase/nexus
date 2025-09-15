//! Input conversion modules for different protocols.
//!
//! Currently only OpenAI protocol is supported.

pub(crate) mod openai;

// Re-export OpenAI conversions for backward compatibility
pub(super) use openai::AnthropicRequest;
