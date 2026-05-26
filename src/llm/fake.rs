use super::bus::LlmBus;
use super::bus::LlmBusError;
use super::types::LlmRequest;
use super::types::LlmResponse;
use super::types::LlmTextResponse;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Default, Clone)]
pub struct FakeLlmBus {
    requests: Arc<Mutex<Vec<LlmRequest>>>,
    batches: Arc<Mutex<Vec<usize>>>,
    turns: Arc<Mutex<HashMap<String, usize>>>,
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
    async fn complete_text(&self, request: LlmRequest) -> Result<LlmTextResponse, LlmBusError> {
        self.requests.lock().await.push(request.clone());
        let instruction = fake_test_instruction(&request.instruction);
        if let Some(content) = scripted_response(&request.test_id, instruction, &self.turns).await {
            return Ok(LlmTextResponse { content });
        }

        let lower = instruction.to_ascii_lowercase();
        let failed =
            lower.contains("missing") || lower.contains("retry") || lower.contains("authorization");
        if failed {
            Ok(LlmTextResponse {
                content: format!(
                    r#"{{"action":"final","status":"failed","severity":"medium","description":"Fake bus flagged `{}` for review.","evidence":[]}}"#,
                    request.test_id
                ),
            })
        } else {
            Ok(LlmTextResponse {
                content: format!(
                    r#"{{"action":"final","status":"passed","severity":null,"description":"Fake bus passed `{}`.","evidence":[]}}"#,
                    request.test_id
                ),
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

async fn scripted_response(
    test_id: &str,
    _instruction: &str,
    turns: &Mutex<HashMap<String, usize>>,
) -> Option<String> {
    let mut turns = turns.lock().await;
    let turn = turns.entry(test_id.to_string()).or_insert(0);
    *turn += 1;
    let turn = *turn;

    if test_id == "tool-triad" {
        return Some(match turn {
            1 => r#"{"action":"get_file_context","path":"src/review.rs","line":3}"#.to_string(),
            2 => r#"{"action":"find_definitions","symbol":"dangerous_sink"}"#.to_string(),
            3 => r#"{"action":"find_references","symbol":"dangerous_sink"}"#.to_string(),
            _ => r#"{"action":"final","status":"failed","severity":"high","description":"Scripted tool triad completed.","evidence":[{"path":"src/review.rs","line":3,"preview":"    dangerous_sink(input);"}]}"#.to_string(),
        });
    }

    if test_id == "multi-turn" {
        return Some(match turn {
            1 => r#"{"action":"search_text","query":"TODO_KOOCHI_MULTI","kind":"source"}"#.to_string(),
            2 => r#"{"action":"read_file","path":"src/review.rs"}"#.to_string(),
            _ => r#"{"action":"final","status":"failed","severity":"medium","description":"Scripted multi-turn reasoning completed after observing search and file content.","evidence":[{"path":"src/review.rs","line":5,"preview":"    // TODO_KOOCHI_MULTI: retry policy is missing"}]}"#.to_string(),
        });
    }

    None
}

fn fake_test_instruction(instruction: &str) -> &str {
    let Some(start) = instruction.find("Agentic invariant:") else {
        return instruction;
    };
    let subject = &instruction[start + "Agentic invariant:".len()..];
    subject
        .split("Repository context:")
        .next()
        .unwrap_or(subject)
        .trim()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::TestStatus;

    #[tokio::test]
    async fn fake_bus_records_requests_and_flags_failures() {
        let bus = FakeLlmBus::new();
        let response = bus
            .complete(LlmRequest {
                test_id: "retry".to_string(),
                model: "gpt-5-nano".to_string(),
                instruction: "Check retry policy".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(response.status, TestStatus::Failed);
        assert_eq!(bus.requests().await.len(), 1);
    }
}
