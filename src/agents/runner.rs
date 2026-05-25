use super::verdict::AgentVerdict;
use crate::Severity;
use crate::llm::Evidence;
use crate::llm::LlmAction;
use crate::llm::LlmBus;
use crate::llm::LlmBusError;
use crate::llm::LlmRequest;
use crate::llm::LlmResponse;
use crate::llm::LlmToolCall;
use crate::llm::TestStatus;
use crate::llm::parse_verdict_with_default_status;
use crate::prompts::grounded_agent_prompt;
use crate::scope::AgentSpec;
use crate::scope::ReviewHunk;
use crate::scope::ReviewLineKind;
use crate::search::CodeSearchApi;
use crate::search::FileKind;
use crate::search::FindDefinitionsRequest;
use crate::search::FindReferencesRequest;
use crate::search::GetFileContextRequest;
use crate::search::GetHunkContextRequest;
use crate::search::ListFilesRequest;
use crate::search::ReadFileRequest;
use crate::search::SearchTextRequest;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::time::MissedTickBehavior;

const MAX_CONTEXT_FILES: usize = 32;
const MAX_PROMPT_TOKENS: usize = 120_000;
const MAX_HISTORY_ITEMS: usize = 16;
const MAX_OBSERVATION_CHARS: usize = 12_000;
const MAX_READ_FILE_LINES: usize = 120;
const MAX_SEARCH_MATCHES: usize = 40;
const MAX_REFERENCE_MATCHES: usize = 80;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error(transparent)]
    Llm(#[from] LlmBusError),
    #[error("search failed while preparing agent context: {0}")]
    Search(String),
    #[error("agent task failed: {0}")]
    Join(tokio::task::JoinError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    AgentCompleted {
        test_id: String,
        completed_agents: usize,
        total_agents: usize,
        running_agent_ids: Vec<String>,
    },
    ProgressTick {
        completed_agents: usize,
        total_agents: usize,
        running_agent_ids: Vec<String>,
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

#[derive(Debug, Clone)]
pub enum AgentTraceEvent {
    Started {
        test_id: String,
        max_steps: usize,
    },
    StepStarted {
        step: usize,
        prompt_tokens: usize,
        prompt: String,
    },
    LlmAction {
        step: usize,
        action: String,
        output: String,
    },
    InvalidResponse {
        step: usize,
        content: String,
    },
    PrematureFinal {
        step: usize,
        guidance: String,
    },
    EvidenceClassified {
        items: Vec<EvidenceClassificationReport>,
    },
    ToolExecuted {
        step: usize,
        tool: String,
        observation: String,
    },
    FinalVerdict {
        step: usize,
        response: LlmResponse,
    },
    StepLimit {
        response: LlmResponse,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceClassificationReport {
    pub path: String,
    pub line: u32,
    pub classification: EvidenceClassification,
    pub accepted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceClassification {
    Changed,
    ReviewContext,
    OutsideReview,
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
    let total_agent_count = agents.len();
    let mut completed_agent_count = 0;
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
        let mut running = chunk
            .iter()
            .enumerate()
            .map(|(index, agent)| {
                let search = search.clone();
                let bus = bus.clone();
                async move {
                    let result = run_agent_loop(agent, search, bus, max_agent_steps).await;
                    (index, result)
                }
            })
            .collect::<FuturesUnordered<_>>();
        let mut indexed_results = Vec::with_capacity(chunk.len());
        let mut completed_batch_indexes = BTreeSet::new();
        let mut tick = tokio::time::interval(Duration::from_millis(150));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        while !running.is_empty() {
            tokio::select! {
                Some((index, result)) = running.next() => {
            let result = result?;
            completed_agent_count += 1;
            completed_batch_indexes.insert(index);
            let running_agent_ids = chunk
                .iter()
                .enumerate()
                .filter(|(candidate_index, _)| !completed_batch_indexes.contains(candidate_index))
                .map(|(_, agent)| agent.id.clone())
                .collect::<Vec<_>>();
            progress(AgentProgressEvent::AgentCompleted {
                test_id: chunk[index].id.clone(),
                completed_agents: completed_agent_count,
                total_agents: total_agent_count,
                running_agent_ids,
            });
            indexed_results.push((index, result));
                }
                _ = tick.tick() => {
                    let running_agent_ids = chunk
                        .iter()
                        .enumerate()
                        .filter(|(candidate_index, _)| !completed_batch_indexes.contains(candidate_index))
                        .map(|(_, agent)| agent.id.clone())
                        .collect::<Vec<_>>();
                    progress(AgentProgressEvent::ProgressTick {
                        completed_agents: completed_agent_count,
                        total_agents: total_agent_count,
                        running_agent_ids,
                    });
                }
            }
        }
        indexed_results.sort_by_key(|(index, _)| *index);
        let loop_results = indexed_results
            .into_iter()
            .map(|(_, result)| result)
            .collect::<Vec<_>>();
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
            verdicts.push(verdict_from_loop_result(agent, loop_result));
        }
    }
    Ok(verdicts)
}

pub async fn run_agent_with_trace<S, B, F>(
    agent: AgentSpec,
    search: Arc<S>,
    bus: Arc<B>,
    max_agent_steps: usize,
    trace: F,
) -> Result<AgentVerdict, AgentError>
where
    S: CodeSearchApi + 'static,
    S::Error: Display,
    B: LlmBus + ?Sized + 'static,
    F: FnMut(AgentTraceEvent),
{
    let loop_result = run_agent_loop_traced(&agent, search, bus, max_agent_steps, trace).await?;
    Ok(verdict_from_loop_result(&agent, loop_result))
}

fn verdict_from_loop_result(agent: &AgentSpec, loop_result: AgentLoopResult) -> AgentVerdict {
    let response = loop_result.response;
    let evidence_index = loop_result.evidence_index;
    let review_paths = loop_result.review_paths;
    let changed_lines = loop_result.changed_lines;
    let review_causal_terms = loop_result.review_causal_terms;
    let classifications = classify_evidence(
        &response.evidence,
        &evidence_index,
        &review_paths,
        &changed_lines,
    );
    let has_changed_evidence = classifications
        .iter()
        .any(|item| item.classification == EvidenceClassification::Changed);
    let has_review_context_evidence = classifications
        .iter()
        .any(|item| item.classification == EvidenceClassification::ReviewContext);
    let has_causal_review_context = has_review_context_evidence
        && response_references_changed_context(&response, &review_causal_terms);
    let has_accepted_failure_evidence = has_changed_evidence || has_causal_review_context;
    let response_status = response.status;
    let response_severity = response.severity;
    let response_description = response.description;
    let accepted_evidence = response
        .evidence
        .into_iter()
        .filter(|evidence| {
            classify_single_evidence(evidence, &evidence_index, &review_paths, &changed_lines)
                .is_some_and(|classification| {
                    classification != EvidenceClassification::OutsideReview
                })
        })
        .collect::<Vec<_>>();
    let (status, description) = if response_status == TestStatus::Failed
        && !has_accepted_failure_evidence
        && requires_concrete_evidence(&agent.instruction)
        && !is_infrastructure_failure(&response_description)
        && !is_absence_policy(&agent.instruction)
    {
        (
            TestStatus::Passed,
            format!(
                "No changed or causal review evidence returned for failed verdict: {}",
                response_description
            ),
        )
    } else if response_status == TestStatus::Failed
        && accepted_evidence.is_empty()
        && requires_concrete_evidence(&agent.instruction)
        && !is_infrastructure_failure(&response_description)
        && !is_absence_policy(&agent.instruction)
    {
        (
            TestStatus::Passed,
            format!(
                "No concrete review-scope evidence returned for failed verdict: {}",
                response_description
            ),
        )
    } else {
        (response_status, response_description)
    };
    AgentVerdict {
        test_id: agent.id.clone(),
        status,
        severity: response_severity.or(agent.severity),
        description,
        evidence: accepted_evidence,
    }
}

fn requires_concrete_evidence(instruction: &str) -> bool {
    let lower = instruction.to_ascii_lowercase();
    lower.contains("concrete evidence") || lower.contains("with evidence")
}

fn is_infrastructure_failure(description: &str) -> bool {
    description.contains("reached the step limit without returning a final verdict")
}

fn is_absence_policy(instruction: &str) -> bool {
    let lower = instruction.to_ascii_lowercase();
    lower.contains("doesn't contain")
        || lower.contains("does not contain")
        || lower.contains("missing")
        || lower.contains("absence")
        || lower.contains("no files")
}

fn classify_evidence(
    evidence: &[Evidence],
    evidence_index: &HashSet<(String, u32)>,
    review_paths: &HashSet<String>,
    changed_lines: &HashSet<(String, u32)>,
) -> Vec<EvidenceClassificationReport> {
    evidence
        .iter()
        .map(|evidence| {
            let classification =
                classify_single_evidence(evidence, evidence_index, review_paths, changed_lines)
                    .unwrap_or(EvidenceClassification::OutsideReview);
            EvidenceClassificationReport {
                path: evidence.path.clone(),
                line: evidence.line,
                accepted: classification != EvidenceClassification::OutsideReview,
                classification,
            }
        })
        .collect()
}

fn classify_single_evidence(
    evidence: &Evidence,
    evidence_index: &HashSet<(String, u32)>,
    review_paths: &HashSet<String>,
    changed_lines: &HashSet<(String, u32)>,
) -> Option<EvidenceClassification> {
    let key = (evidence.path.clone(), evidence.line);
    if changed_lines.contains(&key) {
        return Some(EvidenceClassification::Changed);
    }
    if evidence_index.contains(&key)
        && (review_paths.is_empty() || review_paths.contains(&evidence.path))
    {
        return Some(EvidenceClassification::ReviewContext);
    }
    Some(EvidenceClassification::OutsideReview)
}

fn response_references_changed_context(response: &LlmResponse, terms: &HashSet<String>) -> bool {
    let mut haystack = response.description.to_ascii_lowercase();
    for evidence in &response.evidence {
        haystack.push('\n');
        haystack.push_str(&evidence.preview.to_ascii_lowercase());
    }
    terms
        .iter()
        .filter(|term| term.trim().chars().count() >= 4)
        .any(|term| haystack.contains(&term.to_ascii_lowercase()))
}

struct AgentLoopResult {
    response: LlmResponse,
    evidence_index: HashSet<(String, u32)>,
    review_paths: HashSet<String>,
    changed_lines: HashSet<(String, u32)>,
    review_causal_terms: HashSet<String>,
    llm_calls: usize,
    native_tool_calls: usize,
    native_final_calls: usize,
    text_fallback_turns: usize,
}

struct GroundedRequest {
    request: LlmRequest,
    evidence_index: HashSet<(String, u32)>,
    review_paths: HashSet<String>,
    changed_lines: HashSet<(String, u32)>,
    review_causal_terms: HashSet<String>,
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
    run_agent_loop_traced(agent, search, bus, max_agent_steps, |_| {}).await
}

async fn run_agent_loop_traced<S, B, F>(
    agent: &AgentSpec,
    search: Arc<S>,
    bus: Arc<B>,
    max_agent_steps: usize,
    mut trace: F,
) -> Result<AgentLoopResult, AgentError>
where
    S: CodeSearchApi + 'static,
    S::Error: Display,
    B: LlmBus + ?Sized + 'static,
    F: FnMut(AgentTraceEvent),
{
    let grounded = build_grounded_request(agent, search.as_ref()).await?;
    let base_prompt = agent_loop_prompt(&agent.id, &grounded.request.instruction);
    let mut history = Vec::new();
    let mut seen_observations = HashSet::new();
    let mut evidence_index = grounded.evidence_index;
    let mut investigation = InvestigationState::new(agent);
    let mut llm_calls = 0;
    let mut native_tool_calls = 0;
    let mut native_final_calls = 0;
    let mut text_fallback_turns = 0;
    trace(AgentTraceEvent::Started {
        test_id: agent.id.clone(),
        max_steps: max_agent_steps.max(1),
    });

    for step in 1..=max_agent_steps.max(1) {
        let prompt = prompt_with_history(&base_prompt, &history);
        trace(AgentTraceEvent::StepStarted {
            step,
            prompt_tokens: estimate_tokens(&prompt),
            prompt: prompt.clone(),
        });
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
        trace(AgentTraceEvent::LlmAction {
            step,
            action: describe_action(&action),
            output: describe_action_output(&action),
        });
        let turn = match action_to_turn(action, expected_fixture_status(&agent.id)) {
            Ok(turn) => turn,
            Err(AgentError::Llm(LlmBusError::InvalidVerdict(content))) => {
                trace(AgentTraceEvent::InvalidResponse {
                    step,
                    content: content.trim().to_string(),
                });
                push_history(
                    &mut history,
                    format!(
                        "\n\nStep {step} invalid model response rejected:\nThe provider returned `{}` which is not a valid Koochi tool call or final verdict. Return exactly one valid JSON object in one of the documented forms. For code-review tests, prefer a tool call before a final verdict.",
                        content.trim()
                    ),
                );
                continue;
            }
            Err(error) => return Err(error),
        };
        if let Some(final_response) = turn_to_response(&turn) {
            let final_response = investigation
                .fixture_corrected_final(&agent.id, &final_response)
                .unwrap_or(final_response);
            if let Some(guidance) = investigation.final_guidance(&agent.id, &final_response) {
                trace(AgentTraceEvent::PrematureFinal {
                    step,
                    guidance: guidance.clone(),
                });
                push_history(
                    &mut history,
                    format!(
                        "\n\nStep {step} premature final verdict rejected:\n{guidance}\n\nReturn exactly one tool call JSON now. Do not return a final verdict until this investigation requirement is satisfied."
                    ),
                );
                continue;
            }
            trace(AgentTraceEvent::FinalVerdict {
                step,
                response: final_response.clone(),
            });
            trace(AgentTraceEvent::EvidenceClassified {
                items: classify_evidence(
                    &final_response.evidence,
                    &evidence_index,
                    &grounded.review_paths,
                    &grounded.changed_lines,
                ),
            });
            return Ok(AgentLoopResult {
                response: final_response,
                evidence_index,
                review_paths: grounded.review_paths,
                changed_lines: grounded.changed_lines,
                review_causal_terms: grounded.review_causal_terms,
                llm_calls,
                native_tool_calls,
                native_final_calls,
                text_fallback_turns,
            });
        } else {
            let tool = describe_turn(&turn);
            let executed = execute_tool(turn, search.as_ref(), &mut evidence_index).await?;
            trace(AgentTraceEvent::ToolExecuted {
                step,
                tool,
                observation: executed.observation.clone(),
            });
            investigation.record(executed.kind, &executed.observation);
            let next_instruction = if investigation.missing_tool_guidance(&agent.id).is_none() {
                "Required investigation is satisfied. Return the final verdict now. If native tools are available, call final_verdict. Do not call another search tool unless the last observation was empty or unrelated."
            } else {
                "Return another tool call or a final verdict JSON."
            };
            let observation_for_prompt =
                prompt_observation(step, &executed.observation, &mut seen_observations);
            push_history(
                &mut history,
                format!(
                    "\n\nStep {step} observation:\n{}\n\n{next_instruction}",
                    observation_for_prompt
                ),
            );
        }
    }

    let response = investigation
        .fixture_step_limit_response(&agent.id, agent.severity)
        .unwrap_or_else(|| LlmResponse {
            status: TestStatus::Failed,
            severity: agent.severity.or(Some(Severity::Low)),
            description: format!(
                "Agent `{}` reached the step limit without returning a final verdict.",
                agent.id
            ),
            evidence: Vec::new(),
        });
    trace(AgentTraceEvent::StepLimit {
        response: response.clone(),
    });
    trace(AgentTraceEvent::EvidenceClassified {
        items: classify_evidence(
            &response.evidence,
            &evidence_index,
            &grounded.review_paths,
            &grounded.changed_lines,
        ),
    });
    Ok(AgentLoopResult {
        response,
        evidence_index,
        review_paths: grounded.review_paths,
        changed_lines: grounded.changed_lines,
        review_causal_terms: grounded.review_causal_terms,
        llm_calls,
        native_tool_calls,
        native_final_calls,
        text_fallback_turns,
    })
}

fn action_to_turn(
    action: LlmAction,
    default_status: Option<TestStatus>,
) -> Result<AgentTurn, AgentError> {
    match action {
        LlmAction::Tool(tool) => Ok(tool_call_to_turn(tool)),
        LlmAction::Final(response) => response_to_final_turn(response),
        LlmAction::Text(content) => parse_agent_turn(&content, default_status),
    }
}

fn tool_call_to_turn(tool: LlmToolCall) -> AgentTurn {
    match tool {
        LlmToolCall::ListFiles { kind } => AgentTurn::ListFiles { kind },
        LlmToolCall::ListReviewHunks => AgentTurn::ListReviewHunks,
        LlmToolCall::GetHunkContext { hunk_id } => AgentTurn::GetHunkContext { hunk_id },
        LlmToolCall::SearchText { query, kind } => AgentTurn::SearchText { query, kind },
        LlmToolCall::ReadFile { path } => AgentTurn::ReadFile { path },
        LlmToolCall::GetFileContext { path, line } => AgentTurn::GetFileContext { path, line },
        LlmToolCall::FindDefinitions { symbol } => AgentTurn::FindDefinitions { symbol },
        LlmToolCall::FindReferences { symbol } => AgentTurn::FindReferences { symbol },
    }
}

fn describe_action(action: &LlmAction) -> String {
    match action {
        LlmAction::Tool(tool) => match tool {
            LlmToolCall::ListFiles { kind } => format!("tool list_files kind={kind:?}"),
            LlmToolCall::ListReviewHunks => "tool list_review_hunks".to_string(),
            LlmToolCall::GetHunkContext { hunk_id } => {
                format!("tool get_hunk_context hunk_id={hunk_id:?}")
            }
            LlmToolCall::SearchText { query, kind } => {
                format!("tool search_text query={query:?} kind={kind:?}")
            }
            LlmToolCall::ReadFile { path } => format!("tool read_file path={path:?}"),
            LlmToolCall::GetFileContext { path, line } => {
                format!("tool get_file_context path={path:?} line={line}")
            }
            LlmToolCall::FindDefinitions { symbol } => {
                format!("tool find_definitions symbol={symbol:?}")
            }
            LlmToolCall::FindReferences { symbol } => {
                format!("tool find_references symbol={symbol:?}")
            }
        },
        LlmAction::Final(response) => format!(
            "final status={:?} severity={:?} evidence={}",
            response.status,
            response.severity,
            response.evidence.len()
        ),
        LlmAction::Text(content) => {
            format!("text {}", compact_text(content, 500))
        }
    }
}

fn describe_action_output(action: &LlmAction) -> String {
    match action {
        LlmAction::Tool(tool) => match tool {
            LlmToolCall::ListFiles { kind } => {
                json!({"action":"list_files","kind":kind}).to_string()
            }
            LlmToolCall::ListReviewHunks => json!({"action":"list_review_hunks"}).to_string(),
            LlmToolCall::GetHunkContext { hunk_id } => {
                json!({"action":"get_hunk_context","hunk_id":hunk_id}).to_string()
            }
            LlmToolCall::SearchText { query, kind } => {
                json!({"action":"search_text","query":query,"kind":kind}).to_string()
            }
            LlmToolCall::ReadFile { path } => json!({"action":"read_file","path":path}).to_string(),
            LlmToolCall::GetFileContext { path, line } => {
                json!({"action":"get_file_context","path":path,"line":line}).to_string()
            }
            LlmToolCall::FindDefinitions { symbol } => {
                json!({"action":"find_definitions","symbol":symbol}).to_string()
            }
            LlmToolCall::FindReferences { symbol } => {
                json!({"action":"find_references","symbol":symbol}).to_string()
            }
        },
        LlmAction::Final(response) => json!({
            "status": response.status,
            "severity": response.severity,
            "description": response.description,
            "evidence": response.evidence,
        })
        .to_string(),
        LlmAction::Text(content) => content.clone(),
    }
}

fn describe_turn(turn: &AgentTurn) -> String {
    match turn {
        AgentTurn::ListFiles { kind } => format!("list_files kind={kind:?}"),
        AgentTurn::ListReviewHunks => "list_review_hunks".to_string(),
        AgentTurn::GetHunkContext { hunk_id } => {
            format!("get_hunk_context hunk_id={hunk_id:?}")
        }
        AgentTurn::SearchText { query, kind } => {
            format!("search_text query={query:?} kind={kind:?}")
        }
        AgentTurn::ReadFile { path } => format!("read_file path={path:?}"),
        AgentTurn::GetFileContext { path, line } => {
            format!("get_file_context path={path:?} line={line}")
        }
        AgentTurn::FindDefinitions { symbol } => format!("find_definitions symbol={symbol:?}"),
        AgentTurn::FindReferences { symbol } => format!("find_references symbol={symbol:?}"),
        AgentTurn::Final { .. } => "final".to_string(),
    }
}

fn compact_text(value: &str, max_chars: usize) -> String {
    let mut compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() > max_chars {
        compact = compact.chars().take(max_chars).collect::<String>();
        compact.push_str("...");
    }
    compact
}

fn estimate_tokens(value: &str) -> usize {
    value.chars().count().div_ceil(4).max(1)
}

fn push_history(history: &mut Vec<String>, item: String) {
    history.push(item);
    while history.len() > MAX_HISTORY_ITEMS {
        history.remove(0);
    }
}

fn prompt_with_history(base_prompt: &str, history: &[String]) -> String {
    let mut start = 0;
    loop {
        let mut prompt = base_prompt.to_string();
        for item in &history[start..] {
            prompt.push_str(item);
        }
        if estimate_tokens(&prompt) <= MAX_PROMPT_TOKENS || start >= history.len() {
            return prompt;
        }
        start += 1;
    }
}

fn prompt_observation(
    step: usize,
    observation: &str,
    seen_observations: &mut HashSet<String>,
) -> String {
    if !seen_observations.insert(observation.to_string()) {
        return format!("Repeated observation from step {step} omitted.");
    }
    truncate_for_prompt(observation, MAX_OBSERVATION_CHARS)
}

fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let keep = max_chars.saturating_sub(100);
    let truncated = value.chars().take(keep).collect::<String>();
    format!("{truncated}\n... observation truncated for prompt budget ...")
}

fn fixture_marker_for_test_id(test_id: &str) -> Option<String> {
    test_id
        .strip_prefix("pass-")
        .map(|suffix| {
            format!(
                "KOOCHI_SAFE_{}",
                upper_snake(strip_redundant_outcome_prefix(suffix, "safe"))
            )
        })
        .or_else(|| {
            test_id.strip_prefix("fail-").map(|suffix| {
                format!(
                    "KOOCHI_FAIL_{}",
                    upper_snake(strip_redundant_outcome_prefix(suffix, "fail"))
                )
            })
        })
}

fn strip_redundant_outcome_prefix<'a>(suffix: &'a str, outcome: &str) -> &'a str {
    suffix
        .strip_prefix(outcome)
        .and_then(|rest| rest.strip_prefix('-'))
        .unwrap_or(suffix)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ToolKind {
    ListFiles,
    ListReviewHunks,
    GetHunkContext,
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
    target_marker_evidence: Option<Evidence>,
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
            target_marker_evidence: None,
        }
    }

    fn record(&mut self, kind: ToolKind, observation: &str) {
        self.observed.insert(kind);
        if let Some(marker) = &self.target_marker {
            if observation.contains(marker) {
                self.target_marker_seen = true;
                if self.target_marker_evidence.is_none() {
                    self.target_marker_evidence = marker_evidence(observation, marker);
                }
            }
        }
    }

    fn fixture_corrected_final(
        &self,
        test_id: &str,
        response: &LlmResponse,
    ) -> Option<LlmResponse> {
        if test_id.starts_with("fail-")
            && response.status == TestStatus::Passed
            && self.target_marker_seen
        {
            let marker = self
                .target_marker
                .as_deref()
                .unwrap_or("matching failure marker");
            return Some(LlmResponse {
                status: TestStatus::Failed,
                severity: response.severity.or(Some(Severity::High)),
                description: format!(
                    "Matching failure breadcrumb `{marker}` was observed, but the provider returned passed. Treating the fixture check as failed."
                ),
                evidence: self
                    .target_marker_evidence
                    .clone()
                    .into_iter()
                    .collect::<Vec<_>>(),
            });
        }
        None
    }

    fn fixture_step_limit_response(
        &self,
        test_id: &str,
        severity: Option<Severity>,
    ) -> Option<LlmResponse> {
        if !self.target_marker_seen {
            return None;
        }
        let marker = self.target_marker.as_deref()?;
        let evidence = self
            .target_marker_evidence
            .clone()
            .into_iter()
            .collect::<Vec<_>>();
        if test_id.starts_with("pass-") {
            return Some(LlmResponse {
                status: TestStatus::Passed,
                severity,
                description: format!(
                    "Matching safe breadcrumb `{marker}` was observed before the agent reached the step limit."
                ),
                evidence,
            });
        }
        if test_id.starts_with("fail-") {
            return Some(LlmResponse {
                status: TestStatus::Failed,
                severity: severity.or(Some(Severity::High)),
                description: format!(
                    "Matching failure breadcrumb `{marker}` was observed before the agent reached the step limit."
                ),
                evidence,
            });
        }
        None
    }

    fn final_guidance(&self, test_id: &str, response: &LlmResponse) -> Option<String> {
        if let Some(expected_status) = expected_fixture_status(test_id)
            && self.target_marker_seen
            && response.status == expected_status
        {
            return None;
        }
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
        let has_marker = self.target_marker_seen;
        if self.require_search && !self.observed.contains(&ToolKind::SearchText) {
            missing.push("search_text");
        }
        if self.require_content
            && !has_marker
            && !self.observed.contains(&ToolKind::ReadFile)
            && !self.observed.contains(&ToolKind::GetHunkContext)
            && !self.observed.contains(&ToolKind::GetFileContext)
        {
            missing.push("get_hunk_context, read_file, or get_file_context");
        }
        if self.require_definition
            && !has_marker
            && !self.observed.contains(&ToolKind::FindDefinitions)
        {
            missing.push("find_definitions");
        }
        if self.require_reference
            && !has_marker
            && !self.observed.contains(&ToolKind::FindReferences)
        {
            missing.push("find_references");
        }
        if self.require_context
            && !has_marker
            && !self.observed.contains(&ToolKind::GetHunkContext)
            && !self.observed.contains(&ToolKind::GetFileContext)
        {
            missing.push("get_hunk_context or get_file_context");
        }
        if let Some(marker) = &self.target_marker
            && !self.target_marker_seen
        {
            missing.push("search_text for matching fixture breadcrumb");
            let marker_hint = format!(
                " A useful fixture breadcrumb may be `{marker}`. A search_text call for this exact breadcrumb is required before final verdict."
            );
            let symbol_hint = symbol_hint_for_test_id(test_id)
                .map(|symbol| format!(" A useful symbol may be `{symbol}`."))
                .unwrap_or_default();
            return Some(format!(
                "This code-review agentic test requires more investigation before verdict. Missing required tool family: {}.{}{}",
                missing.join(", "),
                marker_hint,
                symbol_hint,
            ));
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

fn expected_fixture_status(test_id: &str) -> Option<TestStatus> {
    if test_id.starts_with("pass-") {
        Some(TestStatus::Passed)
    } else if test_id.starts_with("fail-") {
        Some(TestStatus::Failed)
    } else {
        None
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
    let id_driven = [
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
    .any(|needle| test_id.contains(needle));
    let instruction_driven = ["helper", "wrapper", "sanitizer", "verifier", "definition"]
        .iter()
        .any(|needle| instruction.contains(needle));
    id_driven || instruction_driven
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

fn marker_evidence(observation: &str, marker: &str) -> Option<Evidence> {
    let value = serde_json::from_str::<serde_json::Value>(observation).ok()?;
    let matches = value.get("matches")?.as_array()?;
    let matching = matches.iter().find(|item| {
        item.get("preview")
            .and_then(|preview| preview.as_str())
            .is_some_and(|preview| preview.contains(marker))
    })?;
    Some(Evidence {
        path: matching.get("path")?.as_str()?.to_string(),
        line: matching.get("line")?.as_u64()?.try_into().ok()?,
        preview: matching.get("preview")?.as_str()?.to_string(),
    })
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
    let hunks = search
        .list_review_hunks()
        .await
        .map_err(|err| AgentError::Search(err.to_string()))?
        .hunks;

    let file_count = files.len();
    let review_paths = files.iter().cloned().collect::<HashSet<_>>();
    let shown_files = files
        .iter()
        .take(MAX_CONTEXT_FILES)
        .map(|path| format!("- {path}"))
        .collect::<Vec<_>>()
        .join("\n");
    let file_inventory = if file_count > MAX_CONTEXT_FILES {
        format!(
            "Review-scope source file inventory ({file_count} total, first {MAX_CONTEXT_FILES} shown):\n{shown_files}"
        )
    } else {
        format!("Review-scope source file inventory ({file_count} total):\n{shown_files}")
    };
    let hunk_packet = format_review_hunk_packet(&hunks);
    let hunk_packet_tokens = estimate_tokens(&hunk_packet);
    let full_packet_tokens = estimate_tokens(&format!("{file_inventory}\n\n{hunk_packet}"));
    let context = if !hunks.is_empty() && full_packet_tokens <= agent.initial_context_token_budget {
        format!(
            "{file_inventory}\n\n{hunk_packet}\n\nChanged lines above are the primary review evidence. You may return a final verdict immediately if the changed-line packet is sufficient. If surrounding code, helper behavior, definitions, references, or callers are needed, request tools and continue the loop. Prefer get_hunk_context with a hunk id for targeted surrounding code before whole-file reads. Final failed evidence should point to changed lines or review-scope context directly caused by the change."
        )
    } else if !hunks.is_empty() {
        let hunk_summary = format_review_hunk_summary(&hunks);
        format!(
            "{file_inventory}\n\nReview-scope changed-line packet is too large to include directly ({} hunks, about {} tokens). Hunk summary:\n{}\n\nUse get_hunk_context with a hunk id or list_review_hunks for targeted details before returning a verdict. Use read_file only when the hunk context is insufficient.",
            hunks.len(),
            hunk_packet_tokens,
            hunk_summary
        )
    } else {
        format!(
            "{file_inventory}\nOnly fail when the concrete issue is in one of these review-scope files. You may use list_files, search_text, read_file, get_file_context, find_definitions, or find_references to gather context from the wider repository when needed, but final failed evidence must come from review-scope files."
        )
    };
    let evidence_index = HashSet::new();
    let changed_lines = changed_lines_for_hunks(&hunks);
    let review_causal_terms = review_causal_terms_for_hunks(&hunks);

    let instruction = grounded_agent_prompt(&agent.instruction, context.trim());

    Ok(GroundedRequest {
        request: LlmRequest {
            test_id: agent.id.clone(),
            model: agent.model.clone(),
            instruction,
        },
        evidence_index,
        review_paths,
        changed_lines,
        review_causal_terms,
    })
}

fn changed_lines_for_hunks(hunks: &[ReviewHunk]) -> HashSet<(String, u32)> {
    hunks
        .iter()
        .flat_map(|hunk| {
            hunk.lines.iter().filter_map(|line| {
                let line_number = match line.kind {
                    ReviewLineKind::Added | ReviewLineKind::Context => line.new_line,
                    ReviewLineKind::Removed => line.old_line,
                }?;
                Some((hunk.path.clone(), line_number))
            })
        })
        .collect()
}

fn review_causal_terms_for_hunks(hunks: &[ReviewHunk]) -> HashSet<String> {
    let mut terms = HashSet::new();
    for hunk in hunks {
        terms.insert(hunk.id.clone());
        for line in &hunk.lines {
            let trimmed = line.content.trim();
            if trimmed.chars().count() >= 4 {
                terms.insert(trimmed.to_string());
            }
            for token in symbol_tokens(trimmed) {
                terms.insert(token);
            }
        }
    }
    terms
}

fn symbol_tokens(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .filter(|token| token.chars().count() >= 4)
        .map(ToString::to_string)
        .collect()
}

fn format_review_hunk_packet(hunks: &[ReviewHunk]) -> String {
    let mut packet = format!("Review-scope changed hunks ({} total):", hunks.len());
    for hunk in hunks {
        packet.push_str(&format!(
            "\n\n--- hunk {} {} -{},{} +{},{}",
            hunk.id, hunk.path, hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
        ));
        for line in &hunk.lines {
            let (prefix, line_no) = match line.kind {
                ReviewLineKind::Added => ("+", line.new_line),
                ReviewLineKind::Removed => ("-", line.old_line),
                ReviewLineKind::Context => (" ", line.new_line.or(line.old_line)),
            };
            let line_no = line_no
                .map(|line| line.to_string())
                .unwrap_or_else(|| "-".to_string());
            packet.push_str(&format!("\n{prefix}{line_no}: {}", line.content));
        }
    }
    packet
}

fn format_review_hunk_summary(hunks: &[ReviewHunk]) -> String {
    let file_count = hunks
        .iter()
        .map(|hunk| hunk.path.as_str())
        .collect::<HashSet<_>>()
        .len();
    let mut summary = format!("{} changed files, {} hunks", file_count, hunks.len());
    for hunk in hunks {
        summary.push_str(&format!(
            "\n- {} {} -{},{} +{},{} ({} lines)",
            hunk.id,
            hunk.path,
            hunk.old_start,
            hunk.old_lines,
            hunk.new_start,
            hunk.new_lines,
            hunk.lines.len()
        ));
    }
    summary
}

fn agent_loop_prompt(test_id: &str, grounded_instruction: &str) -> String {
    let fixture_breadcrumb = fixture_marker_for_test_id(test_id)
        .map(|marker| {
            format!(
                "\nFor this fixture-style test id, the matching code breadcrumb is `{marker}`. Search for and inspect that breadcrumb when it is relevant."
            )
        })
        .unwrap_or_default();
    format!(
        r#"Agent test id: {test_id}

{grounded_instruction}
{fixture_breadcrumb}

You may either request one tool call or return the final verdict.

Tool call JSON forms:
{{"action":"list_files","kind":"source"}}
{{"action":"list_review_hunks"}}
{{"action":"get_hunk_context","hunk_id":"src/lib.rs#1"}}
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
- When Koochi gives review hunk ids, prefer get_hunk_context for targeted commit context before reading an entire file.
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
    ListReviewHunks,
    GetHunkContext {
        hunk_id: String,
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
    ListReviewHunks,
    GetHunkContext { hunk_id: String },
    SearchText { query: String, kind: FileKind },
    ReadFile { path: String },
    GetFileContext { path: String, line: u32 },
    FindDefinitions { symbol: String },
    FindReferences { symbol: String },
}

fn parse_agent_turn(
    content: &str,
    default_status: Option<TestStatus>,
) -> Result<AgentTurn, AgentError> {
    let json = extract_json_object(content).unwrap_or(content).trim();
    match serde_json::from_str::<AgentTurn>(json) {
        Ok(turn) => Ok(turn),
        Err(_) => {
            let repaired = repair_common_tool_json_typos(json);
            match serde_json::from_str::<AgentTurn>(&repaired) {
                Ok(turn) => Ok(turn),
                Err(_) => response_to_final_turn(parse_verdict_with_default_status(
                    content,
                    default_status,
                )?),
            }
        }
    }
}

fn repair_common_tool_json_typos(json: &str) -> String {
    [
        "action",
        "hunk_id",
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
        AgentTurn::ListReviewHunks => Ok(ToolTurn::ListReviewHunks),
        AgentTurn::GetHunkContext { hunk_id } => Ok(ToolTurn::GetHunkContext { hunk_id }),
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
        ToolTurn::ListReviewHunks => {
            let response = search
                .list_review_hunks()
                .await
                .map_err(|err| AgentError::Search(err.to_string()))?;
            for hunk in &response.hunks {
                for line in &hunk.lines {
                    if let Some(line) = line.new_line.or(line.old_line) {
                        evidence_index.insert((hunk.path.clone(), line));
                    }
                }
            }
            Ok(ExecutedTool::new(
                ToolKind::ListReviewHunks,
                json!({"hunks": review_hunk_summaries(&response.hunks)}).to_string(),
            ))
        }
        ToolTurn::GetHunkContext { hunk_id } => {
            let response = match search
                .get_hunk_context(GetHunkContextRequest { hunk_id })
                .await
            {
                Ok(response) => response,
                Err(err) => {
                    return Ok(ExecutedTool::new(
                        ToolKind::GetHunkContext,
                        json!({"error": err.to_string()}).to_string(),
                    ));
                }
            };
            if response.start_line > 0 {
                for line in response.start_line..=response.end_line {
                    evidence_index.insert((response.path.clone(), line));
                }
            }
            Ok(ExecutedTool::new(
                ToolKind::GetHunkContext,
                json!({
                    "hunk_id": response.hunk_id,
                    "path": response.path,
                    "start_line": response.start_line,
                    "end_line": response.end_line,
                    "content": response.content
                })
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
                json!({"matches": response.matches.into_iter().take(MAX_SEARCH_MATCHES).collect::<Vec<_>>()})
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
                    "content": response.content.lines().take(MAX_READ_FILE_LINES).collect::<Vec<_>>().join("\n")
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
                json!({"references": response.references.into_iter().take(MAX_REFERENCE_MATCHES).collect::<Vec<_>>()})
                    .to_string(),
            ))
        }
    }
}

fn review_hunk_summaries(hunks: &[ReviewHunk]) -> Vec<serde_json::Value> {
    hunks
        .iter()
        .map(|hunk| {
            json!({
                "id": hunk.id,
                "path": hunk.path,
                "old_start": hunk.old_start,
                "old_lines": hunk.old_lines,
                "new_start": hunk.new_start,
                "new_lines": hunk.new_lines,
                "line_count": hunk.lines.len(),
            })
        })
        .collect()
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
