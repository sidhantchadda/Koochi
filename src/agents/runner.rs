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
use crate::scope::ReviewMode;
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
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::fmt::Display;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use tokio::sync::Mutex;
use tokio::time::MissedTickBehavior;

mod evidence;
mod grounding;
mod investigation;

use evidence::{classify_evidence, verdict_from_loop_result};
use grounding::{build_grounded_request, substantive_changed_line};
use investigation::{InvestigationState, ToolKind};

const MAX_CONTEXT_FILES: usize = 32;
const MAX_PROMPT_TOKENS: usize = 120_000;
const MAX_HISTORY_ITEMS: usize = 16;
const MAX_OBSERVATION_CHARS: usize = 12_000;
const MAX_READ_FILE_LINES: usize = 120;
const MAX_SEARCH_MATCHES: usize = 40;
const MAX_REFERENCE_MATCHES: usize = 80;
const MAX_HUNK_SUMMARY_PREVIEW_LINES: usize = 4;
const REVIEW_COVERAGE_CHUNK_LINES: usize = MAX_READ_FILE_LINES;
const MAX_REVIEW_COVERAGE_BATCH_CHARS: usize = 10_000;
const FULL_REPO_REQUIRED_SEARCH_TERMS: usize = 30;

#[derive(Debug, Clone)]
pub struct ReviewScopeInventory {
    file_count: usize,
    line_count: u64,
    byte_count: u64,
    chunks: Vec<ReviewSourceChunk>,
}

impl ReviewScopeInventory {
    pub fn file_count(&self) -> usize {
        self.file_count
    }

    pub fn line_count(&self) -> u64 {
        self.line_count
    }

    pub fn byte_count(&self) -> u64 {
        self.byte_count
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn chunk_line_limit(&self) -> usize {
        REVIEW_COVERAGE_CHUNK_LINES
    }

    fn is_empty(&self) -> bool {
        self.file_count == 0 || self.chunks.is_empty()
    }
}

#[derive(Debug, Clone)]
struct ReviewSourceChunk {
    index: usize,
    path: String,
    start_line: u32,
    end_line: u32,
    content: String,
}

#[derive(Debug, Clone, Default)]
pub struct AgentDiagnostics {
    prompt_dump_dir: Option<PathBuf>,
}

impl AgentDiagnostics {
    pub fn with_prompt_dump_dir(path: impl Into<PathBuf>) -> Self {
        Self {
            prompt_dump_dir: Some(path.into()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error(transparent)]
    Llm(#[from] LlmBusError),
    #[error(
        "provider rejected prompt for agent `{test_id}` at step {step} ({prompt_tokens} estimated prompt tokens); redacted prompt dump: {prompt_dump_path}: {source}"
    )]
    PromptRejected {
        test_id: String,
        step: usize,
        prompt_tokens: usize,
        prompt_dump_path: PathBuf,
        #[source]
        source: LlmBusError,
    },
    #[error(
        "provider rejected prompt for agent `{test_id}` at step {step} ({prompt_tokens} estimated prompt tokens); prompt dump unavailable ({dump_error}): {source}"
    )]
    PromptRejectedWithoutDump {
        test_id: String,
        step: usize,
        prompt_tokens: usize,
        dump_error: String,
        #[source]
        source: LlmBusError,
    },
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
        tool_cache_hits: usize,
        tool_cache_misses: usize,
        non_progress_terminations: usize,
        coverage_chunks_delivered: usize,
        coverage_pass_rejections: usize,
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
        cache_hit: bool,
        observation: String,
    },
    NonProgressTerminated {
        step: usize,
        response: LlmResponse,
    },
    ReviewCoverageDelivered {
        step: usize,
        delivered_chunks: usize,
        total_chunks: usize,
        remaining_chunks: usize,
        observation: String,
    },
    PassCoverageRejected {
        step: usize,
        delivered_chunks: usize,
        total_chunks: usize,
        guidance: String,
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
    UnfocusedChanged,
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
    let inventory = Arc::new(build_review_scope_inventory(search.as_ref()).await?);
    run_agents_with_inventory_and_progress(
        agents,
        search,
        bus,
        max_parallel_agents,
        max_agent_steps,
        inventory,
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
    progress: F,
) -> Result<Vec<AgentVerdict>, AgentError>
where
    S: CodeSearchApi + 'static,
    S::Error: Display,
    B: LlmBus + ?Sized + 'static,
    F: FnMut(AgentProgressEvent),
{
    let inventory = Arc::new(build_review_scope_inventory(search.as_ref()).await?);
    run_agents_with_inventory_and_progress(
        agents,
        search,
        bus,
        max_parallel_agents,
        max_agent_steps,
        inventory,
        progress,
    )
    .await
}

pub async fn run_agents_with_inventory_and_progress<S, B, F>(
    agents: Vec<AgentSpec>,
    search: Arc<S>,
    bus: Arc<B>,
    max_parallel_agents: usize,
    max_agent_steps: usize,
    inventory: Arc<ReviewScopeInventory>,
    progress: F,
) -> Result<Vec<AgentVerdict>, AgentError>
where
    S: CodeSearchApi + 'static,
    S::Error: Display,
    B: LlmBus + ?Sized + 'static,
    F: FnMut(AgentProgressEvent),
{
    run_agents_with_inventory_and_progress_and_diagnostics(
        agents,
        search,
        bus,
        max_parallel_agents,
        max_agent_steps,
        inventory,
        AgentDiagnostics::default(),
        progress,
    )
    .await
}

pub async fn run_agents_with_inventory_and_progress_and_diagnostics<S, B, F>(
    agents: Vec<AgentSpec>,
    search: Arc<S>,
    bus: Arc<B>,
    max_parallel_agents: usize,
    max_agent_steps: usize,
    inventory: Arc<ReviewScopeInventory>,
    diagnostics: AgentDiagnostics,
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
    let tool_cache = Arc::new(ToolExecutionCache::default());
    let diagnostics = Arc::new(diagnostics);
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
                let tool_cache = tool_cache.clone();
                let inventory = inventory.clone();
                let diagnostics = diagnostics.clone();
                async move {
                    let result = run_agent_loop(
                        agent,
                        search,
                        bus,
                        tool_cache,
                        inventory,
                        max_agent_steps,
                        diagnostics,
                    )
                    .await;
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
        let tool_cache_hits = loop_results
            .iter()
            .map(|result| result.tool_cache_hits)
            .sum::<usize>();
        let tool_cache_misses = loop_results
            .iter()
            .map(|result| result.tool_cache_misses)
            .sum::<usize>();
        let non_progress_terminations = loop_results
            .iter()
            .map(|result| result.non_progress_terminations)
            .sum::<usize>();
        let coverage_chunks_delivered = loop_results
            .iter()
            .map(|result| result.coverage_chunks_delivered)
            .sum::<usize>();
        let coverage_pass_rejections = loop_results
            .iter()
            .map(|result| result.coverage_pass_rejections)
            .sum::<usize>();
        progress(AgentProgressEvent::BatchCompleted {
            batch_index: batch_index + 1,
            batch_count,
            agent_count: loop_results.len(),
            llm_calls,
            native_tool_calls,
            native_final_calls,
            text_fallback_turns,
            tool_cache_hits,
            tool_cache_misses,
            non_progress_terminations,
            coverage_chunks_delivered,
            coverage_pass_rejections,
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
    let inventory = Arc::new(build_review_scope_inventory(search.as_ref()).await?);
    run_agent_with_trace_and_inventory(agent, search, bus, inventory, max_agent_steps, trace).await
}

pub async fn run_agent_with_trace_and_inventory<S, B, F>(
    agent: AgentSpec,
    search: Arc<S>,
    bus: Arc<B>,
    inventory: Arc<ReviewScopeInventory>,
    max_agent_steps: usize,
    trace: F,
) -> Result<AgentVerdict, AgentError>
where
    S: CodeSearchApi + 'static,
    S::Error: Display,
    B: LlmBus + ?Sized + 'static,
    F: FnMut(AgentTraceEvent),
{
    run_agent_with_trace_and_inventory_and_diagnostics(
        agent,
        search,
        bus,
        inventory,
        max_agent_steps,
        AgentDiagnostics::default(),
        trace,
    )
    .await
}

pub async fn run_agent_with_trace_and_inventory_and_diagnostics<S, B, F>(
    agent: AgentSpec,
    search: Arc<S>,
    bus: Arc<B>,
    inventory: Arc<ReviewScopeInventory>,
    max_agent_steps: usize,
    diagnostics: AgentDiagnostics,
    trace: F,
) -> Result<AgentVerdict, AgentError>
where
    S: CodeSearchApi + 'static,
    S::Error: Display,
    B: LlmBus + ?Sized + 'static,
    F: FnMut(AgentTraceEvent),
{
    let loop_result = run_agent_loop_traced(
        &agent,
        search,
        bus,
        Arc::new(ToolExecutionCache::default()),
        inventory,
        max_agent_steps,
        Arc::new(diagnostics),
        trace,
    )
    .await?;
    Ok(verdict_from_loop_result(&agent, loop_result))
}

struct AgentLoopResult {
    response: LlmResponse,
    evidence_index: HashSet<(String, u32)>,
    review_paths: HashSet<String>,
    changed_lines: HashSet<(String, u32)>,
    relevant_changed_lines: HashSet<(String, u32)>,
    review_causal_terms: HashSet<String>,
    elapsed: Duration,
    llm_calls: usize,
    native_tool_calls: usize,
    native_final_calls: usize,
    text_fallback_turns: usize,
    tool_cache_hits: usize,
    tool_cache_misses: usize,
    non_progress_terminations: usize,
    coverage_chunks_delivered: usize,
    coverage_pass_rejections: usize,
}

struct GroundedRequest {
    request: LlmRequest,
    evidence_index: HashSet<(String, u32)>,
    review_paths: HashSet<String>,
    changed_lines: HashSet<(String, u32)>,
    target_context_line: Option<(String, u32)>,
    focused_context_line: Option<(String, u32)>,
    relevant_changed_lines: HashSet<(String, u32)>,
    review_causal_terms: HashSet<String>,
    allows_direct_verdict: bool,
    full_repo_mode: bool,
    full_repo_search_terms: Vec<String>,
}

#[derive(Debug, Default)]
struct ReviewCoverageState {
    next_chunk: usize,
    coverage_chunks_delivered: usize,
    pass_rejections: usize,
}

struct ReviewCoverageBatch {
    observation: String,
    evidence_lines: Vec<(String, u32)>,
    chunk_count: usize,
    remaining_chunks: usize,
}

impl ReviewCoverageState {
    fn is_complete(&self, inventory: &ReviewScopeInventory) -> bool {
        inventory.is_empty() || self.next_chunk >= inventory.chunks.len()
    }

    fn delivered_chunks(&self) -> usize {
        self.next_chunk
    }

    fn coverage_chunks_delivered(&self) -> usize {
        self.coverage_chunks_delivered
    }

    fn pass_rejections(&self) -> usize {
        self.pass_rejections
    }

    fn record_pass_rejection(&mut self) {
        self.pass_rejections += 1;
    }

    fn next_batch(&mut self, inventory: &ReviewScopeInventory) -> Option<ReviewCoverageBatch> {
        if self.is_complete(inventory) {
            return None;
        }
        let start = self.next_chunk;
        let mut end = start;
        let mut rendered_chars = 0usize;
        let mut chunks = Vec::new();
        let mut evidence_lines = Vec::new();
        while end < inventory.chunks.len() {
            let chunk = &inventory.chunks[end];
            let rendered = coverage_chunk_json(chunk);
            let rendered_len = rendered.to_string().chars().count();
            if end > start && rendered_chars + rendered_len > MAX_REVIEW_COVERAGE_BATCH_CHARS {
                break;
            }
            rendered_chars += rendered_len;
            evidence_lines
                .extend((chunk.start_line..=chunk.end_line).map(|line| (chunk.path.clone(), line)));
            chunks.push(rendered);
            end += 1;
        }
        self.next_chunk = end;
        self.coverage_chunks_delivered += chunks.len();
        let remaining_chunks = inventory.chunks.len().saturating_sub(self.next_chunk);
        Some(ReviewCoverageBatch {
            observation: json!({
                "review_scope_coverage": {
                    "delivered_chunks": chunks,
                    "delivered_chunk_count": end - start,
                    "delivered_through_chunk": self.next_chunk,
                    "total_chunks": inventory.chunks.len(),
                    "remaining_chunks": remaining_chunks,
                }
            })
            .to_string(),
            evidence_lines,
            chunk_count: end - start,
            remaining_chunks,
        })
    }
}

async fn run_agent_loop<S, B>(
    agent: &AgentSpec,
    search: Arc<S>,
    bus: Arc<B>,
    tool_cache: Arc<ToolExecutionCache>,
    inventory: Arc<ReviewScopeInventory>,
    max_agent_steps: usize,
    diagnostics: Arc<AgentDiagnostics>,
) -> Result<AgentLoopResult, AgentError>
where
    S: CodeSearchApi + 'static,
    S::Error: Display,
    B: LlmBus + ?Sized + 'static,
{
    run_agent_loop_traced(
        agent,
        search,
        bus,
        tool_cache,
        inventory,
        max_agent_steps,
        diagnostics,
        |_| {},
    )
    .await
}

async fn run_agent_loop_traced<S, B, F>(
    agent: &AgentSpec,
    search: Arc<S>,
    bus: Arc<B>,
    tool_cache: Arc<ToolExecutionCache>,
    inventory: Arc<ReviewScopeInventory>,
    max_agent_steps: usize,
    diagnostics: Arc<AgentDiagnostics>,
    mut trace: F,
) -> Result<AgentLoopResult, AgentError>
where
    S: CodeSearchApi + 'static,
    S::Error: Display,
    B: LlmBus + ?Sized + 'static,
    F: FnMut(AgentTraceEvent),
{
    let agent_started = Instant::now();
    let grounded = build_grounded_request(agent, search.as_ref()).await?;
    trace(AgentTraceEvent::Started {
        test_id: agent.id.clone(),
        max_steps: max_agent_steps.max(1),
    });
    if grounded.review_paths.is_empty() {
        let response = LlmResponse {
            status: TestStatus::Passed,
            severity: None,
            description: "No review-scope source files were found, so there is no code to evaluate for this invariant.".to_string(),
            evidence: Vec::new(),
        };
        trace(AgentTraceEvent::FinalVerdict {
            step: 0,
            response: response.clone(),
        });
        trace(AgentTraceEvent::EvidenceClassified { items: Vec::new() });
        return Ok(AgentLoopResult {
            response,
            evidence_index: grounded.evidence_index,
            review_paths: grounded.review_paths,
            changed_lines: grounded.changed_lines,
            relevant_changed_lines: grounded.relevant_changed_lines,
            review_causal_terms: grounded.review_causal_terms,
            elapsed: agent_started.elapsed(),
            llm_calls: 0,
            native_tool_calls: 0,
            native_final_calls: 0,
            text_fallback_turns: 0,
            tool_cache_hits: 0,
            tool_cache_misses: 0,
            non_progress_terminations: 0,
            coverage_chunks_delivered: 0,
            coverage_pass_rejections: 0,
        });
    }
    let base_prompt = agent_loop_prompt(&agent.id, &grounded.request.instruction);
    let mut history = Vec::new();
    let mut seen_observations = HashSet::new();
    let has_explicit_targeted_rescue = grounded.target_context_line.is_some()
        || instruction_review_path(&agent.instruction, &grounded.review_paths).is_some();
    let mut targeted_rescue_hint = targeted_rescue_turn(agent, &grounded);
    let mut full_repo_rescue_terms = VecDeque::from(grounded.full_repo_search_terms.clone());
    let mut evidence_index = grounded.evidence_index;
    let mut investigation = InvestigationState::new(agent);
    let mut coverage = ReviewCoverageState::default();
    let mut llm_calls = 0;
    let mut native_tool_calls = 0;
    let mut native_final_calls = 0;
    let mut text_fallback_turns = 0;
    let mut tool_cache_hits = 0;
    let mut tool_cache_misses = 0;
    let mut non_progress_terminations = 0;
    let mut non_progress = NonProgressState::default();
    let mut deferred_failed_response = None;
    let mut contradictory_pass_rejections = 0usize;
    push_history(
        &mut history,
        format!(
            "\n\nReview-scope coverage gate:\nKoochi has indexed {} source files, {} LOC, and {} deterministic source chunks for this review scope. You may return `failed` as soon as concrete review-scope evidence demonstrates a violation. Koochi will not accept `passed` until it has shown this agent every review-scope source chunk.",
            inventory.file_count(),
            inventory.line_count(),
            inventory.chunk_count()
        ),
    );

    for step in 1..=max_agent_steps.max(1) {
        let prompt = prompt_with_history(&base_prompt, &history);
        let prompt_tokens = estimate_tokens(&prompt);
        trace(AgentTraceEvent::StepStarted {
            step,
            prompt_tokens,
            prompt: prompt.clone(),
        });
        llm_calls += 1;
        let action = match bus
            .complete_action(LlmRequest {
                test_id: agent.id.clone(),
                model: agent.model.clone(),
                instruction: prompt.clone(),
            })
            .await
        {
            Ok(action) => action,
            Err(LlmBusError::InvalidVerdict(content)) => {
                trace(AgentTraceEvent::InvalidResponse {
                    step,
                    content: content.trim().to_string(),
                });
                push_history(
                    &mut history,
                    format!(
                        "\n\nStep {step} invalid model response rejected:\nThe provider returned `{}` which is not a valid Koochi tool call or final verdict. Return exactly one valid tool call or final_verdict with all required fields. For search_text, include a non-empty `query` string.",
                        content.trim()
                    ),
                );
                continue;
            }
            Err(error) => {
                return Err(contextualize_llm_error(
                    &agent.id,
                    step,
                    prompt_tokens,
                    &prompt,
                    diagnostics.as_ref(),
                    error,
                )
                .await);
            }
        };
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
        let turn = match action_to_turn(action, None) {
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
            if final_response.status == TestStatus::Passed
                && !coverage.is_complete(inventory.as_ref())
            {
                coverage.record_pass_rejection();
                let guidance = format!(
                    "Passed verdict rejected because this agent has reviewed {}/{} review-scope source chunks. Koochi must show every review-scope source chunk to this agent before accepting passed.",
                    coverage.delivered_chunks(),
                    inventory.chunk_count()
                );
                trace(AgentTraceEvent::PassCoverageRejected {
                    step,
                    delivered_chunks: coverage.delivered_chunks(),
                    total_chunks: inventory.chunk_count(),
                    guidance: guidance.clone(),
                });
                let Some(batch) = coverage.next_batch(inventory.as_ref()) else {
                    push_history(
                        &mut history,
                        format!(
                            "\n\nStep {step} passed verdict rejected:\n{guidance}\n\nReturn exactly one final verdict JSON now."
                        ),
                    );
                    continue;
                };
                for line in &batch.evidence_lines {
                    evidence_index.insert(line.clone());
                }
                investigation.record(ToolKind::ReviewCoverage, &batch.observation);
                trace(AgentTraceEvent::ReviewCoverageDelivered {
                    step,
                    delivered_chunks: coverage.delivered_chunks(),
                    total_chunks: inventory.chunk_count(),
                    remaining_chunks: batch.remaining_chunks,
                    observation: batch.observation.clone(),
                });
                push_history(
                    &mut history,
                    format!(
                        "\n\nStep {step} passed verdict rejected:\n{guidance}\n\nReview-scope source coverage batch ({} chunks, {} remaining):\n{}\n\nStudy this exact source. Return `failed` immediately if this batch demonstrates a concrete violation. Return `passed` only if you have no concrete finding after Koochi has delivered every review-scope source chunk.",
                        batch.chunk_count, batch.remaining_chunks, batch.observation
                    ),
                );
                continue;
            }
            let direct_verdict_allowed = grounded.allows_direct_verdict
                && direct_verdict_is_grounded(&final_response, &grounded.relevant_changed_lines)
                && direct_verdict_satisfies_investigation(&final_response);
            if !direct_verdict_allowed
                && let Some(guidance) = investigation.final_guidance(&final_response)
            {
                if final_response.status == TestStatus::Failed
                    && !final_response.evidence.is_empty()
                {
                    deferred_failed_response = Some(final_response.clone());
                }
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
            if failed_verdict_lacks_line_evidence(&final_response)
                && !is_absence_policy(&agent.instruction)
            {
                let guidance = "Failed verdicts must include at least one accepted evidence item from a changed line or targeted review-scope context. Use exact path, line, and preview values from the most recent get_hunk_context, get_file_context, or read_file observation, then return the failed verdict again.".to_string();
                trace(AgentTraceEvent::PrematureFinal {
                    step,
                    guidance: guidance.clone(),
                });
                push_history(
                    &mut history,
                    format!(
                        "\n\nStep {step} failed verdict rejected:\n{guidance}\n\nReturn exactly one final_verdict JSON now if the issue is concrete, or a passed verdict if no concrete review-scope issue remains."
                    ),
                );
                continue;
            }
            if let Some(guidance) = failed_verdict_has_mismatched_evidence_preview(
                search.as_ref(),
                &final_response,
                &evidence_index,
                &grounded.review_paths,
                &grounded.changed_lines,
                &grounded.relevant_changed_lines,
            )
            .await?
                && !is_absence_policy(&agent.instruction)
            {
                trace(AgentTraceEvent::PrematureFinal {
                    step,
                    guidance: guidance.clone(),
                });
                push_history(
                    &mut history,
                    format!(
                        "\n\nStep {step} failed verdict evidence rejected:\n{guidance}\n\nUse exact path, line, and preview values from a targeted content observation, then return failed only if that exact line demonstrates the issue. Otherwise return passed/no concrete finding."
                    ),
                );
                continue;
            }
            if failed_verdict_lacks_substantive_accepted_evidence(
                &final_response,
                &evidence_index,
                &grounded.review_paths,
                &grounded.changed_lines,
                &grounded.relevant_changed_lines,
            ) && !is_absence_policy(&agent.instruction)
            {
                let guidance = "Failed verdict evidence must cite a substantive line that demonstrates the issue. Do not cite only braces, imports, type aliases, comments, or broad plumbing. Use exact path, line, and preview values from the targeted content observation, then return the failed verdict again.".to_string();
                trace(AgentTraceEvent::PrematureFinal {
                    step,
                    guidance: guidance.clone(),
                });
                push_history(
                    &mut history,
                    format!(
                        "\n\nStep {step} weak failed evidence rejected:\n{guidance}\n\nReturn exactly one final_verdict JSON now if the issue is concrete, or a passed verdict if no concrete review-scope issue remains."
                    ),
                );
                continue;
            }
            if let Some(target_path) = failed_verdict_lacks_target_path_evidence(
                &final_response,
                &agent.instruction,
                &grounded.review_paths,
            ) {
                let guidance = format!(
                    "Failed verdict evidence must be tied to the target review file `{target_path}` named by this invariant. Use get_file_context or read_file for `{target_path}`, then return failed only with evidence from that file or its immediate helper context."
                );
                trace(AgentTraceEvent::PrematureFinal {
                    step,
                    guidance: guidance.clone(),
                });
                push_history(
                    &mut history,
                    format!(
                        "\n\nStep {step} failed verdict rejected:\n{guidance}\n\nReturn exactly one targeted content tool call now."
                    ),
                );
                continue;
            }
            if grounded.full_repo_mode
                && failed_verdict_lacks_full_repo_focus_evidence(
                    &final_response,
                    &grounded.full_repo_search_terms,
                )
            {
                let guidance = "Full-repo failed verdict evidence must be concretely tied to this invariant's focus terms. Do not cite generic declarations, bundled/minified blobs, or unrelated lines. Search a specific invariant term, inspect a focused source context, and cite a line whose preview itself demonstrates the violation.".to_string();
                trace(AgentTraceEvent::PrematureFinal {
                    step,
                    guidance: guidance.clone(),
                });
                push_history(
                    &mut history,
                    format!(
                        "\n\nStep {step} weak full-repo failed evidence rejected:\n{guidance}\n\nReturn one targeted tool call now, or return passed if no concrete full-repo issue remains."
                    ),
                );
                continue;
            }
            if failed_verdict_is_speculative(&final_response) {
                let guidance = "Failed verdicts must describe a concrete review-scope violation, not a possible gap or something that still requires confirmation. Return failed only with evidence that directly demonstrates the violation; otherwise return passed/no concrete finding.".to_string();
                trace(AgentTraceEvent::PrematureFinal {
                    step,
                    guidance: guidance.clone(),
                });
                push_history(
                    &mut history,
                    format!("\n\nStep {step} speculative failed verdict rejected:\n{guidance}"),
                );
                continue;
            }
            if failed_verdict_contradicts_no_finding_language(&final_response) {
                let guidance = "Your description says the invariant is satisfied, no concrete violation was found, or the verdict is only blocked by coverage. Return `failed` only for a concrete review-scope violation; otherwise return `passed` and let Koochi handle any remaining coverage gate.".to_string();
                trace(AgentTraceEvent::PrematureFinal {
                    step,
                    guidance: guidance.clone(),
                });
                push_history(
                    &mut history,
                    format!(
                        "\n\nStep {step} inconsistent failed verdict rejected:\n{guidance}"
                    ),
                );
                continue;
            }
            if passed_verdict_contradicts_failure_language(&final_response, &agent.instruction) {
                contradictory_pass_rejections += 1;
                if contradictory_pass_rejections >= 2 {
                    let mut repaired_response = final_response.clone();
                    repaired_response.status = TestStatus::Failed;
                    if repaired_response.severity.is_none() {
                        repaired_response.severity = agent.severity;
                    }
                    if let Some(evidence) = target_failure_evidence(
                        search.as_ref(),
                        &grounded.target_context_line,
                        &agent.instruction,
                    )
                    .await?
                    {
                        evidence_index.insert((evidence.path.clone(), evidence.line));
                        repaired_response.evidence = vec![evidence];
                    }
                    repaired_response.description = format!(
                        "The provider described the `Fail if` condition for `{}` while returning `passed`; treating it as failed because status must agree with concrete review-scope evidence.",
                        agent.id
                    );
                    if !failed_verdict_lacks_line_evidence(&repaired_response)
                        && failed_verdict_lacks_target_path_evidence(
                            &repaired_response,
                            &agent.instruction,
                            &grounded.review_paths,
                        )
                        .is_none()
                        && !failed_verdict_lacks_substantive_accepted_evidence(
                            &repaired_response,
                            &evidence_index,
                            &grounded.review_paths,
                            &grounded.changed_lines,
                            &grounded.relevant_changed_lines,
                        )
                        && (!grounded.full_repo_mode
                            || !failed_verdict_lacks_full_repo_focus_evidence(
                                &repaired_response,
                                &grounded.full_repo_search_terms,
                            ))
                    {
                        trace(AgentTraceEvent::FinalVerdict {
                            step,
                            response: repaired_response.clone(),
                        });
                        trace(AgentTraceEvent::EvidenceClassified {
                            items: classify_evidence(
                                &repaired_response.evidence,
                                &evidence_index,
                                &grounded.review_paths,
                                &grounded.changed_lines,
                                &grounded.relevant_changed_lines,
                            ),
                        });
                        return Ok(AgentLoopResult {
                            response: repaired_response,
                            evidence_index,
                            review_paths: grounded.review_paths,
                            changed_lines: grounded.changed_lines,
                            relevant_changed_lines: grounded.relevant_changed_lines,
                            review_causal_terms: grounded.review_causal_terms,
                            elapsed: agent_started.elapsed(),
                            llm_calls,
                            native_tool_calls,
                            native_final_calls,
                            text_fallback_turns,
                            tool_cache_hits,
                            tool_cache_misses,
                            non_progress_terminations,
                            coverage_chunks_delivered: coverage.coverage_chunks_delivered(),
                            coverage_pass_rejections: coverage.pass_rejections(),
                        });
                    }
                }
                let guidance = "Your description says the invariant is violated; return `failed` with evidence, or rewrite the description if it is actually satisfied.".to_string();
                trace(AgentTraceEvent::PrematureFinal {
                    step,
                    guidance: guidance.clone(),
                });
                push_history(
                    &mut history,
                    format!("\n\nStep {step} inconsistent final verdict rejected:\n{guidance}"),
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
                    &grounded.relevant_changed_lines,
                ),
            });
            return Ok(AgentLoopResult {
                response: final_response,
                evidence_index,
                review_paths: grounded.review_paths,
                changed_lines: grounded.changed_lines,
                relevant_changed_lines: grounded.relevant_changed_lines,
                review_causal_terms: grounded.review_causal_terms,
                elapsed: agent_started.elapsed(),
                llm_calls,
                native_tool_calls,
                native_final_calls,
                text_fallback_turns,
                tool_cache_hits,
                tool_cache_misses,
                non_progress_terminations,
                coverage_chunks_delivered: coverage.coverage_chunks_delivered(),
                coverage_pass_rejections: coverage.pass_rejections(),
            });
        } else {
            let tool = describe_turn(&turn);
            if has_explicit_targeted_rescue
                && !investigation.has_content_observation()
                && is_broad_discovery_turn(&turn)
                && let Some(rescue_turn) = targeted_rescue_hint.take()
            {
                let rescue_tool = format!("targeted rescue {}", describe_turn(&rescue_turn));
                let executed = execute_tool(
                    rescue_turn,
                    search.as_ref(),
                    tool_cache.as_ref(),
                    &mut evidence_index,
                )
                .await?;
                if executed.cache_hit {
                    tool_cache_hits += 1;
                } else {
                    tool_cache_misses += 1;
                }
                trace(AgentTraceEvent::ToolExecuted {
                    step,
                    tool: rescue_tool,
                    cache_hit: executed.cache_hit,
                    observation: executed.observation.clone(),
                });
                investigation.record(executed.kind, &executed.observation);
                let observation_for_prompt =
                    prompt_observation(step, &executed.observation, &mut seen_observations);
                push_history(
                    &mut history,
                    format!(
                        "\n\nStep {step} targeted context observation supplied instead of broad discovery tool `{tool}`:\n{}\n\nRequired investigation is satisfied. Return the final verdict now. Use exact evidence paths and lines from this observation.",
                        observation_for_prompt
                    ),
                );
                continue;
            }
            if let Some(decision) =
                non_progress.record(&turn, investigation.has_content_observation())
            {
                if grounded.full_repo_mode
                    && !investigation.has_content_observation()
                    && let Some(executed_tools) = execute_full_repo_rescue(
                        search.as_ref(),
                        tool_cache.as_ref(),
                        &mut evidence_index,
                        &mut full_repo_rescue_terms,
                    )
                    .await?
                {
                    let mut observations = Vec::new();
                    for executed in executed_tools {
                        if executed.cache_hit {
                            tool_cache_hits += 1;
                        } else {
                            tool_cache_misses += 1;
                        }
                        trace(AgentTraceEvent::ToolExecuted {
                            step,
                            tool: format!("full-repo targeted rescue {}", executed.tool_label()),
                            cache_hit: executed.cache_hit,
                            observation: executed.observation.clone(),
                        });
                        investigation.record(executed.kind, &executed.observation);
                        observations.push(prompt_observation(
                            step,
                            &executed.observation,
                            &mut seen_observations,
                        ));
                    }
                    push_history(
                        &mut history,
                        format!(
                            "\n\nStep {step} full-repo targeted search/context supplied after broad non-progress:\n{}\n\nUse these concrete observations. If a search match looks relevant, inspect it with get_file_context or read_file before returning failed. Return passed only if no concrete review-scope issue remains.",
                            observations.join("\n")
                        ),
                    );
                    continue;
                }
                if !investigation.has_content_observation()
                    && let Some(rescue_turn) = targeted_rescue_hint.take()
                {
                    let rescue_tool = format!("targeted rescue {}", describe_turn(&rescue_turn));
                    let executed = execute_tool(
                        rescue_turn,
                        search.as_ref(),
                        tool_cache.as_ref(),
                        &mut evidence_index,
                    )
                    .await?;
                    if executed.cache_hit {
                        tool_cache_hits += 1;
                    } else {
                        tool_cache_misses += 1;
                    }
                    trace(AgentTraceEvent::ToolExecuted {
                        step,
                        tool: rescue_tool,
                        cache_hit: executed.cache_hit,
                        observation: executed.observation.clone(),
                    });
                    investigation.record(executed.kind, &executed.observation);
                    let observation_for_prompt =
                        prompt_observation(step, &executed.observation, &mut seen_observations);
                    push_history(
                        &mut history,
                        format!(
                            "\n\nStep {step} targeted context observation after repeated broad tool use:\n{}\n\nRequired investigation is satisfied. Return the final verdict now. Use exact evidence paths and lines from this observation.",
                            observation_for_prompt
                        ),
                    );
                    continue;
                }
                match decision {
                    NonProgressDecision::Warn(guidance) => {
                        push_history(
                            &mut history,
                            format!(
                                "\n\nStep {step} repeated or broad tool call rejected:\n{guidance}\n\nReturn one different targeted tool call or a final passed verdict if there is no concrete review-scope finding."
                            ),
                        );
                        continue;
                    }
                    NonProgressDecision::Terminate(reason) => {
                        if !coverage.is_complete(inventory.as_ref())
                            && let Some(batch) = coverage.next_batch(inventory.as_ref())
                        {
                            for line in &batch.evidence_lines {
                                evidence_index.insert(line.clone());
                            }
                            investigation.record(ToolKind::ReviewCoverage, &batch.observation);
                            trace(AgentTraceEvent::ReviewCoverageDelivered {
                                step,
                                delivered_chunks: coverage.delivered_chunks(),
                                total_chunks: inventory.chunk_count(),
                                remaining_chunks: batch.remaining_chunks,
                                observation: batch.observation.clone(),
                            });
                            push_history(
                                &mut history,
                                format!(
                                    "\n\nStep {step} repeated or broad tool use stopped: {reason}\n\nKoochi is continuing review-scope coverage before any pass can be accepted. Coverage batch ({} chunks, {} remaining):\n{}\n\nStudy this exact source. Return `failed` immediately if this batch demonstrates a concrete violation. Return `passed` only after Koochi has delivered every review-scope source chunk.",
                                    batch.chunk_count, batch.remaining_chunks, batch.observation
                                ),
                            );
                            continue;
                        }
                        non_progress_terminations += 1;
                        let response = no_concrete_finding_response(agent, &reason);
                        trace(AgentTraceEvent::NonProgressTerminated {
                            step,
                            response: response.clone(),
                        });
                        trace(AgentTraceEvent::EvidenceClassified { items: Vec::new() });
                        return Ok(AgentLoopResult {
                            response,
                            evidence_index,
                            review_paths: grounded.review_paths,
                            changed_lines: grounded.changed_lines,
                            relevant_changed_lines: grounded.relevant_changed_lines,
                            review_causal_terms: grounded.review_causal_terms,
                            elapsed: agent_started.elapsed(),
                            llm_calls,
                            native_tool_calls,
                            native_final_calls,
                            text_fallback_turns,
                            tool_cache_hits,
                            tool_cache_misses,
                            non_progress_terminations,
                            coverage_chunks_delivered: coverage.coverage_chunks_delivered(),
                            coverage_pass_rejections: coverage.pass_rejections(),
                        });
                    }
                }
            }
            let executed = execute_tool(
                turn,
                search.as_ref(),
                tool_cache.as_ref(),
                &mut evidence_index,
            )
            .await?;
            if executed.cache_hit {
                tool_cache_hits += 1;
            } else {
                tool_cache_misses += 1;
            }
            trace(AgentTraceEvent::ToolExecuted {
                step,
                tool,
                cache_hit: executed.cache_hit,
                observation: executed.observation.clone(),
            });
            investigation.record(executed.kind, &executed.observation);
            let next_instruction = if investigation.missing_tool_guidance().is_none() {
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

    if investigation.has_content_observation()
        && let Some(response) = deferred_failed_response
        && !failed_verdict_lacks_line_evidence(&response)
        && failed_verdict_lacks_target_path_evidence(
            &response,
            &agent.instruction,
            &grounded.review_paths,
        )
        .is_none()
    {
        trace(AgentTraceEvent::StepLimit {
            response: response.clone(),
        });
        trace(AgentTraceEvent::EvidenceClassified {
            items: classify_evidence(
                &response.evidence,
                &evidence_index,
                &grounded.review_paths,
                &grounded.changed_lines,
                &grounded.relevant_changed_lines,
            ),
        });
        return Ok(AgentLoopResult {
            response,
            evidence_index,
            review_paths: grounded.review_paths,
            changed_lines: grounded.changed_lines,
            relevant_changed_lines: grounded.relevant_changed_lines,
            review_causal_terms: grounded.review_causal_terms,
            elapsed: agent_started.elapsed(),
            llm_calls,
            native_tool_calls,
            native_final_calls,
            text_fallback_turns,
            tool_cache_hits,
            tool_cache_misses,
            non_progress_terminations,
            coverage_chunks_delivered: coverage.coverage_chunks_delivered(),
            coverage_pass_rejections: coverage.pass_rejections(),
        });
    }

    let response = if !coverage.is_complete(inventory.as_ref()) {
        LlmResponse {
            status: TestStatus::Failed,
            severity: agent.severity,
            description: format!(
                "Agent `{}` reached the step limit after reviewing {}/{} review-scope source chunks. Passing is not allowed until every review-scope source chunk has been shown to the agent.",
                agent.id,
                coverage.delivered_chunks(),
                inventory.chunk_count()
            ),
            evidence: Vec::new(),
        }
    } else {
        LlmResponse {
            status: TestStatus::Passed,
            severity: None,
            description: format!(
                "Agent `{}` reached the step limit without a concrete review-scope finding after reviewing all review-scope source chunks.",
                agent.id
            ),
            evidence: Vec::new(),
        }
    };
    trace(AgentTraceEvent::StepLimit {
        response: response.clone(),
    });
    trace(AgentTraceEvent::EvidenceClassified {
        items: classify_evidence(
            &response.evidence,
            &evidence_index,
            &grounded.review_paths,
            &grounded.changed_lines,
            &grounded.relevant_changed_lines,
        ),
    });
    Ok(AgentLoopResult {
        response,
        evidence_index,
        review_paths: grounded.review_paths,
        changed_lines: grounded.changed_lines,
        relevant_changed_lines: grounded.relevant_changed_lines,
        review_causal_terms: grounded.review_causal_terms,
        elapsed: agent_started.elapsed(),
        llm_calls,
        native_tool_calls,
        native_final_calls,
        text_fallback_turns,
        tool_cache_hits,
        tool_cache_misses,
        non_progress_terminations,
        coverage_chunks_delivered: coverage.coverage_chunks_delivered(),
        coverage_pass_rejections: coverage.pass_rejections(),
    })
}

fn direct_verdict_is_grounded(
    response: &LlmResponse,
    relevant_changed_lines: &HashSet<(String, u32)>,
) -> bool {
    match response.status {
        TestStatus::Passed => true,
        TestStatus::Failed => response.evidence.iter().any(|evidence| {
            relevant_changed_lines.contains(&(evidence.path.clone(), evidence.line))
                && substantive_evidence_preview(&evidence.preview)
        }),
    }
}

fn coverage_chunk_json(chunk: &ReviewSourceChunk) -> serde_json::Value {
    json!({
        "chunk_index": chunk.index,
        "path": chunk.path,
        "start_line": chunk.start_line,
        "end_line": chunk.end_line,
        "content": line_numbered_content(&chunk.content, chunk.start_line),
    })
}

fn line_numbered_content(content: &str, start_line: u32) -> String {
    content
        .lines()
        .enumerate()
        .map(|(index, line)| format!("{}: {}", start_line + index as u32, line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn direct_verdict_satisfies_investigation(response: &LlmResponse) -> bool {
    match response.status {
        TestStatus::Passed => true,
        TestStatus::Failed => false,
    }
}

fn failed_verdict_lacks_line_evidence(response: &LlmResponse) -> bool {
    response.status == TestStatus::Failed && response.evidence.is_empty()
}

fn failed_verdict_lacks_substantive_accepted_evidence(
    response: &LlmResponse,
    evidence_index: &HashSet<(String, u32)>,
    review_paths: &HashSet<String>,
    changed_lines: &HashSet<(String, u32)>,
    relevant_changed_lines: &HashSet<(String, u32)>,
) -> bool {
    if response.status != TestStatus::Failed || response.evidence.is_empty() {
        return false;
    }

    let classifications = classify_evidence(
        &response.evidence,
        evidence_index,
        review_paths,
        changed_lines,
        relevant_changed_lines,
    );
    let mut has_accepted_evidence = false;
    for (evidence, classification) in response.evidence.iter().zip(classifications) {
        if !classification.accepted {
            continue;
        }
        has_accepted_evidence = true;
        if substantive_failure_preview(&evidence.preview) {
            return false;
        }
    }

    has_accepted_evidence
}

async fn failed_verdict_has_mismatched_evidence_preview<S>(
    search: &S,
    response: &LlmResponse,
    evidence_index: &HashSet<(String, u32)>,
    review_paths: &HashSet<String>,
    changed_lines: &HashSet<(String, u32)>,
    relevant_changed_lines: &HashSet<(String, u32)>,
) -> Result<Option<String>, AgentError>
where
    S: CodeSearchApi + ?Sized,
    S::Error: Display,
{
    if response.status != TestStatus::Failed {
        return Ok(None);
    }

    let classifications = classify_evidence(
        &response.evidence,
        evidence_index,
        review_paths,
        changed_lines,
        relevant_changed_lines,
    );
    for (evidence, classification) in response.evidence.iter().zip(classifications) {
        if !classification.accepted {
            continue;
        }
        let preview = evidence.preview.trim();
        if preview.is_empty() {
            return Ok(Some(format!(
                "Failed verdict evidence for {}:{} has an empty preview.",
                evidence.path, evidence.line
            )));
        }
        let context = match search
            .get_file_context(GetFileContextRequest {
                path: evidence.path.clone(),
                line: evidence.line,
            })
            .await
        {
            Ok(context) => context,
            Err(_) => {
                return Ok(Some(format!(
                    "Failed verdict evidence cites {}:{}, but that line could not be read.",
                    evidence.path, evidence.line
                )));
            }
        };
        if context.start_line == 0 {
            return Ok(Some(format!(
                "Failed verdict evidence cites {}:{}, but that line could not be read.",
                evidence.path, evidence.line
            )));
        }
        let Some(actual) = context
            .content
            .lines()
            .nth(evidence.line.saturating_sub(context.start_line) as usize)
            .map(str::trim)
        else {
            return Ok(Some(format!(
                "Failed verdict evidence cites {}:{}, but that line is outside the returned file context.",
                evidence.path, evidence.line
            )));
        };
        if !evidence_preview_matches_actual_line(preview, actual) {
            return Ok(Some(format!(
                "Failed verdict evidence preview for {}:{} does not match the actual source line.",
                evidence.path, evidence.line
            )));
        }
    }

    Ok(None)
}

fn evidence_preview_matches_actual_line(preview: &str, actual: &str) -> bool {
    let preview = preview.trim();
    let actual = actual.trim();
    if preview == actual || actual.contains(preview) {
        return true;
    }
    if let Some(prefix) = preview.strip_suffix("...") {
        return actual.starts_with(prefix.trim_end());
    }
    if preview.contains("...") {
        let mut remainder = actual;
        for part in preview
            .split("...")
            .map(str::trim)
            .filter(|part| !part.is_empty())
        {
            let Some(index) = remainder.find(part) else {
                return false;
            };
            remainder = &remainder[index + part.len()..];
        }
        return true;
    }
    false
}

fn failed_verdict_lacks_full_repo_focus_evidence(
    response: &LlmResponse,
    focus_terms: &[String],
) -> bool {
    if response.status != TestStatus::Failed || response.evidence.is_empty() {
        return false;
    }
    let focus_terms = focus_terms
        .iter()
        .filter(|term| term.chars().count() >= 4)
        .collect::<Vec<_>>();
    if focus_terms.len() < 2 {
        return false;
    }
    !response.evidence.iter().any(|evidence| {
        let preview = evidence.preview.to_ascii_lowercase();
        if weak_full_repo_failure_preview(&preview) {
            return false;
        }
        let path = evidence.path.to_ascii_lowercase();
        let preview_matches = focus_terms
            .iter()
            .filter(|term| description_contains_term(&preview, term))
            .count();
        let path_matches = focus_terms
            .iter()
            .filter(|term| description_contains_term(&path, term))
            .count();
        preview_matches >= 2 || (preview_matches >= 1 && path_matches >= 2)
    })
}

fn weak_full_repo_failure_preview(preview: &str) -> bool {
    let trimmed = preview.trim();
    if !substantive_failure_preview(trimmed) {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("(()=>")
        || lower.starts_with("(function(")
        || lower.contains("__webpack_modules__")
    {
        return true;
    }
    let tokens = lower
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if tokens.len() <= 2
        && matches!(
            tokens.first().copied(),
            Some("let" | "const" | "var" | "type" | "interface")
        )
    {
        return true;
    }
    if tokens
        .first()
        .is_some_and(|token| matches!(*token, "let" | "const" | "var"))
        && [
            "allowlist",
            "allowed",
            "config",
            "domains",
            "remote_patterns",
            "remotepatterns",
            "safelist",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return true;
    }
    false
}

fn failed_verdict_is_speculative(response: &LlmResponse) -> bool {
    if response.status != TestStatus::Failed {
        return false;
    }
    let description = response.description.to_ascii_lowercase();
    [
        "appears capable",
        "appears to",
        "could allow",
        "could enable",
        "could execute",
        "could escape",
        "could be surfaced if",
        "could indicate",
        "could influence",
        "does not demonstrate",
        "does not yet confirm",
        "doesn't yet confirm",
        "if altered",
        "if any path",
        "if code path",
        "if metadata",
        "might ",
        "may ",
        "no clear ",
        "not yet confirm",
        "not confirm enforcement",
        "not shown here",
        "potential exposure",
        "potential gap",
        "potential violation",
        "evidence required",
        "requires verifying",
        "requires targeted",
        "required to confirm",
        "required from review-scope",
        "still requires confirmation",
        "targeted evidence",
        "to confirm enforcement",
        "would be if",
        "would indicate",
    ]
    .iter()
    .any(|needle| description.contains(needle))
}

fn failed_verdict_contradicts_no_finding_language(response: &LlmResponse) -> bool {
    if response.status != TestStatus::Failed {
        return false;
    }
    let description = response.description.to_ascii_lowercase();
    failed_verdict_reports_no_concrete_finding(&description)
        || failed_verdict_reports_satisfied_invariant(&description)
        || failed_verdict_reports_coverage_blocked_pass(&description)
}

fn failed_verdict_reports_no_concrete_finding(description: &str) -> bool {
    [
        "no concrete violation",
        "no concrete review-scope violation",
        "no explicit violation",
        "no violation detected",
        "no violation found",
        "no violation has been demonstrated",
        "no violation is demonstrated",
        "no violation is evident",
        "no violation observed",
        "no concrete finding",
        "no concrete issue",
        "no concrete problem",
        "no explicit failing condition",
        "no explicit incorrect",
        "no counterexample",
        "no violating usage",
        "lack of a clear violation",
        "lacks a clear violation",
        "absence of a concrete violation",
        "without a concrete violation",
    ]
    .iter()
    .any(|needle| description.contains(needle))
}

fn failed_verdict_reports_satisfied_invariant(description: &str) -> bool {
    [
        "invariant is satisfied",
        "invariant appears satisfied",
        "invariant satisfied",
        "satisfies the invariant",
        "satisfying the invariant",
        "aligns with the invariant",
        "matches the invariant",
        "target condition is satisfied",
        "current evidence indicates a pass",
        "current evidence confirms direct return",
        "correct final should be passed",
        "correct final should be pass",
        "correct outcome is to report a pass",
        "proper conclusion should be passed",
        "proper conclusion is passed",
        "should be passed",
        "would pass the invariant",
        "the invariant appears satisfied",
        "the invariant is satisfied",
    ]
    .iter()
    .any(|needle| description.contains(needle))
}

fn failed_verdict_reports_coverage_blocked_pass(description: &str) -> bool {
    [
        "because coverage",
        "coverage is incomplete",
        "coverage was incomplete",
        "due to coverage",
        "due to insufficient coverage",
        "incomplete coverage",
        "insufficient coverage",
        "lack full coverage",
        "lacks full coverage",
        "not all chunks",
        "not completed all chunks",
        "until all chunks",
        "until every review-scope source chunk",
        "cannot assert satisfaction",
        "cannot conclusively declare pass",
        "cannot declare pass",
        "cannot declare passed",
        "cannot return passed",
        "must withhold final pass",
        "unable to pass",
        "we cannot declare pass",
        "we cannot declare passed",
    ]
    .iter()
    .any(|needle| description.contains(needle))
}

async fn target_failure_evidence<S>(
    search: &S,
    target_context_line: &Option<(String, u32)>,
    instruction: &str,
) -> Result<Option<Evidence>, AgentError>
where
    S: CodeSearchApi + ?Sized,
    S::Error: Display,
{
    let Some((path, target_line)) = target_context_line.as_ref() else {
        return Ok(None);
    };
    let response = search
        .get_file_context(GetFileContextRequest {
            path: path.clone(),
            line: *target_line,
        })
        .await
        .map_err(|err| AgentError::Search(err.to_string()))?;
    if response.start_line == 0 {
        return Ok(None);
    }

    let lower_instruction = instruction.to_ascii_lowercase();
    let failure_condition = lower_instruction
        .split_once("fail if")
        .map(|(_, condition)| condition)
        .and_then(|condition| condition.split(['.', '\n']).next())
        .unwrap_or(instruction);
    let terms = failure_condition_terms(failure_condition);
    let mut best: Option<(i32, Evidence)> = None;
    let mut first_substantive: Option<Evidence> = None;

    for (index, text) in response.content.lines().enumerate() {
        let line = response.start_line + index as u32;
        if line < *target_line {
            continue;
        }
        let trimmed = text.trim();
        if line > *target_line && starts_new_rust_function(trimmed) {
            break;
        }
        if !substantive_failure_preview(trimmed) {
            continue;
        }

        let evidence = Evidence {
            path: response.path.clone(),
            line,
            preview: trimmed.to_string(),
        };
        first_substantive.get_or_insert_with(|| evidence.clone());

        let lower = trimmed.to_ascii_lowercase();
        let term_score = terms
            .iter()
            .filter(|term| description_contains_term(&lower, term))
            .count() as i32;
        let behavior_score = [
            "format!",
            ".get(",
            ".join(",
            ".map(",
            ".filter(",
            "is_empty",
            "none",
            "requested",
            "global:",
            "return",
        ]
        .iter()
        .filter(|needle| lower.contains(*needle))
        .count() as i32
            * 2;
        let body_score = i32::from(line > *target_line);
        let score = term_score + behavior_score + body_score;
        if best
            .as_ref()
            .is_none_or(|(best_score, _)| score > *best_score)
        {
            best = Some((score, evidence));
        }
    }

    Ok(best.map(|(_, evidence)| evidence).or(first_substantive))
}

fn starts_new_rust_function(trimmed: &str) -> bool {
    trimmed.starts_with("pub fn ")
        || trimmed.starts_with("fn ")
        || trimmed.starts_with("pub async fn ")
        || trimmed.starts_with("async fn ")
}

fn substantive_failure_preview(preview: &str) -> bool {
    let trimmed = preview.trim();
    if !substantive_evidence_preview(trimmed) {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if starts_new_rust_function(&lower) && lower.ends_with('{') {
        return false;
    }
    !matches!(lower.as_str(), "{" | "}" | "};")
}

fn passed_verdict_contradicts_failure_language(response: &LlmResponse, instruction: &str) -> bool {
    if response.status != TestStatus::Passed
        || !instruction.to_ascii_lowercase().contains("fail if")
    {
        return false;
    }
    let lower = response.description.to_ascii_lowercase();
    let decisive_contradictions = [
        "should be marked as failed",
        "should be marked failed",
        "will be marked as failed",
        "would be a failed invariant",
        "this is a failed invariant",
        "correct status is failed",
        "correct verdict is failed",
        "correct verdict should be failed",
        "status should be failed",
        "status must be failed",
        "should be failed",
        "should have failed",
        "should fail",
        "must fail",
        "should report a failure",
        "must report a failure",
        "report a failure",
        "return a failed verdict",
        "should be a failure",
        "should be flagged",
        "constitutes a failed invariant",
        "constitutes a failure",
        "would indicate a failure",
        "would trigger a failure",
        "appropriate verdict would be failed",
        "verdict would be failed",
        "appropriate verdict is failed",
        "verdict should be failed",
        "fail condition is demonstrated",
        "fail condition is triggered",
        "fail condition is met",
        "failure condition is demonstrated",
        "failure condition is triggered",
        "failure condition is met",
        "triggered the fail condition",
        "triggered the failure condition",
        "satisfying the invariant failure condition",
        "satisfies the invariant failure condition",
        "meets the invariant failure condition",
        "satisfying the failure condition",
        "satisfies the failure condition",
        "meets the failure condition",
        "indicates a failure",
        "invariant violated",
        "invariant is violated",
        "violates the invariant",
        "violating the invariant",
        "would violate the invariant",
        "constitutes a violation",
        "implies a violation",
        "would imply a violation",
        "potential violation",
        "violation was found",
    ];
    if decisive_contradictions
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return true;
    }
    if passed_verdict_explicitly_reports_no_finding(&lower)
        || passed_verdict_reports_absent_failure_condition(&lower, instruction)
    {
        return false;
    }
    if passed_verdict_affirms_minimum_that_includes_failure_case(&lower, instruction) {
        return true;
    }
    if passed_verdict_affirms_failure_condition(&lower, instruction) {
        return true;
    }
    if passed_verdict_affirms_no_failure(&lower) {
        return false;
    }
    [
        "unsafe behavior",
        "unsafe pattern",
        "direct unsafe",
        "missing required control",
        "lacks required",
        "contradicts the intended invariant",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn passed_verdict_affirms_failure_condition(description: &str, instruction: &str) -> bool {
    let instruction = instruction.to_ascii_lowercase();
    let Some((_, failure_condition)) = instruction.split_once("fail if") else {
        return false;
    };
    let failure_condition = failure_condition
        .split(['.', '\n'])
        .next()
        .unwrap_or(failure_condition);
    let terms = failure_condition_terms(failure_condition);
    if target_window_affirms_failure_condition(description, &instruction, &terms)
        && (failure_condition_is_negative(failure_condition)
            || !target_window_negates_failure_condition(description, &instruction))
    {
        return true;
    }
    if !failure_condition_is_negative(failure_condition)
        && description_negates_failure_condition(description)
    {
        return false;
    }
    if terms.len() < 2 {
        return false;
    }

    let matched_terms = terms
        .iter()
        .filter(|term| description_contains_term(description, term))
        .count();
    let required_terms = terms.len().min(3);
    matched_terms >= required_terms
}

fn target_window_affirms_failure_condition(
    description: &str,
    instruction: &str,
    terms: &[String],
) -> bool {
    let Some(target) = first_backticked_target(instruction) else {
        return false;
    };
    let Some(start) = description.find(&target) else {
        return false;
    };
    let window = description
        .get(start..)
        .unwrap_or_default()
        .chars()
        .take(260)
        .collect::<String>();
    let matched_terms = terms
        .iter()
        .filter(|term| description_contains_term(&window, term))
        .count();
    matched_terms >= terms.len().min(3)
}

fn target_window_negates_failure_condition(description: &str, instruction: &str) -> bool {
    let Some(target) = first_backticked_target(instruction) else {
        return false;
    };
    let Some(start) = description.find(&target) else {
        return false;
    };
    let window = description
        .get(start..)
        .unwrap_or_default()
        .chars()
        .take(260)
        .collect::<String>();
    description_negates_failure_condition(&window)
}

fn first_backticked_target(instruction: &str) -> Option<String> {
    let mut in_backticks = false;
    let mut fallback = None;
    for part in instruction.split('`') {
        if in_backticks {
            let term = part.trim().to_ascii_lowercase();
            if !term.is_empty() {
                if fallback.is_none() {
                    fallback = Some(term.clone());
                }
                if term.contains('_') && !term.contains('/') && !term.contains('.') {
                    return Some(term);
                }
            }
        }
        in_backticks = !in_backticks;
    }
    fallback
}

fn passed_verdict_affirms_minimum_that_includes_failure_case(
    description: &str,
    instruction: &str,
) -> bool {
    let instruction = instruction.to_ascii_lowercase();
    let Some((_, failure_condition)) = instruction.split_once("fail if") else {
        return false;
    };
    failure_condition.contains("single")
        && (description.contains("at least one")
            || description.contains("non-empty")
            || description.contains("not empty")
            || description.contains("!is_empty"))
}

fn failure_condition_is_negative(value: &str) -> bool {
    [
        "does not",
        "without",
        "omit",
        "omits",
        "omitted",
        "omitting",
        "lack",
        "lacks",
        "missing",
        "ignore",
        "ignores",
        "unbounded",
        "unchecked",
        "no maximum",
        "no service maximum",
    ]
    .iter()
    .any(|needle| value.contains(needle))
}

fn description_negates_failure_condition(description: &str) -> bool {
    if description.contains("no evidence") && description.contains("excludes") {
        return false;
    }
    [
        "does not include",
        "doesn't include",
        "does not use",
        "doesn't use",
        "does not allow",
        "doesn't allow",
        "does not accept",
        "doesn't accept",
        "does not read",
        "doesn't read",
        "does not trust",
        "doesn't trust",
        "does not join",
        "doesn't join",
        "does not log",
        "doesn't log",
        "filters out",
        "filtered out",
        "excludes",
    ]
    .iter()
    .any(|needle| description.contains(needle))
}

fn failure_condition_terms(value: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.chars().count() >= 4)
        .map(|token| token.to_ascii_lowercase())
        .filter(|token| {
            !matches!(
                token.as_str(),
                "changed"
                    | "code"
                    | "concrete"
                    | "demonstrates"
                    | "evidence"
                    | "file"
                    | "function"
                    | "named"
                    | "review"
                    | "scope"
                    | "that"
                    | "this"
                    | "with"
            )
        })
        .filter(|token| seen.insert(token.clone()))
        .collect()
}

fn description_contains_term(description: &str, term: &str) -> bool {
    if description.contains(term) {
        return true;
    }
    if term.len() > 5
        && let Some(stem) = term.strip_suffix("es")
        && stem.len() >= 4
        && description.contains(stem)
    {
        return true;
    }
    if term.len() > 4
        && let Some(singular) = term.strip_suffix('s')
        && singular.len() >= 4
        && description.contains(singular)
    {
        return true;
    }
    false
}

fn passed_verdict_reports_absent_failure_condition(description: &str, instruction: &str) -> bool {
    if [
        "fail condition was not observed",
        "fail condition was not detected",
        "fail condition was not found",
        "fail condition is not observed",
        "fail condition is not detected",
        "fail condition is not found",
        "failure condition was not observed",
        "failure condition was not detected",
        "failure condition was not found",
        "failure condition is not observed",
        "failure condition is not detected",
        "failure condition is not found",
        "violation was not observed",
        "violation was not detected",
        "violation was not found",
        "issue was not observed",
        "issue was not detected",
        "issue was not found",
    ]
    .iter()
    .any(|needle| description.contains(needle))
    {
        return true;
    }

    let instruction = instruction.to_ascii_lowercase();
    let Some((_, failure_condition)) = instruction.split_once("fail if") else {
        return false;
    };
    let failure_condition = failure_condition
        .split(['.', '\n'])
        .next()
        .unwrap_or(failure_condition);
    let terms = failure_condition_terms(failure_condition);

    terms.iter().any(|term| {
        if failure_safeguard_term(term) {
            return false;
        }
        failure_term_variants(term).iter().any(|variant| {
            contains_phrase(description, &format!("no {variant}"))
                || contains_phrase(description, &format!("no `{variant}`"))
                || contains_phrase(description, &format!("{variant} was not observed"))
                || contains_phrase(description, &format!("{variant} were not observed"))
                || contains_phrase(description, &format!("{variant} was not detected"))
                || contains_phrase(description, &format!("{variant} were not detected"))
                || contains_phrase(description, &format!("{variant} was not found"))
                || contains_phrase(description, &format!("{variant} were not found"))
        })
    })
}

fn failure_safeguard_term(term: &str) -> bool {
    matches!(
        term,
        "allowlist"
            | "allowlisting"
            | "bound"
            | "bounded"
            | "check"
            | "checked"
            | "checks"
            | "configuration"
            | "configured"
            | "guard"
            | "guarded"
            | "normalize"
            | "normalized"
            | "redact"
            | "redacted"
            | "redaction"
            | "restrict"
            | "restricted"
            | "restriction"
            | "restrictions"
            | "safe"
            | "safeguard"
            | "safeguards"
            | "sanitize"
            | "sanitized"
            | "validating"
            | "validation"
    )
}

fn failure_term_variants(term: &str) -> Vec<String> {
    let mut variants = vec![term.to_string()];
    if term.len() > 4
        && let Some(singular) = term.strip_suffix('s')
        && singular.len() >= 4
    {
        variants.push(singular.to_string());
    }
    if term.len() > 5
        && let Some(stem) = term.strip_suffix("ed")
        && stem.len() >= 4
    {
        variants.push(stem.to_string());
    }
    variants.sort();
    variants.dedup();
    variants
}

fn contains_phrase(haystack: &str, phrase: &str) -> bool {
    let mut start = 0;
    while let Some(offset) = haystack[start..].find(phrase) {
        let absolute = start + offset;
        let before = haystack[..absolute].chars().next_back();
        let after = haystack[absolute + phrase.len()..].chars().next();
        let before_boundary = before.is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_');
        let after_boundary = after.is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_');
        if before_boundary && after_boundary {
            return true;
        }
        start = absolute + phrase.len();
    }
    false
}

fn passed_verdict_explicitly_reports_no_finding(description: &str) -> bool {
    if description.contains("no evidence") && description.contains("excludes") {
        return false;
    }
    if negated_changed_capability(description) {
        return true;
    }
    [
        "not violated",
        "not found to be violated",
        "no evidence of",
        "no evidence found",
        "no evidence that",
        "no evidence detected",
        "no evidence observed",
        "found no evidence",
        "no concrete evidence",
        "no concrete evidence found",
        "no concrete evidence that",
        "no direct evidence",
        "no direct logic",
        "no concrete change",
        "nothing directly indicating",
        "nothing directly indicates",
        "no concrete violation",
        "no concrete review-scope violation",
        "no concrete failure",
        "no concrete finding",
        "no finding",
    ]
    .iter()
    .any(|needle| description.contains(needle))
}

fn negated_changed_capability(description: &str) -> bool {
    let starts_or_sentence = description.starts_with("no changed ")
        || description.contains(". no changed ")
        || description.contains("; no changed ");
    starts_or_sentence
        && [
            " enables ",
            " enable ",
            " allows ",
            " allow ",
            " can ",
            " could ",
            " would ",
        ]
        .iter()
        .any(|needle| description.contains(needle))
}

fn passed_verdict_affirms_no_failure(description: &str) -> bool {
    if description.contains("no evidence") && description.contains("excludes") {
        return false;
    }
    if negated_changed_capability(description) {
        return true;
    }
    [
        "no unsafe behavior",
        "no unsafe pattern",
        "no direct unsafe",
        "does not violate",
        "doesn't violate",
        "not violate",
        "not violating",
        "does not include",
        "doesn't include",
        "does not use",
        "doesn't use",
        "does not allow",
        "doesn't allow",
        "does not export",
        "doesn't export",
        "does not omit",
        "doesn't omit",
        "filters out",
        "filtered out",
        "excludes",
        "no collision risk",
        "no invariant violation",
        "invariant is not violated",
        "does not indicate a failure",
        "does not satisfy the failure condition",
        "does not meet the failure condition",
        "failure condition is not met",
        "no concrete failure",
        "no concrete finding",
        "no concrete evidence",
        "no finding",
    ]
    .iter()
    .any(|needle| description.contains(needle))
}

fn failed_verdict_lacks_target_path_evidence(
    response: &LlmResponse,
    instruction: &str,
    review_paths: &HashSet<String>,
) -> Option<String> {
    if response.status != TestStatus::Failed || response.evidence.is_empty() {
        return None;
    }
    let target_path = instruction_review_path(instruction, review_paths)?;
    response
        .evidence
        .iter()
        .all(|evidence| evidence.path != target_path)
        .then_some(target_path)
}

fn is_absence_policy(instruction: &str) -> bool {
    let lower = instruction.to_ascii_lowercase();
    lower.contains("doesn't contain")
        || lower.contains("does not contain")
        || lower.contains("missing")
        || lower.contains("absence")
        || lower.contains("no files")
}

fn targeted_rescue_turn(agent: &AgentSpec, grounded: &GroundedRequest) -> Option<AgentTurn> {
    if let Some((path, line)) = &grounded.target_context_line {
        return Some(AgentTurn::GetFileContext {
            path: path.clone(),
            line: *line,
        });
    }
    if let Some(target_path) = instruction_review_path(&agent.instruction, &grounded.review_paths) {
        if let Some((path, line)) = &grounded.focused_context_line
            && path == &target_path
        {
            return Some(AgentTurn::GetFileContext {
                path: path.clone(),
                line: *line,
            });
        }
        return Some(AgentTurn::ReadFile { path: target_path });
    }
    if let Some((path, line)) = &grounded.focused_context_line {
        return Some(AgentTurn::GetFileContext {
            path: path.clone(),
            line: *line,
        });
    }
    None
}

fn is_broad_discovery_turn(turn: &AgentTurn) -> bool {
    matches!(
        turn,
        AgentTurn::ListFiles { .. } | AgentTurn::ListReviewHunks
    )
}

fn instruction_review_path(instruction: &str, review_paths: &HashSet<String>) -> Option<String> {
    let mut in_backticks = false;
    for part in instruction.split('`') {
        if in_backticks {
            let candidate = part.trim();
            if review_paths.contains(candidate) {
                return Some(candidate.to_string());
            }
        }
        in_backticks = !in_backticks;
    }
    None
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

async fn contextualize_llm_error(
    test_id: &str,
    step: usize,
    prompt_tokens: usize,
    prompt: &str,
    diagnostics: &AgentDiagnostics,
    error: LlmBusError,
) -> AgentError {
    if !is_provider_invalid_prompt(&error) {
        return error.into();
    }

    match write_rejected_prompt_dump(test_id, step, prompt_tokens, prompt, diagnostics, &error)
        .await
    {
        Ok(prompt_dump_path) => AgentError::PromptRejected {
            test_id: test_id.to_string(),
            step,
            prompt_tokens,
            prompt_dump_path,
            source: error,
        },
        Err(dump_error) => AgentError::PromptRejectedWithoutDump {
            test_id: test_id.to_string(),
            step,
            prompt_tokens,
            dump_error,
            source: error,
        },
    }
}

fn is_provider_invalid_prompt(error: &LlmBusError) -> bool {
    let LlmBusError::HttpStatus { body, .. } = error else {
        return false;
    };
    let lower = body.to_ascii_lowercase();
    lower.contains("invalid_prompt") || lower.contains("invalid prompt")
}

async fn write_rejected_prompt_dump(
    test_id: &str,
    step: usize,
    prompt_tokens: usize,
    prompt: &str,
    diagnostics: &AgentDiagnostics,
    error: &LlmBusError,
) -> Result<PathBuf, String> {
    let dir = diagnostics
        .prompt_dump_dir
        .as_ref()
        .ok_or_else(|| "prompt dump directory not configured".to_string())?;
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|err| err.to_string())?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let path = dir.join(format!(
        "prompt-rejected-{timestamp}-{}-step-{step}.json",
        safe_file_stem(test_id)
    ));
    let payload = json!({
        "test_id": test_id,
        "step": step,
        "estimated_prompt_tokens": prompt_tokens,
        "prompt_chars": prompt.chars().count(),
        "provider_error": error.to_string(),
        "redaction": "Known API-key and bearer-token shaped substrings are redacted; repository source may otherwise be present for debugging.",
        "prompt_redacted": redact_prompt_for_dump(prompt),
    });
    let json = serde_json::to_string_pretty(&payload).map_err(|err| err.to_string())?;
    tokio::fs::write(&path, json)
        .await
        .map_err(|err| err.to_string())?;
    Ok(path)
}

fn safe_file_stem(value: &str) -> String {
    let stem = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if stem.is_empty() {
        "agent".to_string()
    } else {
        stem
    }
}

fn redact_prompt_for_dump(prompt: &str) -> String {
    prompt
        .lines()
        .map(redact_prompt_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_prompt_line(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    if !lower.contains("bearer ") && !lower.contains("sk-") && !lower.contains("api_key") {
        return line.to_string();
    }
    if let Some(index) = lower.find("bearer ") {
        return format!("{}bearer [REDACTED]", &line[..index]);
    }
    line.split_whitespace()
        .map(redact_prompt_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_prompt_token(token: &str) -> String {
    let trimmed = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | ',' | ';' | ':' | ')' | ']' | '}' | '(' | '[' | '{'
        )
    });
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("sk-") && trimmed.chars().count() > 16 {
        return token.replace(trimmed, "[REDACTED]");
    }
    for key in ["openai_api_key=", "anthropic_api_key=", "api_key="] {
        if let Some(index) = lower.find(key) {
            let prefix_len = index + key.len();
            let (prefix, _) = trimmed.split_at(prefix_len.min(trimmed.len()));
            return token.replace(trimmed, &format!("{prefix}[REDACTED]"));
        }
    }
    token.to_string()
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

fn agent_loop_prompt(test_id: &str, grounded_instruction: &str) -> String {
    format!(
        r#"Agent test id: {test_id}

Status semantics:
- `passed` means the code satisfies the invariant, or no concrete review-scope violation was found.
- `failed` means a concrete review-scope violation was found.
- For `Fail if ...` invariants, the fail condition defines unacceptable behavior. If the named code demonstrates that condition, status MUST be `failed`.
- Do not treat finding the `Fail if` condition as a successful check; finding it is the reason to return `failed`.
- Never use `failed` to mean inconclusive, insufficiently covered, or unable to pass yet. If no concrete violation is found, return `passed`; Koochi will either accept it or provide more review-scope coverage.
- When the instruction names a specific backticked function or file, judge that exact target. Safer or unsafe sibling functions are context only unless the named target calls them.
- Before returning final JSON, make sure `status` agrees with your description and evidence. Never return `passed` while describing a violation, unsafe behavior, missing required control, or triggered fail condition.

{grounded_instruction}

You may either request one tool call or return the final verdict.

Koochi enforces review-scope coverage outside the prompt. You may return `failed` as soon as concrete evidence demonstrates a violation. Koochi will reject `passed` until it has shown this agent every review-scope source chunk: all source files in `--all`/full-repo mode, or changed source files in commit, range, and local-change modes.

Tool call JSON forms:
{{"action":"list_files","kind":"source"}}
{{"action":"list_review_hunks"}}
{{"action":"get_hunk_context","hunk_id":"src/lib.rs#1"}}
{{"action":"search_text","query":"authorization","kind":"source"}}
{{"action":"read_file","path":"src/lib.rs"}}
{{"action":"get_file_context","path":"src/lib.rs","line":42}}
{{"action":"find_definitions","symbol":"handler_name"}}
{{"action":"find_references","symbol":"handler_name"}}

Final verdict JSON forms:
{{"action":"final","status":"passed","severity":null,"description":"...","evidence":[]}}
{{"action":"final","status":"failed","severity":"high","description":"...","evidence":[{{"path":"...","line":1,"preview":"..."}}]}}

Return only JSON. The user-facing test instruction is policy intent, not a tool plan. You decide which search tools to use.

If Koochi included the full review-scope changed hunks above and those hunks are sufficient to show a concrete violation, return a failed verdict after targeted content inspection. Do not call search_text just to rediscover code already shown in the changed-hunk packet. Passed verdicts are allowed only after Koochi's coverage gate has delivered every review-scope source chunk.

When Koochi shows an exact target symbol line or the instruction names a review-scope file, use get_file_context or read_file for that target before broad discovery tools.

If the changed hunks are incomplete, ambiguous, or depend on surrounding/helper/caller behavior, gather concrete evidence with tools:
- Derive search terms from the test id and instruction.
- When Koochi gives review hunk ids, prefer get_hunk_context for targeted commit context before reading an entire file.
- In commit-review mode, prefer list_review_hunks or get_hunk_context before broad list_files/search_text calls.
- Prefer search_text first when the file location is not obvious, then read_file or get_file_context on promising matches.
- Use find_definitions when the test depends on what a helper, wrapper, sanitizer, verifier, cache method, or authorization function does.
- Use find_references when the test depends on whether code is called, dead, or used by a route/export/handler path.
- Use get_file_context when a nearby check matters, such as authorization before a repository call or redaction before logging.

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

#[derive(Debug, Default)]
struct ToolExecutionCache {
    inner: Mutex<ToolExecutionCacheInner>,
}

#[derive(Debug, Default)]
struct ToolExecutionCacheInner {
    entries: HashMap<ToolCacheKey, CachedToolObservation>,
    locks: HashMap<ToolCacheKey, Arc<Mutex<()>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ToolCacheKey {
    ListFiles(FileKind),
    ListReviewHunks,
    GetHunkContext(String),
    SearchText { query: String, kind: FileKind },
    ReadFile(String),
    GetFileContext { path: String, line: u32 },
    FindDefinitions(String),
    FindReferences(String),
}

#[derive(Debug, Clone)]
struct CachedToolObservation {
    kind: ToolKind,
    observation: String,
    evidence_lines: Vec<(String, u32)>,
}

impl ToolExecutionCache {
    async fn get(&self, key: &ToolCacheKey) -> Option<CachedToolObservation> {
        self.inner.lock().await.entries.get(key).cloned()
    }

    async fn lock_for(&self, key: &ToolCacheKey) -> Arc<Mutex<()>> {
        self.inner
            .lock()
            .await
            .locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    async fn insert(&self, key: ToolCacheKey, value: CachedToolObservation) {
        self.inner.lock().await.entries.insert(key, value);
    }
}

#[derive(Debug, Default)]
struct NonProgressState {
    tool_calls: HashMap<ToolCacheKey, usize>,
    broad_without_content: usize,
    warned: bool,
}

enum NonProgressDecision {
    Warn(String),
    Terminate(String),
}

impl NonProgressState {
    fn record(
        &mut self,
        turn: &AgentTurn,
        has_content_observation: bool,
    ) -> Option<NonProgressDecision> {
        let key = tool_cache_key_for_agent_turn(turn)?;
        let count = self.tool_calls.entry(key.clone()).or_default();
        *count += 1;
        let repeated = *count > 1;
        let broad_without_content = matches!(
            key,
            ToolCacheKey::ListFiles(_) | ToolCacheKey::ListReviewHunks
        ) && !has_content_observation;
        if broad_without_content {
            self.broad_without_content += 1;
        }
        if repeated || self.broad_without_content > 3 {
            if self.warned && !has_content_observation {
                return Some(NonProgressDecision::Terminate(format!(
                    "Repeated non-progress tool use ({}) without concrete content inspection.",
                    tool_cache_key_label(&key)
                )));
            }
            self.warned = true;
            return Some(NonProgressDecision::Warn(format!(
                "Tool `{}` has already produced the same kind of broad or repeated observation. Use a targeted content tool such as get_hunk_context, get_file_context, or read_file, or return passed if there is no concrete finding.",
                tool_cache_key_label(&key)
            )));
        }
        None
    }
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
    let repaired = [
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
    });
    repaired
        .replace(r#""line_preview""#, r#""preview""#)
        .replace(r#""linePreview""#, r#""preview""#)
        .replace(r#""code_preview""#, r#""preview""#)
        .replace(r#""codePreview""#, r#""preview""#)
}

fn extract_json_object(content: &str) -> Option<&str> {
    let start = content.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, ch) in content[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    let end = start + offset + ch.len_utf8();
                    return Some(&content[start..end]);
                }
            }
            _ => {}
        }
    }

    None
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
    cache: &ToolExecutionCache,
    evidence_index: &mut HashSet<(String, u32)>,
) -> Result<ExecutedTool, AgentError>
where
    S: CodeSearchApi + ?Sized,
    S::Error: Display,
{
    let tool = turn_to_tool(turn)?;
    let key = tool_cache_key(&tool);
    if let Some(cached) = cache.get(&key).await {
        for line in &cached.evidence_lines {
            evidence_index.insert(line.clone());
        }
        return Ok(ExecutedTool::from_cached(cached, true));
    }
    let lock = cache.lock_for(&key).await;
    let _guard = lock.lock().await;
    if let Some(cached) = cache.get(&key).await {
        for line in &cached.evidence_lines {
            evidence_index.insert(line.clone());
        }
        return Ok(ExecutedTool::from_cached(cached, true));
    }
    let executed = execute_tool_uncached(tool, search).await?;
    for line in &executed.evidence_lines {
        evidence_index.insert(line.clone());
    }
    cache.insert(key, executed.cached_observation()).await;
    Ok(executed)
}

pub async fn build_review_scope_inventory<S>(search: &S) -> Result<ReviewScopeInventory, AgentError>
where
    S: CodeSearchApi + ?Sized,
    S::Error: Display,
{
    let mut files = search
        .list_review_files(ListFilesRequest {
            kind: FileKind::Source,
        })
        .await
        .map_err(|err| AgentError::Search(err.to_string()))?
        .files;
    files.sort();
    files.dedup();
    let file_count = files.len();
    let mut line_count = 0u64;
    let mut byte_count = 0u64;
    let mut chunks = Vec::new();

    for path in files {
        let response = search
            .read_file(ReadFileRequest { path: path.clone() })
            .await
            .map_err(|err| AgentError::Search(err.to_string()))?;
        line_count += response.line_count as u64;
        byte_count += response.content.len() as u64;
        let lines = response.content.lines().collect::<Vec<_>>();
        for (chunk_index, chunk_lines) in lines.chunks(REVIEW_COVERAGE_CHUNK_LINES).enumerate() {
            let start_line = (chunk_index * REVIEW_COVERAGE_CHUNK_LINES + 1) as u32;
            let end_line = start_line + chunk_lines.len() as u32 - 1;
            chunks.push(ReviewSourceChunk {
                index: chunks.len(),
                path: response.path.clone(),
                start_line,
                end_line,
                content: chunk_lines.join("\n"),
            });
        }
    }

    Ok(ReviewScopeInventory {
        file_count,
        line_count,
        byte_count,
        chunks,
    })
}

async fn execute_full_repo_rescue<S>(
    search: &S,
    cache: &ToolExecutionCache,
    evidence_index: &mut HashSet<(String, u32)>,
    terms: &mut VecDeque<String>,
) -> Result<Option<Vec<ExecutedTool>>, AgentError>
where
    S: CodeSearchApi + ?Sized,
    S::Error: Display,
{
    let Some(term) = terms.pop_front() else {
        return Ok(None);
    };
    let search_result = execute_tool(
        AgentTurn::SearchText {
            query: term,
            kind: Some("source".to_string()),
        },
        search,
        cache,
        evidence_index,
    )
    .await?;
    let first_match = first_search_match(&search_result.observation);
    let mut executed = vec![search_result];
    if let Some((path, line)) = first_match {
        executed.push(
            execute_tool(
                AgentTurn::GetFileContext { path, line },
                search,
                cache,
                evidence_index,
            )
            .await?,
        );
    }
    Ok(Some(executed))
}

fn first_search_match(observation: &str) -> Option<(String, u32)> {
    let value: serde_json::Value = serde_json::from_str(observation).ok()?;
    let first = value.get("matches")?.as_array()?.first()?;
    let path = first.get("path")?.as_str()?.to_string();
    let line = first.get("line")?.as_u64()? as u32;
    Some((path, line))
}

async fn execute_tool_uncached<S>(tool: ToolTurn, search: &S) -> Result<ExecutedTool, AgentError>
where
    S: CodeSearchApi + ?Sized,
    S::Error: Display,
{
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
                Vec::new(),
            ))
        }
        ToolTurn::ListReviewHunks => {
            let response = search
                .list_review_hunks()
                .await
                .map_err(|err| AgentError::Search(err.to_string()))?;
            let mut evidence_lines = Vec::new();
            for hunk in &response.hunks {
                for line in &hunk.lines {
                    if let Some(line) = line.new_line.or(line.old_line) {
                        evidence_lines.push((hunk.path.clone(), line));
                    }
                }
            }
            Ok(ExecutedTool::new(
                ToolKind::ListReviewHunks,
                json!({"hunks": review_hunk_summaries(&response.hunks)}).to_string(),
                evidence_lines,
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
                        Vec::new(),
                    ));
                }
            };
            let mut evidence_lines = Vec::new();
            if response.start_line > 0 {
                for line in response.start_line..=response.end_line {
                    evidence_lines.push((response.path.clone(), line));
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
                evidence_lines,
            ))
        }
        ToolTurn::SearchText { query, kind } => {
            let response = search
                .search_text(SearchTextRequest { query, kind })
                .await
                .map_err(|err| AgentError::Search(err.to_string()))?;
            let mut evidence_lines = Vec::new();
            for m in &response.matches {
                evidence_lines.push((m.path.clone(), m.line));
            }
            Ok(ExecutedTool::new(
                ToolKind::SearchText,
                json!({"matches": response.matches.into_iter().take(MAX_SEARCH_MATCHES).collect::<Vec<_>>()})
                    .to_string(),
                evidence_lines,
            ))
        }
        ToolTurn::ReadFile { path } => {
            let response = match search
                .read_file(ReadFileRequest { path: path.clone() })
                .await
            {
                Ok(response) => response,
                Err(err) => {
                    return Ok(ExecutedTool::new(
                        ToolKind::ReadFile,
                        json!({"path": path, "error": err.to_string()}).to_string(),
                        Vec::new(),
                    ));
                }
            };
            let evidence_lines = (1..=response.line_count)
                .map(|line| (response.path.clone(), line))
                .collect::<Vec<_>>();
            Ok(ExecutedTool::new(
                ToolKind::ReadFile,
                json!({
                    "path": response.path,
                    "line_count": response.line_count,
                    "content": response.content.lines().take(MAX_READ_FILE_LINES).collect::<Vec<_>>().join("\n")
                })
                .to_string(),
                evidence_lines,
            ))
        }
        ToolTurn::GetFileContext { path, line } => {
            let response = match search
                .get_file_context(GetFileContextRequest {
                    path: path.clone(),
                    line,
                })
                .await
            {
                Ok(response) => response,
                Err(err) => {
                    return Ok(ExecutedTool::new(
                        ToolKind::GetFileContext,
                        json!({"path": path, "line": line, "error": err.to_string()}).to_string(),
                        Vec::new(),
                    ));
                }
            };
            let mut evidence_lines = Vec::new();
            if response.start_line > 0 {
                for line in response.start_line..=response.end_line {
                    evidence_lines.push((response.path.clone(), line));
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
                evidence_lines,
            ))
        }
        ToolTurn::FindDefinitions { symbol } => {
            let response = search
                .find_definitions(FindDefinitionsRequest { symbol })
                .await
                .map_err(|err| AgentError::Search(err.to_string()))?;
            let mut evidence_lines = Vec::new();
            for m in &response.definitions {
                evidence_lines.push((m.path.clone(), m.line));
            }
            Ok(ExecutedTool::new(
                ToolKind::FindDefinitions,
                json!({"definitions": response.definitions}).to_string(),
                evidence_lines,
            ))
        }
        ToolTurn::FindReferences { symbol } => {
            let response = search
                .find_references(FindReferencesRequest { symbol })
                .await
                .map_err(|err| AgentError::Search(err.to_string()))?;
            let mut evidence_lines = Vec::new();
            for m in &response.references {
                evidence_lines.push((m.path.clone(), m.line));
            }
            Ok(ExecutedTool::new(
                ToolKind::FindReferences,
                json!({"references": response.references.into_iter().take(MAX_REFERENCE_MATCHES).collect::<Vec<_>>()})
                    .to_string(),
                evidence_lines,
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
                "preview_lines": review_hunk_preview_lines(hunk),
            })
        })
        .collect()
}

fn review_hunk_preview_lines(hunk: &ReviewHunk) -> Vec<serde_json::Value> {
    hunk.lines
        .iter()
        .filter(|line| substantive_changed_line(&line.content))
        .filter_map(|line| {
            let line_number = match line.kind {
                ReviewLineKind::Added | ReviewLineKind::Context => line.new_line,
                ReviewLineKind::Removed => line.old_line,
            }?;
            let kind = match line.kind {
                ReviewLineKind::Added => "added",
                ReviewLineKind::Removed => "removed",
                ReviewLineKind::Context => "context",
            };
            Some(json!({
                "kind": kind,
                "line": line_number,
                "content": line.content.trim(),
            }))
        })
        .take(MAX_HUNK_SUMMARY_PREVIEW_LINES)
        .collect()
}

struct ExecutedTool {
    kind: ToolKind,
    observation: String,
    evidence_lines: Vec<(String, u32)>,
    cache_hit: bool,
}

impl ExecutedTool {
    fn new(kind: ToolKind, observation: String, evidence_lines: Vec<(String, u32)>) -> Self {
        Self {
            kind,
            observation,
            evidence_lines,
            cache_hit: false,
        }
    }

    fn from_cached(cached: CachedToolObservation, cache_hit: bool) -> Self {
        Self {
            kind: cached.kind,
            observation: cached.observation,
            evidence_lines: cached.evidence_lines,
            cache_hit,
        }
    }

    fn cached_observation(&self) -> CachedToolObservation {
        CachedToolObservation {
            kind: self.kind,
            observation: self.observation.clone(),
            evidence_lines: self.evidence_lines.clone(),
        }
    }

    fn tool_label(&self) -> &'static str {
        match self.kind {
            ToolKind::ReviewCoverage => "review_scope_coverage",
            ToolKind::ListFiles => "list_files",
            ToolKind::ListReviewHunks => "list_review_hunks",
            ToolKind::GetHunkContext => "get_hunk_context",
            ToolKind::SearchText => "search_text",
            ToolKind::ReadFile => "read_file",
            ToolKind::GetFileContext => "get_file_context",
            ToolKind::FindDefinitions => "find_definitions",
            ToolKind::FindReferences => "find_references",
        }
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

fn tool_cache_key(tool: &ToolTurn) -> ToolCacheKey {
    match tool {
        ToolTurn::ListFiles { kind } => ToolCacheKey::ListFiles(*kind),
        ToolTurn::ListReviewHunks => ToolCacheKey::ListReviewHunks,
        ToolTurn::GetHunkContext { hunk_id } => ToolCacheKey::GetHunkContext(hunk_id.clone()),
        ToolTurn::SearchText { query, kind } => ToolCacheKey::SearchText {
            query: query.trim().to_string(),
            kind: *kind,
        },
        ToolTurn::ReadFile { path } => ToolCacheKey::ReadFile(normalize_tool_path(path)),
        ToolTurn::GetFileContext { path, line } => ToolCacheKey::GetFileContext {
            path: normalize_tool_path(path),
            line: *line,
        },
        ToolTurn::FindDefinitions { symbol } => {
            ToolCacheKey::FindDefinitions(symbol.trim().to_string())
        }
        ToolTurn::FindReferences { symbol } => {
            ToolCacheKey::FindReferences(symbol.trim().to_string())
        }
    }
}

fn tool_cache_key_for_agent_turn(turn: &AgentTurn) -> Option<ToolCacheKey> {
    match turn {
        AgentTurn::ListFiles { kind } => {
            Some(ToolCacheKey::ListFiles(parse_file_kind(kind.clone())))
        }
        AgentTurn::ListReviewHunks => Some(ToolCacheKey::ListReviewHunks),
        AgentTurn::GetHunkContext { hunk_id } => {
            Some(ToolCacheKey::GetHunkContext(hunk_id.clone()))
        }
        AgentTurn::SearchText { query, kind } => Some(ToolCacheKey::SearchText {
            query: query.trim().to_string(),
            kind: parse_file_kind(kind.clone()),
        }),
        AgentTurn::ReadFile { path } => Some(ToolCacheKey::ReadFile(normalize_tool_path(path))),
        AgentTurn::GetFileContext { path, line } => Some(ToolCacheKey::GetFileContext {
            path: normalize_tool_path(path),
            line: *line,
        }),
        AgentTurn::FindDefinitions { symbol } => {
            Some(ToolCacheKey::FindDefinitions(symbol.trim().to_string()))
        }
        AgentTurn::FindReferences { symbol } => {
            Some(ToolCacheKey::FindReferences(symbol.trim().to_string()))
        }
        AgentTurn::Final { .. } => None,
    }
}

fn tool_cache_key_label(key: &ToolCacheKey) -> String {
    match key {
        ToolCacheKey::ListFiles(kind) => format!("list_files({kind:?})"),
        ToolCacheKey::ListReviewHunks => "list_review_hunks".to_string(),
        ToolCacheKey::GetHunkContext(hunk_id) => format!("get_hunk_context({hunk_id})"),
        ToolCacheKey::SearchText { query, kind } => format!("search_text({query:?}, {kind:?})"),
        ToolCacheKey::ReadFile(path) => format!("read_file({path})"),
        ToolCacheKey::GetFileContext { path, line } => {
            format!("get_file_context({path}:{line})")
        }
        ToolCacheKey::FindDefinitions(symbol) => format!("find_definitions({symbol})"),
        ToolCacheKey::FindReferences(symbol) => format!("find_references({symbol})"),
    }
}

fn normalize_tool_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_string()
}

fn substantive_evidence_preview(preview: &str) -> bool {
    grounding::substantive_changed_line(preview)
}

fn no_concrete_finding_response(agent: &AgentSpec, reason: &str) -> LlmResponse {
    LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description: format!(
            "No concrete review-scope finding for `{}`: {reason}",
            agent.id
        ),
        evidence: Vec::new(),
    }
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod runner_tests;
