pub mod metrics;
pub mod tracing;

use std::borrow::Cow;

use futures::stream::BoxStream;

use crate::request::RequestContext;

const OPERATION_NAME: &str = "chat";

pub trait Request: Send + 'static {
    fn ctx(&self) -> &RequestContext;
    fn provider_name(&self) -> Cow<'static, str>;
    fn model(&self) -> &str;
    fn max_tokens(&self) -> Option<u32>;
    fn temperature(&self) -> Option<f32>;
}

pub trait Response: Send + 'static {
    type Complete: Message;
    type Stream: StreamResponse;

    fn as_message_or_stream_mut(&mut self) -> Result<&mut Self::Complete, &mut Self::Stream>;
}

pub trait StreamResponse: Send + 'static {
    type Event: Message;

    fn error_type(&self) -> Option<&str>;
    fn wrap_event_stream(&mut self, f: impl FnOnce(BoxStream<'static, Self::Event>) -> BoxStream<'static, Self::Event>);
}

pub trait Message: Send + 'static {
    fn error_type(&self) -> Option<&str>;
    fn id(&self) -> Option<&str>;
    fn model(&self) -> Option<&str>;
    fn tokens(&self) -> Option<Tokens>;
    /// Array of reasons the model stopped generating tokens, corresponding to each generation received.
    /// For example: `["stop", "length"]` or `["stop"]`
    fn finish_reasons(&self) -> Option<String>;
}

#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub input: u32,
    pub output: u32,
}
