use super::bus::LlmBusError;
use super::types::LlmAction;
use super::types::LlmToolCall;
use super::types::TestStatus;
use super::verdict_parser::parse_verdict_with_default_status;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Clone)]
pub(crate) struct ToolSpec {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) schema: serde_json::Value,
}

pub(crate) fn default_status_for_test_id(test_id: &str) -> Option<TestStatus> {
    if test_id.starts_with("pass-") {
        Some(TestStatus::Passed)
    } else if test_id.starts_with("fail-") {
        Some(TestStatus::Failed)
    } else {
        None
    }
}

pub(crate) fn parse_tool_action_from_json_str(
    name: &str,
    arguments: &str,
    default_status: Option<TestStatus>,
) -> Result<LlmAction, LlmBusError> {
    if name == "final_verdict" {
        let response = parse_verdict_with_default_status(arguments, default_status)?;
        return Ok(LlmAction::Final(response));
    }
    let input = serde_json::from_str(arguments)
        .map_err(|_| LlmBusError::InvalidVerdict(arguments.to_string()))?;
    parse_tool_action_from_value(name, input, default_status)
}

pub(crate) fn parse_tool_action_from_value(
    name: &str,
    input: serde_json::Value,
    default_status: Option<TestStatus>,
) -> Result<LlmAction, LlmBusError> {
    match name {
        "list_files" => {
            let args: KindArgs = parse_args(input)?;
            Ok(LlmAction::Tool(LlmToolCall::ListFiles { kind: args.kind }))
        }
        "list_review_hunks" => Ok(LlmAction::Tool(LlmToolCall::ListReviewHunks)),
        "get_hunk_context" => {
            let args: HunkIdArgs = parse_args(input)?;
            Ok(LlmAction::Tool(LlmToolCall::GetHunkContext {
                hunk_id: args.hunk_id,
            }))
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
            let response = parse_verdict_with_default_status(&input.to_string(), default_status)?;
            Ok(LlmAction::Final(response))
        }
        _ => Err(LlmBusError::InvalidVerdict(format!(
            "unsupported LLM tool call `{name}`"
        ))),
    }
}

pub(crate) fn tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "list_files",
            description: "List repo files by kind.",
            schema: object_schema(
                vec![(
                    "kind",
                    string_enum_schema(&["source", "tests", "configs", "all"]),
                )],
                vec![],
            ),
        },
        ToolSpec {
            name: "list_review_hunks",
            description: "List changed review hunks with exact changed line numbers.",
            schema: object_schema(vec![], vec![]),
        },
        ToolSpec {
            name: "get_hunk_context",
            description: "Read bounded surrounding code for a specific changed review hunk id.",
            schema: object_schema(vec![("hunk_id", json!({"type":"string"}))], vec!["hunk_id"]),
        },
        ToolSpec {
            name: "search_text",
            description: "Search source text literally.",
            schema: object_schema(
                vec![
                    ("query", json!({"type":"string"})),
                    (
                        "kind",
                        string_enum_schema(&["source", "tests", "configs", "all"]),
                    ),
                ],
                vec!["query"],
            ),
        },
        ToolSpec {
            name: "read_file",
            description: "Read a complete repo-relative file.",
            schema: object_schema(vec![("path", json!({"type":"string"}))], vec!["path"]),
        },
        ToolSpec {
            name: "get_file_context",
            description: "Read a fixed-radius context window around a line.",
            schema: object_schema(
                vec![
                    ("path", json!({"type":"string"})),
                    ("line", json!({"type":"integer","minimum":1})),
                ],
                vec!["path", "line"],
            ),
        },
        ToolSpec {
            name: "find_definitions",
            description: "Find likely language-agnostic symbol definitions.",
            schema: object_schema(vec![("symbol", json!({"type":"string"}))], vec!["symbol"]),
        },
        ToolSpec {
            name: "find_references",
            description: "Find likely language-agnostic symbol references.",
            schema: object_schema(vec![("symbol", json!({"type":"string"}))], vec!["symbol"]),
        },
        ToolSpec {
            name: "final_verdict",
            description: "Return the final Koochi agentic invariant verdict.",
            schema: object_schema(
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
        },
    ]
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
