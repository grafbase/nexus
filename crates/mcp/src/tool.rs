pub mod adder;

use std::borrow::Cow;

use futures_util::future::BoxFuture;
use rmcp::{
    RoleServer,
    model::{CallToolResult, ErrorCode, ErrorData, JsonObject, ToolAnnotations},
    service::RequestContext,
};
use schemars::{JsonSchema, schema_for};
use serde::de::DeserializeOwned;
use serde_json::Value;

pub(crate) trait Tool: Send + Sync + 'static {
    type Parameters: DeserializeOwned + JsonSchema;

    fn name() -> &'static str;
    fn description(&self) -> Cow<'_, str>;
    fn annotations(&self) -> ToolAnnotations;

    fn call(
        &self,
        parameters: Self::Parameters,
        http_headers: Option<http::HeaderMap>,
    ) -> impl Future<Output = anyhow::Result<CallToolResult>> + Send;
}

pub(crate) trait RmcpTool: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn to_tool(&self) -> rmcp::model::Tool;

    fn call(
        &self,
        parameters: Option<JsonObject>,
        context: RequestContext<RoleServer>,
    ) -> BoxFuture<'_, Result<CallToolResult, ErrorData>>;
}

impl<T: Tool> RmcpTool for T {
    fn name(&self) -> &str {
        T::name()
    }

    fn to_tool(&self) -> rmcp::model::Tool {
        let Value::Object(schema) = serde_json::to_value(schema_for!(<T as Tool>::Parameters)).unwrap() else {
            unreachable!()
        };

        rmcp::model::Tool::new(self.name().to_string(), self.description().into_owned(), schema)
            .annotate(self.annotations())
    }

    fn call(
        &self,
        parameters: Option<JsonObject>,
        mut context: RequestContext<RoleServer>,
    ) -> BoxFuture<'_, Result<CallToolResult, ErrorData>> {
        let http_headers = context
            .extensions
            .get_mut::<http::request::Parts>()
            .map(|parts| std::mem::take(&mut parts.headers));

        Box::pin(async move {
            let parameters: T::Parameters = serde_json::from_value(Value::Object(parameters.unwrap_or_default()))
                .map_err(|err| ErrorData::new(ErrorCode::INVALID_PARAMS, err.to_string(), None))?;

            match Tool::call(self, parameters, http_headers).await {
                Ok(data) => Ok(data),
                Err(err) => Err(ErrorData::new(ErrorCode::INTERNAL_ERROR, err.to_string(), None)),
            }
        })
    }
}
