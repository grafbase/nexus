//! Model family detection and routing for AWS Bedrock models.
//!
//! AWS Bedrock hosts models from multiple vendors, each with different request/response
//! formats. This module provides utilities for detecting the model family from model IDs
//! and routing requests to the appropriate transformation logic.

use anyhow::{Result, anyhow};

/// Represents the different model families available in AWS Bedrock.
///
/// Each family corresponds to a different vendor and has its own request/response format:
///
/// - **Anthropic**: Claude models (claude-3-opus, claude-3-sonnet, claude-3-haiku, etc.)
/// - **Amazon**: Titan models (titan-text-express, titan-embed-text, etc.)
/// - **Meta**: Llama models (llama3-70b-instruct, llama2-70b-chat, etc.)
/// - **Mistral**: Mistral and Mixtral models (mistral-7b-instruct, mixtral-8x7b, etc.)
/// - **Cohere**: Command and Embed models (command-text, command-light-text, etc.)
/// - **AI21**: Jurassic and Jamba models (j2-ultra, jamba-instruct, etc.)
/// - **Stability**: Stable Diffusion models (stable-diffusion-xl, etc.) - Image generation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFamily {
    /// Anthropic models (Claude family)
    /// Model IDs: `anthropic.claude-*`
    Anthropic,

    /// Amazon models (Titan family)
    /// Model IDs: `amazon.titan-*`
    Amazon,

    /// Meta models (Llama family)
    /// Model IDs: `meta.llama*`
    Meta,

    /// Mistral AI models (Mistral and Mixtral families)
    /// Model IDs: `mistral.mistral-*`, `mistral.mixtral-*`
    Mistral,

    /// Cohere models (Command and Embed families)
    /// Model IDs: `cohere.command-*`, `cohere.embed-*`
    Cohere,

    /// AI21 Labs models (Jurassic and Jamba families)
    /// Model IDs: `ai21.j2-*`, `ai21.jamba-*`
    AI21,

    /// Stability AI models (Stable Diffusion family)
    /// Model IDs: `stability.stable-diffusion-*`
    /// Note: These are image generation models, not text completion
    Stability,
}

impl ModelFamily {
    /// Detect the model family from a Bedrock model ID.
    ///
    /// AWS Bedrock model IDs follow the format: `<vendor>.<model-name>-<version>`
    ///
    /// # Examples
    /// ```
    /// use llm::provider::bedrock::ModelFamily;
    ///
    /// assert_eq!(
    ///     ModelFamily::from_model_id("anthropic.claude-3-sonnet-20240229-v1:0").unwrap(),
    ///     ModelFamily::Anthropic
    /// );
    /// assert_eq!(
    ///     ModelFamily::from_model_id("amazon.titan-text-express-v1").unwrap(),
    ///     ModelFamily::Amazon
    /// );
    /// assert_eq!(
    ///     ModelFamily::from_model_id("meta.llama3-70b-instruct-v1:0").unwrap(),
    ///     ModelFamily::Meta
    /// );
    /// ```
    ///
    /// # Errors
    /// Returns an error if:
    /// - The model ID doesn't contain a dot (invalid format)
    /// - The vendor prefix is not recognized
    pub fn from_model_id(model_id: &str) -> Result<Self> {
        if !model_id.contains('.') {
            return Err(anyhow!("Invalid model ID format: '{model_id}' (missing vendor prefix)"));
        }

        let vendor = model_id
            .split('.')
            .next()
            .expect("split always returns at least one element");

        match vendor {
            "anthropic" => Ok(Self::Anthropic),
            "amazon" => Ok(Self::Amazon),
            "meta" => Ok(Self::Meta),
            "mistral" => Ok(Self::Mistral),
            "cohere" => Ok(Self::Cohere),
            "ai21" => Ok(Self::AI21),
            "stability" => Ok(Self::Stability),
            _ => Err(anyhow!(
                "Unknown model family for vendor: '{vendor}'. Supported vendors: anthropic, amazon, meta, mistral, cohere, ai21, stability"
            )),
        }
    }

    /// Get the vendor prefix for this model family.
    ///
    /// # Examples
    /// ```
    /// use llm::provider::bedrock::ModelFamily;
    ///
    /// assert_eq!(ModelFamily::Anthropic.vendor_prefix(), "anthropic");
    /// assert_eq!(ModelFamily::Amazon.vendor_prefix(), "amazon");
    /// ```
    pub fn vendor_prefix(&self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::Amazon => "amazon",
            Self::Meta => "meta",
            Self::Mistral => "mistral",
            Self::Cohere => "cohere",
            Self::AI21 => "ai21",
            Self::Stability => "stability",
        }
    }

    /// Check if this model family supports streaming responses.
    ///
    /// Based on AWS Bedrock documentation, not all model families support streaming:
    /// - **Anthropic**: ✅ All Claude models support streaming
    /// - **Amazon**: ✅ Titan Text models support streaming (not Embed models)
    /// - **Meta**: ✅ Llama 2 and Llama 3 models support streaming
    /// - **Mistral**: ✅ Mistral and Mixtral models support streaming
    /// - **Cohere**: ✅ Command models support streaming (not Embed models)
    /// - **AI21**: ❌ Jurassic and Jamba models do not support streaming
    /// - **Stability**: N/A Image generation uses different paradigm
    pub fn supports_streaming(&self) -> bool {
        match self {
            Self::Anthropic => true,
            Self::Amazon => true, // Note: Only Text models, not Embed models
            Self::Meta => true,
            Self::Mistral => true,
            Self::Cohere => true, // Note: Only Command models, not Embed models
            Self::AI21 => false,
            Self::Stability => false, // Image generation doesn't use text streaming
        }
    }

    /// Get a human-readable description of this model family.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic Claude models",
            Self::Amazon => "Amazon Titan models",
            Self::Meta => "Meta Llama models",
            Self::Mistral => "Mistral AI models",
            Self::Cohere => "Cohere Command and Embed models",
            Self::AI21 => "AI21 Labs Jurassic and Jamba models",
            Self::Stability => "Stability AI Stable Diffusion models",
        }
    }
}

impl std::fmt::Display for ModelFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.vendor_prefix())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_model_detection() {
        assert_eq!(
            ModelFamily::from_model_id("anthropic.claude-3-opus-20240229-v1:0").unwrap(),
            ModelFamily::Anthropic
        );
        assert_eq!(
            ModelFamily::from_model_id("anthropic.claude-3-sonnet-20240229-v1:0").unwrap(),
            ModelFamily::Anthropic
        );
        assert_eq!(
            ModelFamily::from_model_id("anthropic.claude-3-haiku-20240307-v1:0").unwrap(),
            ModelFamily::Anthropic
        );
        assert_eq!(
            ModelFamily::from_model_id("anthropic.claude-instant-v1").unwrap(),
            ModelFamily::Anthropic
        );
    }

    #[test]
    fn amazon_model_detection() {
        assert_eq!(
            ModelFamily::from_model_id("amazon.titan-text-express-v1").unwrap(),
            ModelFamily::Amazon
        );
        assert_eq!(
            ModelFamily::from_model_id("amazon.titan-text-lite-v1").unwrap(),
            ModelFamily::Amazon
        );
        assert_eq!(
            ModelFamily::from_model_id("amazon.titan-embed-text-v1").unwrap(),
            ModelFamily::Amazon
        );
    }

    #[test]
    fn meta_model_detection() {
        assert_eq!(
            ModelFamily::from_model_id("meta.llama3-70b-instruct-v1:0").unwrap(),
            ModelFamily::Meta
        );
        assert_eq!(
            ModelFamily::from_model_id("meta.llama2-70b-chat-v1").unwrap(),
            ModelFamily::Meta
        );
        assert_eq!(
            ModelFamily::from_model_id("meta.llama3-8b-instruct-v1:0").unwrap(),
            ModelFamily::Meta
        );
    }

    #[test]
    fn mistral_model_detection() {
        assert_eq!(
            ModelFamily::from_model_id("mistral.mistral-7b-instruct-v0:2").unwrap(),
            ModelFamily::Mistral
        );
        assert_eq!(
            ModelFamily::from_model_id("mistral.mixtral-8x7b-instruct-v0:1").unwrap(),
            ModelFamily::Mistral
        );
    }

    #[test]
    fn cohere_model_detection() {
        assert_eq!(
            ModelFamily::from_model_id("cohere.command-text-v14").unwrap(),
            ModelFamily::Cohere
        );
        assert_eq!(
            ModelFamily::from_model_id("cohere.command-light-text-v14").unwrap(),
            ModelFamily::Cohere
        );
        assert_eq!(
            ModelFamily::from_model_id("cohere.embed-english-v3").unwrap(),
            ModelFamily::Cohere
        );
    }

    #[test]
    fn ai21_model_detection() {
        assert_eq!(
            ModelFamily::from_model_id("ai21.j2-ultra-v1").unwrap(),
            ModelFamily::AI21
        );
        assert_eq!(ModelFamily::from_model_id("ai21.j2-mid-v1").unwrap(), ModelFamily::AI21);
        assert_eq!(
            ModelFamily::from_model_id("ai21.jamba-instruct-v1:0").unwrap(),
            ModelFamily::AI21
        );
    }

    #[test]
    fn stability_model_detection() {
        assert_eq!(
            ModelFamily::from_model_id("stability.stable-diffusion-xl-v1").unwrap(),
            ModelFamily::Stability
        );
    }

    #[test]
    fn unknown_vendor() {
        let result = ModelFamily::from_model_id("unknown.model-v1");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown model family"));
    }

    #[test]
    fn invalid_format() {
        let result = ModelFamily::from_model_id("no-dot-in-model-id");
        assert!(result.is_err());

        let error_msg = result.unwrap_err().to_string();
        println!("Actual error message: {}", error_msg);
        assert!(error_msg.contains("Invalid model ID format"));
    }

    #[test]
    fn streaming_support() {
        assert!(ModelFamily::Anthropic.supports_streaming());
        assert!(ModelFamily::Amazon.supports_streaming());
        assert!(ModelFamily::Meta.supports_streaming());
        assert!(ModelFamily::Mistral.supports_streaming());
        assert!(ModelFamily::Cohere.supports_streaming());
        assert!(!ModelFamily::AI21.supports_streaming());
        assert!(!ModelFamily::Stability.supports_streaming());
    }

    #[test]
    fn vendor_prefix() {
        assert_eq!(ModelFamily::Anthropic.vendor_prefix(), "anthropic");
        assert_eq!(ModelFamily::Amazon.vendor_prefix(), "amazon");
        assert_eq!(ModelFamily::Meta.vendor_prefix(), "meta");
        assert_eq!(ModelFamily::Mistral.vendor_prefix(), "mistral");
        assert_eq!(ModelFamily::Cohere.vendor_prefix(), "cohere");
        assert_eq!(ModelFamily::AI21.vendor_prefix(), "ai21");
        assert_eq!(ModelFamily::Stability.vendor_prefix(), "stability");
    }

    #[test]
    fn display() {
        assert_eq!(ModelFamily::Anthropic.to_string(), "anthropic");
        assert_eq!(ModelFamily::Amazon.to_string(), "amazon");
        assert_eq!(ModelFamily::Meta.to_string(), "meta");
    }
}
