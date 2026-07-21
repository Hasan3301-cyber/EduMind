use std::time::Duration;

use reqwest::{Client, Url};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    config::types::NotebookLmConfig,
    infra::{EduMindError, Result},
};

/// Routes NotebookLM operations through the local Python bridge first when configured.
#[derive(Clone)]
pub struct NotebookLmRouter {
    mcp: NotebookLmClient,
    python: NotebookLmClient,
}

impl NotebookLmRouter {
    /// Creates the MCP and optional Python bridge clients from the active configuration.
    pub fn new(mcp: NotebookLmConfig, python: NotebookLmConfig) -> Result<Self> {
        Ok(Self {
            mcp: NotebookLmClient::new(mcp)?,
            python: NotebookLmClient::new(python)?,
        })
    }

    /// Calls a NotebookLM MCP tool with a bounded JSON-RPC request.
    pub async fn call(&self, operation: &str, arguments: Value) -> Result<Value> {
        let supports_python = matches!(
            operation,
            "notebooklm_ask" | "notebooklm_list_notebooks" | "notebooklm_get_health"
        );
        if supports_python && self.python.enabled() && self.python.config.prefer_for_ask {
            match self.python.call(operation, arguments.clone()).await {
                Ok(result) => return Ok(with_transport("notebooklm_py", result)),
                Err(python_error) if self.python.config.fallback_to_mcp && self.mcp.enabled() => {
                    return self
                        .mcp
                        .call(operation, arguments)
                        .await
                        .map(|result| with_transport("notebooklm_mcp", result))
                        .map_err(|mcp_error| {
                            EduMindError::Tool(format!(
                                "NotebookLM Python bridge failed: {python_error}; MCP fallback failed: {mcp_error}"
                            ))
                        });
                }
                Err(error) => return Err(error),
            }
        }
        self.mcp
            .call(operation, arguments)
            .await
            .map(|result| with_transport("notebooklm_mcp", result))
    }

    /// Reports the preferred available NotebookLM integration health.
    pub async fn health(&self) -> Result<Value> {
        self.call("notebooklm_get_health", json!({})).await
    }

    /// Indicates whether either local integration has been enabled.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.mcp.enabled() || self.python.enabled()
    }
}

#[derive(Clone)]
struct NotebookLmClient {
    config: NotebookLmConfig,
    client: Client,
}

impl NotebookLmClient {
    fn new(config: NotebookLmConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(EduMindError::from)?;
        Ok(Self { config, client })
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }

    async fn call(&self, operation: &str, arguments: Value) -> Result<Value> {
        if !self.config.enabled {
            return Err(EduMindError::Tool(
                "NotebookLM integration is disabled; enable tools.notebooklm or tools.notebooklm_py"
                    .to_owned(),
            ));
        }
        let endpoint = self.endpoint()?;
        let request = json!({
            "jsonrpc": "2.0",
            "id": Uuid::new_v4().to_string(),
            "method": "tools/call",
            "params": {
                "name": operation,
                "arguments": arguments,
            },
        });
        let response = self
            .client
            .post(endpoint)
            .json(&request)
            .send()
            .await
            .map_err(|error| EduMindError::Tool(format!("NotebookLM request failed: {error}")))?
            .error_for_status()
            .map_err(|error| {
                EduMindError::Tool(format!("NotebookLM endpoint rejected request: {error}"))
            })?;
        let payload: Value = response.json().await.map_err(|error| {
            EduMindError::Tool(format!("NotebookLM returned invalid JSON: {error}"))
        })?;
        if let Some(error) = payload.get("error") {
            return Err(EduMindError::Tool(format!("NotebookLM MCP error: {error}")));
        }
        Ok(payload.get("result").cloned().unwrap_or(payload))
    }

    fn endpoint(&self) -> Result<&str> {
        let endpoint = self
            .config
            .endpoint
            .as_deref()
            .map(str::trim)
            .filter(|endpoint| !endpoint.is_empty())
            .ok_or_else(|| {
                EduMindError::Tool(
                    "NotebookLM integration requires a configured local MCP endpoint".to_owned(),
                )
            })?;
        Url::parse(endpoint).map_err(|error| {
            EduMindError::Tool(format!("NotebookLM endpoint is invalid: {error}"))
        })?;
        Ok(endpoint)
    }
}

fn with_transport(transport: &str, result: Value) -> Value {
    json!({
        "transport": transport,
        "result": result,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::NotebookLmRouter;
    use crate::config::types::NotebookLmConfig;

    #[tokio::test]
    async fn disabled_integrations_return_a_clear_error() {
        let router =
            NotebookLmRouter::new(NotebookLmConfig::default(), NotebookLmConfig::default())
                .unwrap();
        let error = router
            .call("notebooklm_ask", json!({"question": "test"}))
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("NotebookLM integration is disabled")
        );
    }
}
