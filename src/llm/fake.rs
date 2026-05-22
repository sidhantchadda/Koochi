use super::bus::LlmBus;
use super::bus::LlmBusError;
use super::types::LlmRequest;
use super::types::LlmResponse;
use super::types::TestStatus;
use crate::Severity;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Default, Clone)]
pub struct FakeLlmBus {
    requests: Arc<Mutex<Vec<LlmRequest>>>,
    batches: Arc<Mutex<Vec<usize>>>,
}

impl FakeLlmBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn requests(&self) -> Vec<LlmRequest> {
        self.requests.lock().await.clone()
    }

    pub async fn batches(&self) -> Vec<usize> {
        self.batches.lock().await.clone()
    }
}

#[async_trait]
impl LlmBus for FakeLlmBus {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, LlmBusError> {
        self.requests.lock().await.push(request.clone());
        let lower = request.instruction.to_ascii_lowercase();
        let failed =
            lower.contains("missing") || lower.contains("retry") || lower.contains("authorization");
        if failed {
            Ok(LlmResponse {
                status: TestStatus::Failed,
                severity: Some(Severity::Medium),
                description: format!("Fake bus flagged `{}` for review.", request.test_id),
                evidence: Vec::new(),
            })
        } else {
            Ok(LlmResponse {
                status: TestStatus::Passed,
                severity: None,
                description: format!("Fake bus passed `{}`.", request.test_id),
                evidence: Vec::new(),
            })
        }
    }

    async fn complete_batch(
        &self,
        requests: Vec<LlmRequest>,
    ) -> Result<Vec<LlmResponse>, LlmBusError> {
        self.batches.lock().await.push(requests.len());
        let mut responses = Vec::with_capacity(requests.len());
        for request in requests {
            responses.push(self.complete(request).await?);
        }
        Ok(responses)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_bus_records_requests_and_flags_failures() {
        let bus = FakeLlmBus::new();
        let response = bus
            .complete(LlmRequest {
                test_id: "retry".to_string(),
                model: "gpt-5.4-nano".to_string(),
                instruction: "Check retry policy".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(response.status, TestStatus::Failed);
        assert_eq!(bus.requests().await.len(), 1);
    }
}
