use super::bus::LlmBus;
use super::bus::LlmBusError;
use super::types::LlmAction;
use super::types::LlmRequest;
use super::types::LlmResponse;
use super::types::LlmTextResponse;
use super::types::LlmTokenUsage;
use async_trait::async_trait;
use futures::StreamExt;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::Semaphore;
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
    stats: Arc<ManagedLlmBusStats>,
    semaphore: Arc<Semaphore>,
}

#[derive(Debug, Default)]
pub struct ManagedLlmBusStats {
    provider_calls: AtomicUsize,
    retry_attempts: AtomicUsize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ManagedLlmBusStatsSnapshot {
    pub provider_calls: usize,
    pub retry_attempts: usize,
    pub token_usage: LlmTokenUsage,
}

impl ManagedLlmBus {
    pub fn new(inner: Arc<dyn LlmBus>, config: ManagedLlmBusConfig) -> Self {
        let max_concurrent_requests = config.max_concurrent_requests.max(1);
        Self {
            inner,
            config: ManagedLlmBusConfig {
                max_concurrent_requests,
                ..config
            },
            stats: Arc::new(ManagedLlmBusStats::default()),
            semaphore: Arc::new(Semaphore::new(max_concurrent_requests)),
        }
    }

    pub fn stats(&self) -> ManagedLlmBusStatsSnapshot {
        ManagedLlmBusStatsSnapshot {
            provider_calls: self.stats.provider_calls.load(Ordering::Relaxed),
            retry_attempts: self.stats.retry_attempts.load(Ordering::Relaxed),
            token_usage: self.inner.token_usage(),
        }
    }

    async fn complete_text_with_retry(
        &self,
        request: LlmRequest,
    ) -> Result<LlmTextResponse, LlmBusError> {
        let mut attempt = 0;
        loop {
            self.stats.provider_calls.fetch_add(1, Ordering::Relaxed);
            match self.inner.complete_text(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(error) if should_retry(&error) && attempt < self.config.max_retries => {
                    self.stats.retry_attempts.fetch_add(1, Ordering::Relaxed);
                    sleep(backoff(self.config.initial_backoff, attempt)).await;
                    attempt += 1;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn complete_action_with_retry(
        &self,
        request: LlmRequest,
    ) -> Result<LlmAction, LlmBusError> {
        let mut attempt = 0;
        loop {
            self.stats.provider_calls.fetch_add(1, Ordering::Relaxed);
            match self.inner.complete_action(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(error) if should_retry(&error) && attempt < self.config.max_retries => {
                    self.stats.retry_attempts.fetch_add(1, Ordering::Relaxed);
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
    fn token_usage(&self) -> LlmTokenUsage {
        self.inner.token_usage()
    }

    async fn complete_text(&self, request: LlmRequest) -> Result<LlmTextResponse, LlmBusError> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| LlmBusError::Failed("llm concurrency limiter closed".to_string()))?;
        self.complete_text_with_retry(request).await
    }

    async fn complete_action(&self, request: LlmRequest) -> Result<LlmAction, LlmBusError> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| LlmBusError::Failed("llm concurrency limiter closed".to_string()))?;
        self.complete_action_with_retry(request).await
    }

    async fn complete_batch(
        &self,
        requests: Vec<LlmRequest>,
    ) -> Result<Vec<LlmResponse>, LlmBusError> {
        futures::stream::iter(requests.into_iter().map(|request| {
            let bus = self.clone();
            async move { bus.complete(request).await }
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
        assert_eq!(
            bus.stats(),
            ManagedLlmBusStatsSnapshot {
                provider_calls: 2,
                retry_attempts: 1,
                token_usage: LlmTokenUsage::default(),
            }
        );
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
        assert_eq!(bus.stats().provider_calls, 8);
        assert_eq!(bus.stats().retry_attempts, 0);
    }

    #[tokio::test]
    async fn limits_direct_complete_text_concurrency() {
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

        let calls = (0..8).map(|index| {
            let bus = bus.clone();
            async move {
                bus.complete_text(LlmRequest {
                    test_id: format!("test-{index}"),
                    model: "fake".to_string(),
                    instruction: "pass".to_string(),
                })
                .await
            }
        });
        let responses = futures::future::try_join_all(calls).await.unwrap();

        assert_eq!(responses.len(), 8);
        assert_eq!(inner.max_seen.load(Ordering::SeqCst), 2);
        assert_eq!(bus.stats().provider_calls, 8);
        assert_eq!(bus.stats().retry_attempts, 0);
    }

    struct FlakyBus {
        remaining_failures: Mutex<usize>,
        in_flight: AtomicUsize,
        max_seen: AtomicUsize,
    }

    #[async_trait]
    impl LlmBus for FlakyBus {
        async fn complete_text(
            &self,
            _request: LlmRequest,
        ) -> Result<LlmTextResponse, LlmBusError> {
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

            Ok(LlmTextResponse {
                content:
                    r#"{"status":"passed","severity":null,"description":"passed","evidence":[]}"#
                        .to_string(),
            })
        }
    }
}
