use super::bus::LlmBus;
use super::bus::LlmBusError;
use super::types::Evidence;
use super::types::LlmAction;
use super::types::LlmRequest;
use super::types::LlmResponse;
use super::types::LlmTextResponse;
use super::types::LlmToolCall;
use super::types::TestStatus;
use crate::Severity;
use crate::config::KoochiConfig;
use crate::prompts::verdict_system_prompt;
use async_trait::async_trait;
use reqwest::header::CONTENT_TYPE;
use reqwest::header::HeaderMap;
use reqwest::header::HeaderValue;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Clone)]
pub struct AnthropicBus {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
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
        })
    }
}

#[async_trait]
impl LlmBus for AnthropicBus {
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
                model: request.model,
                max_tokens: 1024,
                system: verdict_system_prompt(),
                messages: vec![AnthropicMessage {
                    role: "user",
                    content: request.instruction,
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
                model: request.model,
                max_tokens: 1024,
                system: verdict_system_prompt(),
                messages: vec![AnthropicMessage {
                    role: "user",
                    content: request.instruction,
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
        for content in parsed.content {
            match content {
                AnthropicContent::ToolUse { name, input, .. } => {
                    return parse_tool_use(&name, input);
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

fn parse_tool_use(name: &str, input: serde_json::Value) -> Result<LlmAction, LlmBusError> {
    match name {
        "list_files" => {
            let args: KindArgs = parse_args(input)?;
            Ok(LlmAction::Tool(LlmToolCall::ListFiles { kind: args.kind }))
        }
        "search_text" => {
            let args: SearchTextArgs = parse_args(input)?;
            Ok(LlmAction::Tool(LlmToolCall::SearchText {
                query: args.query,
                kind: args.kind,
            }))
        }
        "read_file" => {
            let args: PathArgs = parse_args(input)?;
            Ok(LlmAction::Tool(LlmToolCall::ReadFile { path: args.path }))
        }
        "get_file_context" => {
            let args: ContextArgs = parse_args(input)?;
            Ok(LlmAction::Tool(LlmToolCall::GetFileContext {
                path: args.path,
                line: args.line,
            }))
        }
        "find_definitions" => {
            let args: SymbolArgs = parse_args(input)?;
            Ok(LlmAction::Tool(LlmToolCall::FindDefinitions {
                symbol: args.symbol,
            }))
        }
        "find_references" => {
            let args: SymbolArgs = parse_args(input)?;
            Ok(LlmAction::Tool(LlmToolCall::FindReferences {
                symbol: args.symbol,
            }))
        }
        "final_verdict" => {
            let args: FinalVerdictArgs = parse_args(input)?;
            Ok(LlmAction::Final(LlmResponse {
                status: args.status,
                severity: args.severity,
                description: args.description,
                evidence: args.evidence,
            }))
        }
        _ => Err(LlmBusError::InvalidVerdict(format!(
            "unsupported Anthropic tool use `{name}`"
        ))),
    }
}

fn parse_args<T>(input: serde_json::Value) -> Result<T, LlmBusError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(input).map_err(|error| LlmBusError::InvalidVerdict(error.to_string()))
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
struct ContextArgs {
    path: String,
    line: u32,
}

#[derive(Debug, Deserialize)]
struct SymbolArgs {
    symbol: String,
}

#[derive(Debug, Deserialize)]
struct FinalVerdictArgs {
    status: TestStatus,
    severity: Option<Severity>,
    description: String,
    #[serde(default)]
    evidence: Vec<Evidence>,
}

fn tool_definitions() -> Vec<AnthropicTool> {
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
    input_schema: serde_json::Value,
) -> AnthropicTool {
    AnthropicTool {
        name,
        description,
        input_schema,
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

    #[test]
    fn parses_native_tool_use() {
        let action = parse_tool_use(
            "find_references",
            json!({
                "symbol": "dangerous_sink"
            }),
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
    fn parses_native_final_verdict_tool_use() {
        let action = parse_tool_use(
            "final_verdict",
            json!({
                "status": "failed",
                "severity": "medium",
                "description": "bad thing",
                "evidence": [{"path": "src/lib.rs", "line": 3, "preview": "bad();"}]
            }),
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
}
