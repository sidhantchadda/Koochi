use super::render::{dim, format_count, format_duration};
use super::*;

#[derive(Debug, Default)]
pub(crate) struct DebugRunStats {
    pub(crate) agent_batches: usize,
    llm_turns: usize,
    native_tool_calls: usize,
    native_final_calls: usize,
    text_fallback_turns: usize,
    tool_cache_hits: usize,
    tool_cache_misses: usize,
    non_progress_terminations: usize,
    pub(crate) llm_elapsed: Duration,
}

impl DebugRunStats {
    pub(crate) fn record(&mut self, event: &AgentProgressEvent) {
        match event {
            AgentProgressEvent::BatchCompleted {
                llm_elapsed,
                llm_calls,
                native_tool_calls,
                native_final_calls,
                text_fallback_turns,
                tool_cache_hits,
                tool_cache_misses,
                non_progress_terminations,
                ..
            } => {
                self.agent_batches += 1;
                self.llm_turns += llm_calls;
                self.native_tool_calls += native_tool_calls;
                self.native_final_calls += native_final_calls;
                self.text_fallback_turns += text_fallback_turns;
                self.tool_cache_hits += tool_cache_hits;
                self.tool_cache_misses += tool_cache_misses;
                self.non_progress_terminations += non_progress_terminations;
                self.llm_elapsed += *llm_elapsed;
            }
            AgentProgressEvent::BatchPreparing { .. }
            | AgentProgressEvent::BatchCallingLlm { .. }
            | AgentProgressEvent::AgentCompleted { .. }
            | AgentProgressEvent::ProgressTick { .. } => {}
        }
    }

    pub(crate) fn record_trace(&mut self, event: &AgentTraceEvent) {
        if let AgentTraceEvent::LlmAction { action, .. } = event {
            self.llm_turns += 1;
            if action.starts_with("tool ") {
                self.native_tool_calls += 1;
            } else if action.starts_with("final ") {
                self.native_final_calls += 1;
            } else if action.starts_with("text ") {
                self.text_fallback_turns += 1;
            }
        }
        if let AgentTraceEvent::ToolExecuted { cache_hit, .. } = event {
            if *cache_hit {
                self.tool_cache_hits += 1;
            } else {
                self.tool_cache_misses += 1;
            }
        }
        if matches!(event, AgentTraceEvent::NonProgressTerminated { .. }) {
            self.non_progress_terminations += 1;
        }
    }
}

pub(crate) fn print_debug_report(
    debug: &DebugRunStats,
    llm_bus: ManagedLlmBusStatsSnapshot,
    search: SearchStatsSnapshot,
) {
    println!();
    println!("{}", dim("debug performance:"));
    let list_files_calls = search.list_files_hits + search.list_files_misses;
    let list_review_files_calls = search.list_review_files_hits + search.list_review_files_misses;
    let read_file_calls = search.read_file_hits + search.read_file_misses;
    let search_text_calls = search.search_text_hits + search.search_text_misses;
    let definition_calls = search.definition_hits + search.definition_misses;
    let reference_calls = search.reference_hits + search.reference_misses;
    let total_search_calls = list_files_calls
        + list_review_files_calls
        + read_file_calls
        + search.get_hunk_context_calls
        + search.get_file_context_calls
        + search_text_calls
        + definition_calls
        + reference_calls;
    println!(
        "  LLM: {} turns across {} agent batches, {} provider calls ({} retries), total invocation time {}",
        debug.llm_turns,
        debug.agent_batches,
        llm_bus.provider_calls,
        llm_bus.retry_attempts,
        format_duration(debug.llm_elapsed)
    );
    if llm_bus.token_usage.total_tokens > 0 {
        println!(
            "  LLM tokens: {} total ({} prompt, {} completion)",
            format_count(llm_bus.token_usage.total_tokens),
            format_count(llm_bus.token_usage.prompt_tokens),
            format_count(llm_bus.token_usage.completion_tokens)
        );
    }
    println!(
        "  LLM actions: {} native tool calls, {} native final verdicts, {} text fallback turns",
        debug.native_tool_calls, debug.native_final_calls, debug.text_fallback_turns
    );
    println!(
        "  agent guardrails: {} tool-cache hits / {} misses, {} non-progress terminations",
        debug.tool_cache_hits, debug.tool_cache_misses, debug.non_progress_terminations
    );
    println!(
        "  search tools: {} calls total (list_files {}, list_review_files {}, read_file {}, get_hunk_context {}, get_file_context {}, search_text {}, definitions {}, references {})",
        total_search_calls,
        list_files_calls,
        list_review_files_calls,
        read_file_calls,
        search.get_hunk_context_calls,
        search.get_file_context_calls,
        search_text_calls,
        definition_calls,
        reference_calls
    );
    println!(
        "  search cache: list_files {} hits / {} misses, list_review_files {} hits / {} misses, read_file {} hits / {} misses",
        search.list_files_hits,
        search.list_files_misses,
        search.list_review_files_hits,
        search.list_review_files_misses,
        search.read_file_hits,
        search.read_file_misses
    );
    println!(
        "  search cache: search_text {} hits / {} misses, definitions {} hits / {} misses, references {} hits / {} misses",
        search.search_text_hits,
        search.search_text_misses,
        search.definition_hits,
        search.definition_misses,
        search.reference_hits,
        search.reference_misses
    );
}

#[derive(Debug, serde::Serialize)]
struct DebugLog {
    elapsed_ms: u128,
    passed: usize,
    failed: usize,
    review: DebugReviewLog,
    llm: DebugLlmLog,
    search: DebugSearchLog,
}

#[derive(Debug, serde::Serialize)]
struct DebugReviewLog {
    hunk_count: usize,
    initial_packet_mode: &'static str,
    initial_packet_tokens: usize,
}

#[derive(Debug, serde::Serialize)]
struct DebugLlmLog {
    turns: usize,
    agent_batches: usize,
    provider_calls: usize,
    retry_attempts: usize,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    native_tool_calls: usize,
    native_final_calls: usize,
    text_fallback_turns: usize,
    tool_cache_hits: usize,
    tool_cache_misses: usize,
    non_progress_terminations: usize,
    elapsed_ms: u128,
}

#[derive(Debug, serde::Serialize)]
struct DebugSearchLog {
    total_calls: usize,
    list_files_calls: usize,
    list_review_files_calls: usize,
    read_file_calls: usize,
    get_hunk_context_calls: usize,
    get_file_context_calls: usize,
    search_text_calls: usize,
    definition_calls: usize,
    reference_calls: usize,
    cache: SearchStatsSnapshot,
}

pub(crate) async fn write_debug_log(
    repo_root: &std::path::Path,
    review: &crate::scope::ReviewScope,
    initial_context_token_budget: usize,
    debug: &DebugRunStats,
    llm_bus: ManagedLlmBusStatsSnapshot,
    search: SearchStatsSnapshot,
    report: &SynthesisReport,
    elapsed: Duration,
) -> Result<PathBuf, CliError> {
    let log = build_debug_log(
        review,
        initial_context_token_budget,
        debug,
        llm_bus,
        search,
        report,
        elapsed,
    );
    let dir = repo_root.join(".koochi").join("debug");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|source| CliError::WriteDebug {
            path: dir.clone(),
            source,
        })?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let path = dir.join(format!("run-{timestamp}.json"));
    let json = serde_json::to_string_pretty(&log).map_err(CliError::Serialize)?;
    tokio::fs::write(&path, json)
        .await
        .map_err(|source| CliError::WriteDebug {
            path: path.clone(),
            source,
        })?;
    Ok(path)
}

fn build_debug_log(
    review: &crate::scope::ReviewScope,
    initial_context_token_budget: usize,
    debug: &DebugRunStats,
    llm_bus: ManagedLlmBusStatsSnapshot,
    search: SearchStatsSnapshot,
    report: &SynthesisReport,
    elapsed: Duration,
) -> DebugLog {
    let list_files_calls = search.list_files_hits + search.list_files_misses;
    let list_review_files_calls = search.list_review_files_hits + search.list_review_files_misses;
    let read_file_calls = search.read_file_hits + search.read_file_misses;
    let search_text_calls = search.search_text_hits + search.search_text_misses;
    let definition_calls = search.definition_hits + search.definition_misses;
    let reference_calls = search.reference_hits + search.reference_misses;
    let total_calls = list_files_calls
        + list_review_files_calls
        + read_file_calls
        + search.get_hunk_context_calls
        + search.get_file_context_calls
        + search_text_calls
        + definition_calls
        + reference_calls;
    DebugLog {
        elapsed_ms: elapsed.as_millis(),
        passed: report.passed.len(),
        failed: report.failed.len(),
        review: DebugReviewLog {
            hunk_count: review.hunks.len(),
            initial_packet_mode: initial_packet_mode(review, initial_context_token_budget),
            initial_packet_tokens: estimate_review_packet_tokens(review),
        },
        llm: DebugLlmLog {
            turns: debug.llm_turns,
            agent_batches: debug.agent_batches,
            provider_calls: llm_bus.provider_calls,
            retry_attempts: llm_bus.retry_attempts,
            prompt_tokens: llm_bus.token_usage.prompt_tokens,
            completion_tokens: llm_bus.token_usage.completion_tokens,
            total_tokens: llm_bus.token_usage.total_tokens,
            native_tool_calls: debug.native_tool_calls,
            native_final_calls: debug.native_final_calls,
            text_fallback_turns: debug.text_fallback_turns,
            tool_cache_hits: debug.tool_cache_hits,
            tool_cache_misses: debug.tool_cache_misses,
            non_progress_terminations: debug.non_progress_terminations,
            elapsed_ms: debug.llm_elapsed.as_millis(),
        },
        search: DebugSearchLog {
            total_calls,
            list_files_calls,
            list_review_files_calls,
            read_file_calls,
            get_hunk_context_calls: search.get_hunk_context_calls,
            get_file_context_calls: search.get_file_context_calls,
            search_text_calls,
            definition_calls,
            reference_calls,
            cache: search,
        },
    }
}

fn estimate_review_packet_tokens(review: &crate::scope::ReviewScope) -> usize {
    let chars = review
        .hunks
        .iter()
        .map(|hunk| {
            let line_chars = hunk
                .lines
                .iter()
                .map(|line| line.content.chars().count() + 8)
                .sum::<usize>();
            hunk.id.chars().count() + hunk.path.chars().count() + line_chars + 32
        })
        .sum::<usize>();
    chars.div_ceil(4).max(1)
}

fn initial_packet_mode(
    review: &crate::scope::ReviewScope,
    initial_context_token_budget: usize,
) -> &'static str {
    if review.hunks.is_empty()
        || estimate_review_packet_tokens(review) <= initial_context_token_budget
    {
        "full"
    } else {
        "summary"
    }
}
