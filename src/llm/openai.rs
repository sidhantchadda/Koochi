use super::bus::LlmBus;
use super::bus::LlmBusError;
use super::types::LlmAction;
use super::types::LlmRequest;
use super::types::LlmTextResponse;
use super::types::LlmTokenUsage;
use super::types::LlmToolCall;
use super::verdict_parser::parse_verdict_with_default_status;
use crate::config::KoochiConfig;
use crate::prompts::verdict_system_prompt;
use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

#[derive(Debug, Clone)]
pub struct OpenAiBus {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    usage: Arc<OpenAiUsageStats>,
}

impl OpenAiBus {
    pub fn from_config(config: &KoochiConfig) -> Result<Self, LlmBusError> {
        let api_key_env = config
            .api_key_env
            .clone()
            .unwrap_or_else(|| "OPENAI_API_KEY".to_string());
        Ok(Self {
            client: reqwest::Client::new(),
            api_key: std::env::var(&api_key_env)
                .map_err(|_| LlmBusError::MissingApiKey(api_key_env))?,
            base_url: config
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            usage: Arc::new(OpenAiUsageStats::default()),
        })
    }
}

#[async_trait]
impl LlmBus for OpenAiBus {
    fn token_usage(&self) -> LlmTokenUsage {
        self.usage.snapshot()
    }

    async fn complete_text(&self, request: LlmRequest) -> Result<LlmTextResponse, LlmBusError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&OpenAiChatRequest {
                model: request.model.clone(),
                messages: vec![
                    OpenAiMessage {
                        role: "system",
                        content: verdict_system_prompt(),
                    },
                    OpenAiMessage {
                        role: "user",
                        content: request.instruction.clone(),
                    },
                ],
                temperature: temperature_for_model(&request.model),
                tools: Vec::new(),
                tool_choice: None,
            })
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(LlmBusError::HttpStatus { status, body });
        }
        let parsed: OpenAiChatResponse =
            serde_json::from_str(&body).map_err(|_| LlmBusError::InvalidVerdict(body.clone()))?;
        record_usage(
            &self.usage,
            &request,
            parsed.usage.as_ref(),
            "complete_text",
        );
        let content = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .ok_or_else(|| LlmBusError::InvalidVerdict(body.clone()))?;
        Ok(LlmTextResponse { content })
    }

    async fn complete_action(&self, request: LlmRequest) -> Result<LlmAction, LlmBusError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&OpenAiChatRequest {
                model: request.model.clone(),
                messages: vec![
                    OpenAiMessage {
                        role: "system",
                        content: verdict_system_prompt(),
                    },
                    OpenAiMessage {
                        role: "user",
                        content: request.instruction.clone(),
                    },
                ],
                temperature: temperature_for_model(&request.model),
                tools: tool_definitions(),
                tool_choice: Some("auto"),
            })
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(LlmBusError::HttpStatus { status, body });
        }
        let parsed: OpenAiChatResponse =
            serde_json::from_str(&body).map_err(|_| LlmBusError::InvalidVerdict(body.clone()))?;
        record_usage(
            &self.usage,
            &request,
            parsed.usage.as_ref(),
            "complete_action",
        );
        let message = parsed
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message)
            .ok_or_else(|| LlmBusError::InvalidVerdict(body.clone()))?;
        if let Some(tool_call) = message.tool_calls.unwrap_or_default().into_iter().next() {
            return parse_tool_call(tool_call, default_status_for_test_id(&request.test_id));
        }
        if let Some(content) = message.content {
            return Ok(LlmAction::Text(content));
        }
        Err(LlmBusError::InvalidVerdict(body))
    }
}

#[derive(Debug, Serialize)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    tools: Vec<OpenAiTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct OpenAiMessage {
    role: &'static str,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

#[derive(Debug, Default)]
struct OpenAiUsageStats {
    prompt_tokens: AtomicU64,
    completion_tokens: AtomicU64,
    total_tokens: AtomicU64,
}

impl OpenAiUsageStats {
    fn record(&self, usage: &OpenAiUsage) {
        let prompt_tokens = usage.prompt_tokens.unwrap_or_default();
        let completion_tokens = usage.completion_tokens.unwrap_or_default();
        let total_tokens = usage
            .total_tokens
            .unwrap_or(prompt_tokens + completion_tokens);
        self.prompt_tokens
            .fetch_add(prompt_tokens, Ordering::Relaxed);
        self.completion_tokens
            .fetch_add(completion_tokens, Ordering::Relaxed);
        self.total_tokens.fetch_add(total_tokens, Ordering::Relaxed);
    }

    fn snapshot(&self) -> LlmTokenUsage {
        LlmTokenUsage {
            prompt_tokens: self.prompt_tokens.load(Ordering::Relaxed),
            completion_tokens: self.completion_tokens.load(Ordering::Relaxed),
            total_tokens: self.total_tokens.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoiceMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolCall {
    function: OpenAiToolCallFunction,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolCallFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct OpenAiTool {
    #[serde(rename = "type")]
    kind: &'static str,
    function: OpenAiToolFunction,
}

#[derive(Debug, Serialize)]
struct OpenAiToolFunction {
    name: &'static str,
    description: &'static str,
    parameters: serde_json::Value,
}

fn temperature_for_model(model: &str) -> Option<f32> {
    (!model.starts_with("gpt-5")).then_some(0.0)
}

fn record_usage(
    stats: &OpenAiUsageStats,
    request: &LlmRequest,
    usage: Option<&OpenAiUsage>,
    operation: &'static str,
) {
    let Some(usage) = usage else {
        return;
    };
    stats.record(usage);
    let Some(path) = std::env::var_os("KOOCHI_TOKEN_USAGE_LOG") else {
        return;
    };
    let line = json!({
        "provider": "openai",
        "operation": operation,
        "test_id": request.test_id,
        "model": request.model,
        "prompt_tokens": usage.prompt_tokens,
        "completion_tokens": usage.completion_tokens,
        "total_tokens": usage.total_tokens,
    })
    .to_string();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

fn parse_tool_call(
    tool_call: OpenAiToolCall,
    default_status: Option<super::types::TestStatus>,
) -> Result<LlmAction, LlmBusError> {
    match tool_call.function.name.as_str() {
        "list_files" => {
            let args: KindArgs = parse_args(&tool_call.function.arguments)?;
            Ok(LlmAction::Tool(LlmToolCall::ListFiles { kind: args.kind }))
        }
        "list_review_hunks" => Ok(LlmAction::Tool(LlmToolCall::ListReviewHunks)),
        "get_hunk_context" => {
            let args: HunkIdArgs = parse_args(&tool_call.function.arguments)?;
            Ok(LlmAction::Tool(LlmToolCall::GetHunkContext {
                hunk_id: args.hunk_id,
            }))
        }
        "search_text" => {
            let args: SearchTextArgs = parse_args(&tool_call.function.arguments)?;
            Ok(LlmAction::Tool(LlmToolCall::SearchText {
                query: args.query,
                kind: args.kind,
            }))
        }
        "read_file" => {
            let args: PathArgs = parse_args(&tool_call.function.arguments)?;
            Ok(LlmAction::Tool(LlmToolCall::ReadFile { path: args.path }))
        }
        "get_file_context" => {
            let args: ContextArgs = parse_args(&tool_call.function.arguments)?;
            Ok(LlmAction::Tool(LlmToolCall::GetFileContext {
                path: args.path,
                line: args.line,
            }))
        }
        "find_definitions" => {
            let args: SymbolArgs = parse_args(&tool_call.function.arguments)?;
            Ok(LlmAction::Tool(LlmToolCall::FindDefinitions {
                symbol: args.symbol,
            }))
        }
        "find_references" => {
            let args: SymbolArgs = parse_args(&tool_call.function.arguments)?;
            Ok(LlmAction::Tool(LlmToolCall::FindReferences {
                symbol: args.symbol,
            }))
        }
        "final_verdict" => {
            let response =
                parse_verdict_with_default_status(&tool_call.function.arguments, default_status)?;
            Ok(LlmAction::Final(response))
        }
        _ => Err(LlmBusError::InvalidVerdict(format!(
            "unsupported OpenAI tool call `{}`",
            tool_call.function.name
        ))),
    }
}

fn default_status_for_test_id(test_id: &str) -> Option<super::types::TestStatus> {
    if test_id.starts_with("pass-") {
        Some(super::types::TestStatus::Passed)
    } else if test_id.starts_with("fail-") {
        Some(super::types::TestStatus::Failed)
    } else {
        None
    }
}

fn parse_args<T>(arguments: &str) -> Result<T, LlmBusError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_str(arguments).map_err(|_| LlmBusError::InvalidVerdict(arguments.to_string()))
}

#[derive(Debug, Deserialize)]
struct KindArgs {
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchTextArgs {
    query: String,
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PathArgs {
    path: String,
}

#[derive(Debug, Deserialize)]
struct HunkIdArgs {
    hunk_id: String,
}

#[derive(Debug, Deserialize)]
struct ContextArgs {
    path: String,
    line: u32,
}

#[derive(Debug, Deserialize)]
struct SymbolArgs {
    symbol: String,
}

fn tool_definitions() -> Vec<OpenAiTool> {
    vec![
        tool(
            "list_files",
            "List repo files by kind.",
            object_schema(
                vec![(
                    "kind",
                    string_enum_schema(&["source", "tests", "configs", "all"]),
                )],
                vec![],
            ),
        ),
        tool(
            "list_review_hunks",
            "List changed review hunks with exact changed line numbers.",
            object_schema(vec![], vec![]),
        ),
        tool(
            "get_hunk_context",
            "Read bounded surrounding code for a specific changed review hunk id.",
            object_schema(vec![("hunk_id", json!({"type":"string"}))], vec!["hunk_id"]),
        ),
        tool(
            "search_text",
            "Search source text literally.",
            object_schema(
                vec![
                    ("query", json!({"type":"string"})),
                    (
                        "kind",
                        string_enum_schema(&["source", "tests", "configs", "all"]),
                    ),
                ],
                vec!["query"],
            ),
        ),
        tool(
            "read_file",
            "Read a complete repo-relative file.",
            object_schema(vec![("path", json!({"type":"string"}))], vec!["path"]),
        ),
        tool(
            "get_file_context",
            "Read a fixed-radius context window around a line.",
            object_schema(
                vec![
                    ("path", json!({"type":"string"})),
                    ("line", json!({"type":"integer","minimum":1})),
                ],
                vec!["path", "line"],
            ),
        ),
        tool(
            "find_definitions",
            "Find likely language-agnostic symbol definitions.",
            object_schema(vec![("symbol", json!({"type":"string"}))], vec!["symbol"]),
        ),
        tool(
            "find_references",
            "Find likely language-agnostic symbol references.",
            object_schema(vec![("symbol", json!({"type":"string"}))], vec!["symbol"]),
        ),
        tool(
            "final_verdict",
            "Return the final Koochi agentic test verdict.",
            object_schema(
                vec![
                    ("status", string_enum_schema(&["passed", "failed"])),
                    (
                        "severity",
                        string_enum_schema(&["low", "medium", "high", "critical"]),
                    ),
                    ("description", json!({"type":"string"})),
                    (
                        "evidence",
                        json!({
                            "type":"array",
                            "items": {
                                "type":"object",
                                "properties": {
                                    "path": {"type":"string"},
                                    "line": {"type":"integer", "minimum":1},
                                    "preview": {"type":"string"}
                                },
                                "required": ["path", "line", "preview"]
                            }
                        }),
                    ),
                ],
                vec!["status", "description"],
            ),
        ),
    ]
}

fn tool(
    name: &'static str,
    description: &'static str,
    parameters: serde_json::Value,
) -> OpenAiTool {
    OpenAiTool {
        kind: "function",
        function: OpenAiToolFunction {
            name,
            description,
            parameters,
        },
    }
}

fn object_schema(
    properties: Vec<(&'static str, serde_json::Value)>,
    required: Vec<&'static str>,
) -> serde_json::Value {
    let properties = properties
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect::<serde_json::Map<_, _>>();
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

fn string_enum_schema(values: &[&str]) -> serde_json::Value {
    json!({
        "type": "string",
        "enum": values,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Evidence;
    use crate::LlmResponse;
    use crate::Severity;
    use crate::TestStatus;

    #[test]
    fn omits_temperature_for_gpt_5_models() {
        assert_eq!(temperature_for_model("gpt-5-nano"), None);
        assert_eq!(temperature_for_model("gpt-5-mini"), None);
        assert_eq!(temperature_for_model("gpt-5.1"), None);
        assert_eq!(temperature_for_model("gpt-4o-mini"), Some(0.0));
    }

    #[test]
    fn parses_native_tool_call() {
        let action = parse_tool_call(
            OpenAiToolCall {
                function: OpenAiToolCallFunction {
                    name: "search_text".to_string(),
                    arguments: r#"{"query":"token","kind":"source"}"#.to_string(),
                },
            },
            None,
        )
        .unwrap();
        assert_eq!(
            action,
            LlmAction::Tool(LlmToolCall::SearchText {
                query: "token".to_string(),
                kind: Some("source".to_string()),
            })
        );
    }

    #[test]
    fn parses_native_review_hunks_tool_call() {
        let action = parse_tool_call(
            OpenAiToolCall {
                function: OpenAiToolCallFunction {
                    name: "list_review_hunks".to_string(),
                    arguments: r#"{}"#.to_string(),
                },
            },
            None,
        )
        .unwrap();
        assert_eq!(action, LlmAction::Tool(LlmToolCall::ListReviewHunks));
    }

    #[test]
    fn parses_native_hunk_context_tool_call() {
        let action = parse_tool_call(
            OpenAiToolCall {
                function: OpenAiToolCallFunction {
                    name: "get_hunk_context".to_string(),
                    arguments: r#"{"hunk_id":"src/lib.rs#1"}"#.to_string(),
                },
            },
            None,
        )
        .unwrap();
        assert_eq!(
            action,
            LlmAction::Tool(LlmToolCall::GetHunkContext {
                hunk_id: "src/lib.rs#1".to_string(),
            })
        );
    }

    #[test]
    fn parses_native_final_verdict_call() {
        let action = parse_tool_call(
            OpenAiToolCall {
                function: OpenAiToolCallFunction {
                    name: "final_verdict".to_string(),
                    arguments: r#"{"status":"failed","severity":"high","description":"bad thing","evidence":[{"path":"src/lib.rs","line":7,"preview":"bad();"}]}"#.to_string(),
                },
            },
            None,
        )
        .unwrap();
        assert_eq!(
            action,
            LlmAction::Final(LlmResponse {
                status: TestStatus::Failed,
                severity: Some(Severity::High),
                description: "bad thing".to_string(),
                evidence: vec![Evidence {
                    path: "src/lib.rs".to_string(),
                    line: 7,
                    preview: "bad();".to_string(),
                }],
            })
        );
    }

    #[test]
    fn parses_native_final_verdict_with_default_status() {
        let action = parse_tool_call(
            OpenAiToolCall {
                function: OpenAiToolCallFunction {
                    name: "final_verdict".to_string(),
                    arguments: r#"{"description":"safe marker found","severity":"low","evidence":[{"path":"src/workflows.rs","line":189,"preview":"// KOOCHI_SAFE_WORKFLOW_ROUTE_009"}]}"#.to_string(),
                },
            },
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
