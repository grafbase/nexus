//! Message types for different LLM protocols.
//!
//! This module organizes message formats by protocol, with OpenAI format
//! serving as the current default and primary interchange format.

pub(crate) mod anthropic;
pub(crate) mod openai;
