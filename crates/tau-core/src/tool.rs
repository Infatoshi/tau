use async_trait::async_trait;
use serde_json::Value;
use tau_llm::ToolSchema;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn schema(&self) -> ToolSchema;

    async fn execute(
        &self,
        input: Value,
        cancellation: CancellationToken,
    ) -> anyhow::Result<ToolResult>;

    async fn cleanup(&self) -> anyhow::Result<()> {
        Ok(())
    }
}
