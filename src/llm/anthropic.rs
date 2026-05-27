use super::bus::LlmBus;
use super::bus::LlmBusError;
use super::tools::default_status_for_test_id;
use super::tools::parse_tool_action_from_value;
use super::tools::tool_specs;
use super::types::LlmAction;
use super::types::LlmRequest;
use super::types::LlmTextResponse;
use super::types::LlmTokenUsage;
use crate::config::KoochiConfig;
use crate::prompts::verdict_system_prompt;
use async_trait::async_trait;
use reqwest::header::CONTENT_TYPE;
use reqwest::header::HeaderMap;
use reqwest::header::HeaderValue;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

#[derive(Debug, Clone)]
pub struct AnthropicBus {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    usage: Arc<AnthropicUsageStats>,
}

impl AnthropicBus {
    pub fn from_config(config: &KoochiConfig) -> Result<Self, LlmBusError> {
        let api_key_env = config
            .api_key_env
            .clone()
            .unwrap_or_else(|| "ANTHROPIC_API_KEY".to_string());
        Ok(Self {
            client: reqwest::Client::new(),
            api_key: std::env::var(&api_key_env)
                .map_err(|_| LlmBusError::MissingApiKey(api_key_env))?,
            base_url: config
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string()),
            usage: Arc::new(AnthropicUsageStats::default()),
        })
    }
}

#[async_trait]
impl LlmBus for AnthropicBus {
    fn token_usage(&self) -> LlmTokenUsage {
        self.usage.snapshot()
    }

    async fn complete_text(&self, request: LlmRequest) -> Result<LlmTextResponse, LlmBusError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&self.api_key)
                .map_err(|_| LlmBusError::InvalidHeader("x-api-key"))?,
        );
        let url = format!("{}/messages", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .headers(headers)
            .json(&AnthropicMessageRequest {
                model: request.model.clone(),
                max_tokens: 1024,
                system: verdict_system_prompt(),
                messages: vec![AnthropicMessage {
                    role: "user",
                    content: request.instruction.clone(),
                }],
                tools: Vec::new(),
            })
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(LlmBusError::HttpStatus { status, body });
        }
        let parsed: AnthropicMessageResponse =
            serde_json::from_str(&body).map_err(|_| LlmBusError::InvalidVerdict(body.clone()))?;
        if let Some(usage) = &parsed.usage {
            self.usage.record(usage);
        }
        let content = parsed
            .content
            .into_iter()
            .find_map(|content| match content {
                AnthropicContent::Text { text } => Some(text),
                AnthropicContent::ToolUse { .. } => None,
            })
            .ok_or_else(|| LlmBusError::InvalidVerdict(body.clone()))?;
        Ok(LlmTextResponse { content })
    }

    async fn complete_action(&self, request: LlmRequest) -> Result<LlmAction, LlmBusError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&self.api_key)
                .map_err(|_| LlmBusError::InvalidHeader("x-api-key"))?,
        );
        let url = format!("{}/messages", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .headers(headers)
            .json(&AnthropicMessageRequest {
                model: request.model.clone(),
                max_tokens: 1024,
                system: verdict_system_prompt(),
                messages: vec![AnthropicMessage {
                    role: "user",
                    content: request.instruction.clone(),
                }],
                tools: tool_definitions(),
            })
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(LlmBusError::HttpStatus { status, body });
        }
        let parsed: AnthropicMessageResponse =
            serde_json::from_str(&body).map_err(|_| LlmBusError::InvalidVerdict(body.clone()))?;
        if let Some(usage) = &parsed.usage {
            self.usage.record(usage);
        }
        for content in parsed.content {
            match content {
                AnthropicContent::ToolUse { name, input, .. } => {
                    return parse_tool_use(
                        &name,
                        input,
                        default_status_for_test_id(&request.test_id),
                    );
                }
                AnthropicContent::Text { text } if !text.trim().is_empty() => {
                    return Ok(LlmAction::Text(text));
                }
                AnthropicContent::Text { .. } => {}
            }
        }
        Err(LlmBusError::InvalidVerdict(body))
    }
}

#[derive(Debug, Serialize)]
struct AnthropicMessageRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    tools: Vec<AnthropicTool>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: &'static str,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessageResponse {
    content: Vec<AnthropicContent>,
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[derive(Debug, Default)]
struct AnthropicUsageStats {
    input_tokens: AtomicU64,
    output_tokens: AtomicU64,
}

impl AnthropicUsageStats {
    fn record(&self, usage: &AnthropicUsage) {
        self.input_tokens
            .fetch_add(usage.input_tokens.unwrap_or_default(), Ordering::Relaxed);
        self.output_tokens
            .fetch_add(usage.output_tokens.unwrap_or_default(), Ordering::Relaxed);
    }

    fn snapshot(&self) -> LlmTokenUsage {
        let prompt_tokens = self.input_tokens.load(Ordering::Relaxed);
        let completion_tokens = self.output_tokens.load(Ordering::Relaxed);
        LlmTokenUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AnthropicContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        #[allow(dead_code)]
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: &'static str,
    description: &'static str,
    input_schema: serde_json::Value,
}

fn parse_tool_use(
    name: &str,
    input: serde_json::Value,
    default_status: Option<super::types::TestStatus>,
) -> Result<LlmAction, LlmBusError> {
    parse_tool_action_from_value(name, input, default_status)
}

fn tool_definitions() -> Vec<AnthropicTool> {
    tool_specs()
        .into_iter()
        .map(|spec| AnthropicTool {
            name: spec.name,
            description: spec.description,
            input_schema: spec.schema,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Evidence;
    use crate::LlmResponse;
    use crate::LlmToolCall;
    use crate::Severity;
    use crate::TestStatus;
    use serde_json::json;

    #[test]
    fn parses_native_tool_use() {
        let action = parse_tool_use(
            "find_references",
            json!({
                "symbol": "dangerous_sink"
            }),
            None,
        )
        .unwrap();
        assert_eq!(
            action,
            LlmAction::Tool(LlmToolCall::FindReferences {
                symbol: "dangerous_sink".to_string(),
            })
        );
    }

    #[test]
    fn parses_native_review_hunks_tool_use() {
        let action = parse_tool_use("list_review_hunks", json!({}), None).unwrap();
        assert_eq!(action, LlmAction::Tool(LlmToolCall::ListReviewHunks));
    }

    #[test]
    fn parses_native_hunk_context_tool_use() {
        let action =
            parse_tool_use("get_hunk_context", json!({"hunk_id": "src/lib.rs#1"}), None).unwrap();
        assert_eq!(
            action,
            LlmAction::Tool(LlmToolCall::GetHunkContext {
                hunk_id: "src/lib.rs#1".to_string(),
            })
        );
    }

    #[test]
    fn parses_native_final_verdict_tool_use() {
        let action = parse_tool_use(
            "final_verdict",
            json!({
                "status": "failed",
                "severity": "medium",
                "description": "bad thing",
                "evidence": [{"path": "src/lib.rs", "line": 3, "preview": "bad();"}]
            }),
            None,
        )
        .unwrap();
        assert_eq!(
            action,
            LlmAction::Final(LlmResponse {
                status: TestStatus::Failed,
                severity: Some(Severity::Medium),
                description: "bad thing".to_string(),
                evidence: vec![Evidence {
                    path: "src/lib.rs".to_string(),
                    line: 3,
                    preview: "bad();".to_string(),
                }],
            })
        );
    }

    #[test]
    fn parses_native_final_verdict_with_default_status() {
        let action = parse_tool_use(
            "final_verdict",
            json!({
                "description": "safe route found",
                "severity": "low",
                "evidence": [{"path": "src/workflows.rs", "line": 189, "preview": "ensure_workflow_route();"}]
            }),
            Some(TestStatus::Passed),
        )
        .unwrap();
        assert!(matches!(
            action,
            LlmAction::Final(LlmResponse {
                status: TestStatus::Passed,
                ..
            })
        ));
    }
}
