use super::anthropic::AnthropicBus;
use super::fake::FakeLlmBus;
use super::managed::ManagedLlmBus;
use super::managed::ManagedLlmBusConfig;
use super::managed::ManagedLlmBusStatsSnapshot;
use super::openai::OpenAiBus;
use super::types::LlmAction;
use super::types::LlmRequest;
use super::types::LlmResponse;
use super::types::LlmTextResponse;
use super::verdict_parser::parse_verdict;
use crate::config::AiProvider;
use crate::config::KoochiConfig;
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum LlmBusError {
    #[error("llm bus failed: {0}")]
    Failed(String),
    #[error("missing API key env var `{0}`")]
    MissingApiKey(String),
    #[error("invalid header value for `{0}`")]
    InvalidHeader(&'static str),
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("provider returned status {status}: {body}")]
    HttpStatus {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error("provider response did not contain parseable verdict JSON: {0}")]
    InvalidVerdict(String),
}

#[async_trait]
pub trait LlmBus: Send + Sync {
    async fn complete_text(&self, request: LlmRequest) -> Result<LlmTextResponse, LlmBusError>;

    async fn complete_action(&self, request: LlmRequest) -> Result<LlmAction, LlmBusError> {
        let response = self.complete_text(request).await?;
        Ok(LlmAction::Text(response.content))
    }

    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, LlmBusError> {
        let response = self.complete_text(request).await?;
        parse_verdict(&response.content)
    }

    async fn complete_batch(
        &self,
        requests: Vec<LlmRequest>,
    ) -> Result<Vec<LlmResponse>, LlmBusError> {
        let mut responses = Vec::with_capacity(requests.len());
        for request in requests {
            responses.push(self.complete(request).await?);
        }
        Ok(responses)
    }
}

#[derive(Clone)]
pub struct BuiltLlmBus {
    pub bus: Arc<dyn LlmBus>,
    managed: Arc<ManagedLlmBus>,
}

impl BuiltLlmBus {
    pub fn stats(&self) -> ManagedLlmBusStatsSnapshot {
        self.managed.stats()
    }
}

pub fn build_llm_bus(config: &KoochiConfig) -> Result<BuiltLlmBus, LlmBusError> {
    let provider: Arc<dyn LlmBus> = match config.provider {
        AiProvider::Fake => Arc::new(FakeLlmBus::new()),
        AiProvider::OpenAi => Arc::new(OpenAiBus::from_config(config)?),
        AiProvider::Anthropic => Arc::new(AnthropicBus::from_config(config)?),
    };
    let managed = Arc::new(ManagedLlmBus::new(
        provider,
        ManagedLlmBusConfig {
            max_concurrent_requests: config.max_parallel_llm_requests,
            max_retries: config.llm_max_retries,
            ..ManagedLlmBusConfig::default()
        },
    ));
    Ok(BuiltLlmBus {
        bus: managed.clone(),
        managed,
    })
}
