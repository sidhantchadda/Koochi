use super::bus::LlmBus;
use super::bus::LlmBusError;
use super::types::LlmRequest;
use super::types::LlmResponse;
use async_trait::async_trait;
use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Debug, Clone)]
pub struct ManagedLlmBusConfig {
    pub max_concurrent_requests: usize,
    pub max_retries: usize,
    pub initial_backoff: Duration,
}

impl Default for ManagedLlmBusConfig {
    fn default() -> Self {
        Self {
            max_concurrent_requests: 128,
            max_retries: 2,
            initial_backoff: Duration::from_millis(100),
        }
    }
}

#[derive(Clone)]
pub struct ManagedLlmBus {
    inner: Arc<dyn LlmBus>,
    config: ManagedLlmBusConfig,
}

impl ManagedLlmBus {
    pub fn new(inner: Arc<dyn LlmBus>, config: ManagedLlmBusConfig) -> Self {
        Self {
            inner,
            config: ManagedLlmBusConfig {
                max_concurrent_requests: config.max_concurrent_requests.max(1),
                ..config
            },
        }
    }

    async fn complete_with_retry(&self, request: LlmRequest) -> Result<LlmResponse, LlmBusError> {
        let mut attempt = 0;
        loop {
            match self.inner.complete(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(error) if should_retry(&error) && attempt < self.config.max_retries => {
                    sleep(backoff(self.config.initial_backoff, attempt)).await;
                    attempt += 1;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

#[async_trait]
impl LlmBus for ManagedLlmBus {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, LlmBusError> {
        self.complete_with_retry(request).await
    }

    async fn complete_batch(
        &self,
        requests: Vec<LlmRequest>,
    ) -> Result<Vec<LlmResponse>, LlmBusError> {
        futures::stream::iter(requests.into_iter().map(|request| {
            let bus = self.clone();
            async move { bus.complete_with_retry(request).await }
        }))
        .buffered(self.config.max_concurrent_requests)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect()
    }
}

fn should_retry(error: &LlmBusError) -> bool {
    match error {
        LlmBusError::Http(_) => true,
        LlmBusError::HttpStatus { status, .. } => {
            *status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
        }
        LlmBusError::Failed(_) => true,
        LlmBusError::MissingApiKey(_)
        | LlmBusError::InvalidHeader(_)
        | LlmBusError::InvalidVerdict(_) => false,
    }
}

fn backoff(initial: Duration, attempt: usize) -> Duration {
    initial.saturating_mul(2_u32.saturating_pow(attempt as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::TestStatus;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn retries_transient_failures() {
        let inner = Arc::new(FlakyBus {
            remaining_failures: Mutex::new(1),
            in_flight: AtomicUsize::new(0),
            max_seen: AtomicUsize::new(0),
        });
        let bus = ManagedLlmBus::new(
            inner.clone(),
            ManagedLlmBusConfig {
                max_concurrent_requests: 2,
                max_retries: 2,
                initial_backoff: Duration::ZERO,
            },
        );

        let response = bus
            .complete(LlmRequest {
                test_id: "one".to_string(),
                model: "fake".to_string(),
                instruction: "pass".to_string(),
            })
            .await
            .unwrap();

        assert_eq!(response.status, TestStatus::Passed);
    }

    #[tokio::test]
    async fn limits_batch_concurrency() {
        let inner = Arc::new(FlakyBus {
            remaining_failures: Mutex::new(0),
            in_flight: AtomicUsize::new(0),
            max_seen: AtomicUsize::new(0),
        });
        let bus = ManagedLlmBus::new(
            inner.clone(),
            ManagedLlmBusConfig {
                max_concurrent_requests: 2,
                max_retries: 0,
                initial_backoff: Duration::ZERO,
            },
        );

        let requests = (0..8)
            .map(|index| LlmRequest {
                test_id: format!("test-{index}"),
                model: "fake".to_string(),
                instruction: "pass".to_string(),
            })
            .collect();
        let responses = bus.complete_batch(requests).await.unwrap();

        assert_eq!(responses.len(), 8);
        assert_eq!(inner.max_seen.load(Ordering::SeqCst), 2);
    }

    struct FlakyBus {
        remaining_failures: Mutex<usize>,
        in_flight: AtomicUsize,
        max_seen: AtomicUsize,
    }

    #[async_trait]
    impl LlmBus for FlakyBus {
        async fn complete(&self, _request: LlmRequest) -> Result<LlmResponse, LlmBusError> {
            let in_flight = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_seen.fetch_max(in_flight, Ordering::SeqCst);
            tokio::task::yield_now().await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);

            let mut remaining = self.remaining_failures.lock().await;
            if *remaining > 0 {
                *remaining -= 1;
                return Err(LlmBusError::HttpStatus {
                    status: reqwest::StatusCode::TOO_MANY_REQUESTS,
                    body: "rate limited".to_string(),
                });
            }

            Ok(LlmResponse {
                status: TestStatus::Passed,
                severity: None,
                description: "passed".to_string(),
                evidence: Vec::new(),
            })
        }
    }
}
