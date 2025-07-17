mod execute;
mod search;

pub use execute::ExecuteTool;
pub use search::SearchTool;

use std::borrow::Cow;

use axum::http::request::Parts;
use futures_util::future::BoxFuture;
use rmcp::{
    ErrorData, RoleServer,
    model::{CallToolResult, Content, ErrorCode, JsonObject, ToolAnnotations},
    serde_json::{self, Value},
    service::RequestContext,
};
use schemars::schema_for;
use serde::de::DeserializeOwned;

pub(crate) trait Tool: Send + Sync + 'static {
    type Parameters: DeserializeOwned + schemars::JsonSchema;

    fn name() -> &'static str;
    fn description(&self) -> Cow<'_, str>;
    fn annotations(&self) -> ToolAnnotations;

    fn call(
        &self,
        parts: Parts,
        parameters: Self::Parameters,
    ) -> impl Future<Output = anyhow::Result<CallToolResult>> + Send;
}

pub(crate) trait RmcpTool: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn to_tool(&self) -> rmcp::model::Tool;

    fn call(
        &self,
        ctx: RequestContext<RoleServer>,
        parameters: Option<JsonObject>,
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
        mut ctx: RequestContext<RoleServer>,
        parameters: Option<JsonObject>,
    ) -> BoxFuture<'_, Result<CallToolResult, ErrorData>> {
        let parts = ctx
            .extensions
            .remove::<Parts>()
            .unwrap_or_else(|| http::Request::builder().body(Vec::<u8>::new()).unwrap().into_parts().0);

        Box::pin(async move {
            let parameters: T::Parameters = serde_json::from_value(Value::Object(parameters.unwrap_or_default()))
                .map_err(|err| ErrorData::new(ErrorCode::INVALID_PARAMS, err.to_string(), None))?;

            match Tool::call(self, parts, parameters).await {
                Ok(data) => Ok(data),
                Err(err) => {
                    // Try to downcast the anyhow::Error back to ErrorData
                    if let Some(error_data) = err.downcast_ref::<ErrorData>() {
                        Err(error_data.clone())
                    } else {
                        Err(ErrorData::new(ErrorCode::INTERNAL_ERROR, err.to_string(), None))
                    }
                }
            }
        })
    }
}

struct SdlAndErrors {
    sdl: String,
    errors: Vec<String>,
}

impl From<SdlAndErrors> for CallToolResult {
    fn from(SdlAndErrors { sdl, errors }: SdlAndErrors) -> Self {
        let mut content = Vec::new();

        if !sdl.is_empty() {
            content.push(Content::text(sdl));
        }

        if !errors.is_empty() {
            content.push(Content::json(ErrorList { errors }).unwrap());
        }

        CallToolResult {
            content,
            is_error: None,
        }
    }
}

#[derive(serde::Serialize)]
struct ErrorList<T> {
    errors: Vec<T>,
}
