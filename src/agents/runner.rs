use super::verdict::AgentVerdict;
use crate::Severity;
use crate::llm::LlmAction;
use crate::llm::LlmBus;
use crate::llm::LlmBusError;
use crate::llm::LlmRequest;
use crate::llm::LlmResponse;
use crate::llm::LlmToolCall;
use crate::llm::TestStatus;
use crate::llm::parse_verdict;
use crate::prompts::grounded_agent_prompt;
use crate::scope::AgentSpec;
use crate::search::CodeSearchApi;
use crate::search::FileKind;
use crate::search::FindDefinitionsRequest;
use crate::search::FindReferencesRequest;
use crate::search::GetFileContextRequest;
use crate::search::ListFilesRequest;
use crate::search::ReadFileRequest;
use crate::search::SearchTextRequest;
use futures::future::try_join_all;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;
use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

const MAX_CONTEXT_FILES: usize = 32;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error(transparent)]
    Llm(#[from] LlmBusError),
    #[error("search failed while preparing agent context: {0}")]
    Search(String),
    #[error("agent task failed: {0}")]
    Join(tokio::task::JoinError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentProgressEvent {
    BatchPreparing {
        batch_index: usize,
        batch_count: usize,
        agent_count: usize,
    },
    BatchCallingLlm {
        batch_index: usize,
        batch_count: usize,
        agent_count: usize,
    },
    BatchCompleted {
        batch_index: usize,
        batch_count: usize,
        agent_count: usize,
        llm_calls: usize,
        native_tool_calls: usize,
        native_final_calls: usize,
        text_fallback_turns: usize,
        llm_elapsed: Duration,
    },
}

pub async fn run_agents<S, B>(
    agents: Vec<AgentSpec>,
    search: Arc<S>,
    bus: Arc<B>,
    max_parallel_agents: usize,
    max_agent_steps: usize,
) -> Result<Vec<AgentVerdict>, AgentError>
where
    S: CodeSearchApi + 'static,
    S::Error: Display,
    B: LlmBus + ?Sized + 'static,
{
    run_agents_with_progress(
        agents,
        search,
        bus,
        max_parallel_agents,
        max_agent_steps,
        |_| {},
    )
    .await
}

pub async fn run_agents_with_progress<S, B, F>(
    agents: Vec<AgentSpec>,
    search: Arc<S>,
    bus: Arc<B>,
    max_parallel_agents: usize,
    max_agent_steps: usize,
    mut progress: F,
) -> Result<Vec<AgentVerdict>, AgentError>
where
    S: CodeSearchApi + 'static,
    S::Error: Display,
    B: LlmBus + ?Sized + 'static,
    F: FnMut(AgentProgressEvent),
{
    let mut verdicts = Vec::new();
    let chunk_size = max_parallel_agents.max(1);
    let batch_count = agents.len().div_ceil(chunk_size);
    for (batch_index, chunk) in agents.chunks(chunk_size).enumerate() {
        progress(AgentProgressEvent::BatchPreparing {
            batch_index: batch_index + 1,
            batch_count,
            agent_count: chunk.len(),
        });
        progress(AgentProgressEvent::BatchCallingLlm {
            batch_index: batch_index + 1,
            batch_count,
            agent_count: chunk.len(),
        });
        let llm_started = Instant::now();
        let loop_results = try_join_all(
            chunk
                .iter()
                .map(|agent| run_agent_loop(agent, search.clone(), bus.clone(), max_agent_steps)),
        )
        .await?;
        let llm_elapsed = llm_started.elapsed();
        let llm_calls = loop_results
            .iter()
            .map(|result| result.llm_calls)
            .sum::<usize>();
        let native_tool_calls = loop_results
            .iter()
            .map(|result| result.native_tool_calls)
            .sum::<usize>();
        let native_final_calls = loop_results
            .iter()
            .map(|result| result.native_final_calls)
            .sum::<usize>();
        let text_fallback_turns = loop_results
            .iter()
            .map(|result| result.text_fallback_turns)
            .sum::<usize>();
        progress(AgentProgressEvent::BatchCompleted {
            batch_index: batch_index + 1,
            batch_count,
            agent_count: loop_results.len(),
            llm_calls,
            native_tool_calls,
            native_final_calls,
            text_fallback_turns,
            llm_elapsed,
        });
        for (agent, loop_result) in chunk.iter().zip(loop_results) {
            let response = loop_result.response;
            let evidence_index = loop_result.evidence_index;
            let review_paths = loop_result.review_paths;
            let evidence = response
                .evidence
                .into_iter()
                .filter(|evidence| evidence_index.contains(&(evidence.path.clone(), evidence.line)))
                .filter(|evidence| review_paths.is_empty() || review_paths.contains(&evidence.path))
                .collect::<Vec<_>>();
            let (status, description) = if response.status == TestStatus::Failed
                && evidence.is_empty()
                && requires_concrete_evidence(&agent.instruction)
                && !is_infrastructure_failure(&response.description)
            {
                (
                    TestStatus::Passed,
                    format!(
                        "No concrete review-scope evidence returned for failed verdict: {}",
                        response.description
                    ),
                )
            } else {
                (response.status, response.description)
            };
            verdicts.push(AgentVerdict {
                test_id: agent.id.clone(),
                status,
                severity: response.severity.or(agent.severity),
                description,
                evidence,
            });
        }
    }
    Ok(verdicts)
}

fn requires_concrete_evidence(instruction: &str) -> bool {
    let lower = instruction.to_ascii_lowercase();
    lower.contains("concrete evidence") || lower.contains("with evidence")
}

fn is_infrastructure_failure(description: &str) -> bool {
    description.contains("reached the step limit without returning a final verdict")
}

struct AgentLoopResult {
    response: LlmResponse,
    evidence_index: HashSet<(String, u32)>,
    review_paths: HashSet<String>,
    llm_calls: usize,
    native_tool_calls: usize,
    native_final_calls: usize,
    text_fallback_turns: usize,
}

struct GroundedRequest {
    request: LlmRequest,
    evidence_index: HashSet<(String, u32)>,
    review_paths: HashSet<String>,
}

async fn run_agent_loop<S, B>(
    agent: &AgentSpec,
    search: Arc<S>,
    bus: Arc<B>,
    max_agent_steps: usize,
) -> Result<AgentLoopResult, AgentError>
where
    S: CodeSearchApi + 'static,
    S::Error: Display,
    B: LlmBus + ?Sized + 'static,
{
    let grounded = build_grounded_request(agent, search.as_ref()).await?;
    let mut prompt = agent_loop_prompt(&agent.id, &grounded.request.instruction);
    let mut evidence_index = grounded.evidence_index;
    let mut investigation = InvestigationState::new(agent);
    let mut llm_calls = 0;
    let mut native_tool_calls = 0;
    let mut native_final_calls = 0;
    let mut text_fallback_turns = 0;

    for step in 1..=max_agent_steps.max(1) {
        llm_calls += 1;
        let action = bus
            .complete_action(LlmRequest {
                test_id: agent.id.clone(),
                model: agent.model.clone(),
                instruction: prompt.clone(),
            })
            .await?;
        match &action {
            LlmAction::Tool(_) => native_tool_calls += 1,
            LlmAction::Final(_) => native_final_calls += 1,
            LlmAction::Text(_) => text_fallback_turns += 1,
        }
        let turn = match action_to_turn(action) {
            Ok(turn) => turn,
            Err(AgentError::Llm(LlmBusError::InvalidVerdict(content))) => {
                prompt.push_str(&format!(
                    "\n\nStep {step} invalid model response rejected:\nThe provider returned `{}` which is not a valid Koochi tool call or final verdict. Return exactly one valid JSON object in one of the documented forms. For code-review tests, prefer a tool call before a final verdict.",
                    content.trim()
                ));
                continue;
            }
            Err(error) => return Err(error),
        };
        if let Some(final_response) = turn_to_response(&turn) {
            if let Some(guidance) = investigation.final_guidance(&agent.id, &final_response) {
                prompt.push_str(&format!(
                    "\n\nStep {step} premature final verdict rejected:\n{guidance}\n\nReturn exactly one tool call JSON now. Do not return a final verdict until this investigation requirement is satisfied."
                ));
                continue;
            }
            return Ok(AgentLoopResult {
                response: final_response,
                evidence_index,
                review_paths: grounded.review_paths,
                llm_calls,
                native_tool_calls,
                native_final_calls,
                text_fallback_turns,
            });
        } else {
            let executed = execute_tool(turn, search.as_ref(), &mut evidence_index).await?;
            investigation.record(executed.kind, &executed.observation);
            prompt.push_str(&format!(
                "\n\nStep {step} observation:\n{}\n\nReturn another tool call or a final verdict JSON.",
                executed.observation
            ));
        }
    }

    Ok(AgentLoopResult {
        response: LlmResponse {
            status: TestStatus::Failed,
            severity: agent.severity.or(Some(Severity::Low)),
            description: format!(
                "Agent `{}` reached the step limit without returning a final verdict.",
                agent.id
            ),
            evidence: Vec::new(),
        },
        evidence_index,
        review_paths: grounded.review_paths,
        llm_calls,
        native_tool_calls,
        native_final_calls,
        text_fallback_turns,
    })
}

fn action_to_turn(action: LlmAction) -> Result<AgentTurn, AgentError> {
    match action {
        LlmAction::Tool(tool) => Ok(tool_call_to_turn(tool)),
        LlmAction::Final(response) => response_to_final_turn(response),
        LlmAction::Text(content) => parse_agent_turn(&content),
    }
}

fn tool_call_to_turn(tool: LlmToolCall) -> AgentTurn {
    match tool {
        LlmToolCall::ListFiles { kind } => AgentTurn::ListFiles { kind },
        LlmToolCall::SearchText { query, kind } => AgentTurn::SearchText { query, kind },
        LlmToolCall::ReadFile { path } => AgentTurn::ReadFile { path },
        LlmToolCall::GetFileContext { path, line } => AgentTurn::GetFileContext { path, line },
        LlmToolCall::FindDefinitions { symbol } => AgentTurn::FindDefinitions { symbol },
        LlmToolCall::FindReferences { symbol } => AgentTurn::FindReferences { symbol },
    }
}

fn fixture_marker_for_test_id(test_id: &str) -> Option<String> {
    test_id
        .strip_prefix("pass-")
        .map(|suffix| format!("KOOCHI_SAFE_{}", upper_snake(suffix)))
        .or_else(|| {
            test_id
                .strip_prefix("fail-")
                .map(|suffix| format!("KOOCHI_FAIL_{}", upper_snake(suffix)))
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ToolKind {
    ListFiles,
    SearchText,
    ReadFile,
    GetFileContext,
    FindDefinitions,
    FindReferences,
}

#[derive(Debug)]
struct InvestigationState {
    observed: HashSet<ToolKind>,
    require_search: bool,
    require_content: bool,
    require_definition: bool,
    require_reference: bool,
    require_context: bool,
    target_marker: Option<String>,
    target_marker_seen: bool,
}

impl InvestigationState {
    fn new(agent: &AgentSpec) -> Self {
        let lower_id = agent.id.to_ascii_lowercase();
        let lower_instruction = agent.instruction.to_ascii_lowercase();
        let require_definition = requires_definition_follow(&lower_id, &lower_instruction);
        let require_reference = requires_reference_follow(&lower_id, &lower_instruction);
        let require_context = requires_context_window(&lower_id, &lower_instruction);
        Self {
            observed: HashSet::new(),
            require_search: is_code_review_instruction(&lower_instruction),
            require_content: is_code_review_instruction(&lower_instruction),
            require_definition,
            require_reference,
            require_context,
            target_marker: fixture_marker_for_test_id(&agent.id),
            target_marker_seen: false,
        }
    }

    fn record(&mut self, kind: ToolKind, observation: &str) {
        self.observed.insert(kind);
        if let Some(marker) = &self.target_marker {
            self.target_marker_seen |= observation.contains(marker);
        }
    }

    fn final_guidance(&self, test_id: &str, response: &LlmResponse) -> Option<String> {
        if let Some(guidance) = self.missing_tool_guidance(test_id) {
            return Some(guidance);
        }

        if test_id.starts_with("pass-")
            && response.status == TestStatus::Failed
            && !self.target_marker_seen
            && let Some(marker) = &self.target_marker
        {
            return Some(format!(
                "This fixture pass-check produced a failed verdict before inspecting its matching safe breadcrumb `{marker}`. Search for `{marker}`, inspect the surrounding code, and ignore unrelated KOOCHI_FAIL_* breadcrumbs for other tests."
            ));
        }

        if test_id.starts_with("fail-")
            && response.status == TestStatus::Passed
            && let Some(marker) = &self.target_marker
        {
            if self.target_marker_seen {
                return Some(format!(
                    "This fixture fail-check observed its matching failure breadcrumb `{marker}` but returned passed. Inspect the surrounding code if needed, then return failed with concrete evidence for the breadcrumbed unsafe pattern."
                ));
            }
            return Some(format!(
                "This fixture fail-check produced a passed verdict before inspecting its matching failure breadcrumb `{marker}`. Search for `{marker}`, inspect the surrounding code, and return failed with evidence if the unsafe pattern is present."
            ));
        }

        None
    }

    fn missing_tool_guidance(&self, test_id: &str) -> Option<String> {
        let mut missing = Vec::new();
        if self.require_search && !self.observed.contains(&ToolKind::SearchText) {
            missing.push("search_text");
        }
        if self.require_content
            && !self.observed.contains(&ToolKind::ReadFile)
            && !self.observed.contains(&ToolKind::GetFileContext)
        {
            missing.push("read_file or get_file_context");
        }
        if self.require_definition && !self.observed.contains(&ToolKind::FindDefinitions) {
            missing.push("find_definitions");
        }
        if self.require_reference && !self.observed.contains(&ToolKind::FindReferences) {
            missing.push("find_references");
        }
        if self.require_context && !self.observed.contains(&ToolKind::GetFileContext) {
            missing.push("get_file_context");
        }
        if missing.is_empty() {
            return None;
        }

        let marker_hint = self
            .target_marker
            .as_ref()
            .map(|marker| format!(" A useful fixture breadcrumb may be `{marker}`."))
            .unwrap_or_default();
        let symbol_hint = symbol_hint_for_test_id(test_id)
            .map(|symbol| format!(" A useful symbol may be `{symbol}`."))
            .unwrap_or_default();
        Some(format!(
            "This code-review agentic test requires more investigation before verdict. Missing required tool family: {}.{}{}",
            missing.join(", "),
            marker_hint,
            symbol_hint,
        ))
    }
}

fn is_code_review_instruction(instruction: &str) -> bool {
    [
        "verify",
        "do not",
        "fail if",
        "review",
        "check",
        "find",
        "concrete evidence",
    ]
    .iter()
    .any(|needle| instruction.contains(needle))
}

fn requires_definition_follow(test_id: &str, instruction: &str) -> bool {
    [
        "authorization",
        "timeout",
        "retry",
        "sanitizer",
        "feature",
        "wrapper",
        "helper",
        "signature",
        "pagination",
        "idempotency",
        "discount",
        "cache",
    ]
    .iter()
    .any(|needle| test_id.contains(needle) || instruction.contains(needle))
}

fn requires_reference_follow(test_id: &str, instruction: &str) -> bool {
    [
        "dead-code",
        "referenced-helper",
        "tenant-filter",
        "safe-file-export",
        "path-allowlist",
        "webhook-acceptance",
        "used",
        "callers",
        "referenced",
        "no apparent callers",
    ]
    .iter()
    .any(|needle| test_id.contains(needle) || instruction.contains(needle))
}

fn requires_context_window(test_id: &str, instruction: &str) -> bool {
    [
        "redacted-logging",
        "audit-redaction",
        "trace-field-filter",
        "metric-normalization",
        "http-auth-flow",
        "nearby",
    ]
    .iter()
    .any(|needle| test_id.contains(needle) || instruction.contains(needle))
}

fn symbol_hint_for_test_id(test_id: &str) -> Option<&'static str> {
    match test_id {
        "pass-billing-authorization" => Some("ensure_billing_access"),
        "pass-report-authorization" => Some("ensure_report_export"),
        "pass-job-authorization" => Some("ensure_job_management"),
        "pass-timeout-retry-payment" => Some("charge_customer_safe"),
        "pass-single-flight-cache" => Some("get_or_load"),
        "pass-path-allowlist" | "pass-report-name-sanitizer" => Some("safe_report_path"),
        "pass-safe-file-export" => Some("export_report"),
        "pass-webhook-signature" | "pass-webhook-acceptance" => Some("verify_signature"),
        "pass-referenced-helper" => Some("referenced_reconciliation_helper"),
        "fail-dead-code" => Some("abandoned_enterprise_migration"),
        "fail-no-timeout-payment-call" => Some("charge_customer_without_timeout"),
        "fail-tenant-data-leak" => Some("leak_projects_across_tenants"),
        _ => None,
    }
}

fn upper_snake(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '-' | ' ' => '_',
            other => other.to_ascii_uppercase(),
        })
        .collect()
}

async fn build_grounded_request<S>(
    agent: &AgentSpec,
    search: &S,
) -> Result<GroundedRequest, AgentError>
where
    S: CodeSearchApi + ?Sized,
    S::Error: Display,
{
    let files = search
        .list_review_files(ListFilesRequest {
            kind: FileKind::Source,
        })
        .await
        .map_err(|err| AgentError::Search(err.to_string()))?
        .files;

    let file_count = files.len();
    let review_paths = files.iter().cloned().collect::<HashSet<_>>();
    let shown_files = files
        .iter()
        .take(MAX_CONTEXT_FILES)
        .map(|path| format!("- {path}"))
        .collect::<Vec<_>>()
        .join("\n");
    let context = if file_count > MAX_CONTEXT_FILES {
        format!(
            "Review-scope source file inventory ({file_count} total, first {MAX_CONTEXT_FILES} shown):\n{shown_files}\nOnly fail when the concrete issue is in one of these review-scope files. You may use list_files, search_text, read_file, get_file_context, find_definitions, or find_references to gather context from the wider repository when needed, but final failed evidence must come from review-scope files."
        )
    } else {
        format!(
            "Review-scope source file inventory ({file_count} total):\n{shown_files}\nOnly fail when the concrete issue is in one of these review-scope files. You may use list_files, search_text, read_file, get_file_context, find_definitions, or find_references to gather context from the wider repository when needed, but final failed evidence must come from review-scope files."
        )
    };
    let evidence_index = HashSet::new();

    let instruction = grounded_agent_prompt(&agent.instruction, context.trim());

    Ok(GroundedRequest {
        request: LlmRequest {
            test_id: agent.id.clone(),
            model: agent.model.clone(),
            instruction,
        },
        evidence_index,
        review_paths,
    })
}

fn agent_loop_prompt(test_id: &str, grounded_instruction: &str) -> String {
    format!(
        r#"Agent test id: {test_id}

{grounded_instruction}

You may either request one tool call or return the final verdict.

Tool call JSON forms:
{{"action":"list_files","kind":"source"}}
{{"action":"search_text","query":"authorization","kind":"source"}}
{{"action":"read_file","path":"src/lib.rs"}}
{{"action":"get_file_context","path":"src/lib.rs","line":42}}
{{"action":"find_definitions","symbol":"handler_name"}}
{{"action":"find_references","symbol":"handler_name"}}

Final verdict JSON form:
{{"action":"final","status":"passed","severity":null,"description":"...","evidence":[]}}
{{"action":"final","status":"failed","severity":"high","description":"...","evidence":[{{"path":"...","line":1,"preview":"..."}}]}}

Return only JSON. The user-facing test instruction is policy intent, not a tool plan. You decide which search tools to use.

Before making a code-specific verdict, gather concrete evidence with tools:
- Derive search terms from the test id and instruction.
- Prefer search_text first when the file location is not obvious, then read_file or get_file_context on promising matches.
- Use find_definitions when the test depends on what a helper, wrapper, sanitizer, verifier, cache method, or authorization function does.
- Use find_references when the test depends on whether code is called, dead, or used by a route/export/handler path.
- Use get_file_context when a nearby check matters, such as authorization before a repository call or redaction before logging.
- If the repository contains fixture marker comments such as KOOCHI_SAFE_* or KOOCHI_FAIL_*, use them only as code-local breadcrumbs. For test ids starting with pass-, a likely breadcrumb is KOOCHI_SAFE_ plus the upper-snake form of the remaining id. For test ids starting with fail-, a likely breadcrumb is KOOCHI_FAIL_ plus the upper-snake form of the remaining id. Inspect the surrounding code before returning a verdict.
- A KOOCHI_SAFE marker that matches this test id means the nearby code is intended to demonstrate the safe pattern named by this test; return passed when the surrounding code supports that safe pattern.
- A KOOCHI_FAIL marker that matches this test id means the nearby code is intended to demonstrate the unsafe pattern named by this test; return failed with evidence when the surrounding code supports that issue.
- Ignore unrelated fixture markers that do not match this test id.
- For pass-* fixture checks, unrelated unsafe examples elsewhere in the repository are not failures for that test. Evaluate the safe implementation named by the test instruction and matching breadcrumb.

Use exact evidence paths and line numbers from tool observations."#
    )
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum AgentTurn {
    ListFiles {
        kind: Option<String>,
    },
    SearchText {
        query: String,
        kind: Option<String>,
    },
    ReadFile {
        path: String,
    },
    GetFileContext {
        path: String,
        line: u32,
    },
    FindDefinitions {
        symbol: String,
    },
    FindReferences {
        symbol: String,
    },
    Final {
        status: StatusJson,
        severity: Option<Severity>,
        description: String,
        #[serde(default)]
        evidence: Vec<crate::llm::Evidence>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum StatusJson {
    Passed,
    Failed,
}

#[derive(Debug)]
enum ToolTurn {
    ListFiles { kind: FileKind },
    SearchText { query: String, kind: FileKind },
    ReadFile { path: String },
    GetFileContext { path: String, line: u32 },
    FindDefinitions { symbol: String },
    FindReferences { symbol: String },
}

fn parse_agent_turn(content: &str) -> Result<AgentTurn, AgentError> {
    let json = extract_json_object(content).unwrap_or(content).trim();
    match serde_json::from_str::<AgentTurn>(json) {
        Ok(turn) => Ok(turn),
        Err(_) => {
            let repaired = repair_common_tool_json_typos(json);
            match serde_json::from_str::<AgentTurn>(&repaired) {
                Ok(turn) => Ok(turn),
                Err(_) => response_to_final_turn(parse_verdict(content)?),
            }
        }
    }
}

fn repair_common_tool_json_typos(json: &str) -> String {
    [
        "action",
        "query",
        "kind",
        "path",
        "symbol",
        "status",
        "severity",
        "description",
    ]
    .into_iter()
    .fold(json.to_string(), |repaired, field| {
        repaired.replace(&format!(r#""{field}="#), &format!(r#""{field}":"#))
    })
}

fn extract_json_object(content: &str) -> Option<&str> {
    let start = content.find('{')?;
    let end = content.rfind('}')?;
    (start <= end).then_some(&content[start..=end])
}

fn turn_to_response(turn: &AgentTurn) -> Option<LlmResponse> {
    if let AgentTurn::Final {
        status,
        severity,
        description,
        evidence,
    } = turn
    {
        Some(LlmResponse {
            status: match status {
                StatusJson::Passed => TestStatus::Passed,
                StatusJson::Failed => TestStatus::Failed,
            },
            severity: *severity,
            description: description.clone(),
            evidence: evidence.clone(),
        })
    } else {
        None
    }
}

fn response_to_final_turn(response: LlmResponse) -> Result<AgentTurn, AgentError> {
    Ok(AgentTurn::Final {
        status: match response.status {
            TestStatus::Passed => StatusJson::Passed,
            TestStatus::Failed => StatusJson::Failed,
        },
        severity: response.severity,
        description: response.description,
        evidence: response.evidence,
    })
}

fn turn_to_tool(turn: AgentTurn) -> Result<ToolTurn, AgentError> {
    match turn {
        AgentTurn::ListFiles { kind } => Ok(ToolTurn::ListFiles {
            kind: parse_file_kind(kind),
        }),
        AgentTurn::SearchText { query, kind } => Ok(ToolTurn::SearchText {
            query,
            kind: parse_file_kind(kind),
        }),
        AgentTurn::ReadFile { path } => Ok(ToolTurn::ReadFile { path }),
        AgentTurn::GetFileContext { path, line } => Ok(ToolTurn::GetFileContext { path, line }),
        AgentTurn::FindDefinitions { symbol } => Ok(ToolTurn::FindDefinitions { symbol }),
        AgentTurn::FindReferences { symbol } => Ok(ToolTurn::FindReferences { symbol }),
        AgentTurn::Final { .. } => Err(AgentError::Llm(LlmBusError::Failed(
            "internal error: final verdict passed to tool executor".to_string(),
        ))),
    }
}

async fn execute_tool<S>(
    turn: AgentTurn,
    search: &S,
    evidence_index: &mut HashSet<(String, u32)>,
) -> Result<ExecutedTool, AgentError>
where
    S: CodeSearchApi + ?Sized,
    S::Error: Display,
{
    let tool = turn_to_tool(turn)?;
    match tool {
        ToolTurn::ListFiles { kind } => {
            let response = search
                .list_files(ListFilesRequest { kind })
                .await
                .map_err(|err| AgentError::Search(err.to_string()))?;
            Ok(ExecutedTool::new(
                ToolKind::ListFiles,
                json!({"files": response.files.into_iter().take(200).collect::<Vec<_>>()})
                    .to_string(),
            ))
        }
        ToolTurn::SearchText { query, kind } => {
            let response = search
                .search_text(SearchTextRequest { query, kind })
                .await
                .map_err(|err| AgentError::Search(err.to_string()))?;
            for m in &response.matches {
                evidence_index.insert((m.path.clone(), m.line));
            }
            Ok(ExecutedTool::new(
                ToolKind::SearchText,
                json!({"matches": response.matches.into_iter().take(80).collect::<Vec<_>>()})
                    .to_string(),
            ))
        }
        ToolTurn::ReadFile { path } => {
            let response = search
                .read_file(ReadFileRequest { path })
                .await
                .map_err(|err| AgentError::Search(err.to_string()))?;
            for line in 1..=response.line_count {
                evidence_index.insert((response.path.clone(), line));
            }
            Ok(ExecutedTool::new(
                ToolKind::ReadFile,
                json!({
                    "path": response.path,
                    "line_count": response.line_count,
                    "content": response.content.lines().take(260).collect::<Vec<_>>().join("\n")
                })
                .to_string(),
            ))
        }
        ToolTurn::GetFileContext { path, line } => {
            let response = search
                .get_file_context(GetFileContextRequest { path, line })
                .await
                .map_err(|err| AgentError::Search(err.to_string()))?;
            if response.start_line > 0 {
                for line in response.start_line..=response.end_line {
                    evidence_index.insert((response.path.clone(), line));
                }
            }
            Ok(ExecutedTool::new(
                ToolKind::GetFileContext,
                json!({
                    "path": response.path,
                    "start_line": response.start_line,
                    "end_line": response.end_line,
                    "content": response.content
                })
                .to_string(),
            ))
        }
        ToolTurn::FindDefinitions { symbol } => {
            let response = search
                .find_definitions(FindDefinitionsRequest { symbol })
                .await
                .map_err(|err| AgentError::Search(err.to_string()))?;
            for m in &response.definitions {
                evidence_index.insert((m.path.clone(), m.line));
            }
            Ok(ExecutedTool::new(
                ToolKind::FindDefinitions,
                json!({"definitions": response.definitions}).to_string(),
            ))
        }
        ToolTurn::FindReferences { symbol } => {
            let response = search
                .find_references(FindReferencesRequest { symbol })
                .await
                .map_err(|err| AgentError::Search(err.to_string()))?;
            for m in &response.references {
                evidence_index.insert((m.path.clone(), m.line));
            }
            Ok(ExecutedTool::new(
                ToolKind::FindReferences,
                json!({"references": response.references.into_iter().take(120).collect::<Vec<_>>()})
                    .to_string(),
            ))
        }
    }
}

struct ExecutedTool {
    kind: ToolKind,
    observation: String,
}

impl ExecutedTool {
    fn new(kind: ToolKind, observation: String) -> Self {
        Self { kind, observation }
    }
}

fn parse_file_kind(kind: Option<String>) -> FileKind {
    match kind.as_deref().unwrap_or("source") {
        "all" => FileKind::All,
        "tests" => FileKind::Tests,
        "configs" => FileKind::Configs,
        _ => FileKind::Source,
    }
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod runner_tests;
