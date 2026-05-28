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
    coverage_source_files: usize,
    coverage_loc: u64,
    coverage_loc_label: &'static str,
    coverage_bytes: u64,
    coverage_chunks: usize,
    coverage_chunk_line_limit: usize,
    coverage_chunks_delivered: usize,
    coverage_pass_rejections: usize,
    agent_runs: Vec<AgentRunDebugStats>,
    pub(crate) llm_elapsed: Duration,
}

impl DebugRunStats {
    pub(crate) fn set_inventory(&mut self, inventory: &crate::agents::ReviewScopeInventory) {
        self.coverage_source_files = inventory.file_count();
        self.coverage_loc = inventory.line_count();
        self.coverage_loc_label = inventory.coverage_loc_label();
        self.coverage_bytes = inventory.byte_count();
        self.coverage_chunks = inventory.chunk_count();
        self.coverage_chunk_line_limit = inventory.chunk_line_limit();
    }

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
                coverage_chunks_delivered,
                coverage_pass_rejections,
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
                self.coverage_chunks_delivered += coverage_chunks_delivered;
                self.coverage_pass_rejections += coverage_pass_rejections;
                self.llm_elapsed += *llm_elapsed;
            }
            AgentProgressEvent::AgentCompleted { debug_stats, .. } => {
                if let Some(debug_stats) = debug_stats {
                    self.agent_runs.push(debug_stats.clone());
                }
            }
            AgentProgressEvent::BatchPreparing { .. }
            | AgentProgressEvent::BatchCallingLlm { .. }
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
        if matches!(event, AgentTraceEvent::FailureAdjudicated { .. }) {
            self.llm_turns += 1;
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
        if let AgentTraceEvent::ReviewCoverageDelivered {
            delivered_chunks, ..
        } = event
        {
            self.coverage_chunks_delivered = *delivered_chunks;
        }
        if matches!(event, AgentTraceEvent::PassCoverageRejected { .. }) {
            self.coverage_pass_rejections += 1;
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
    if debug.coverage_source_files > 0 {
        println!(
            "  review coverage: {} source files, {} {}, {} chunks ({} lines/chunk)",
            format_count(debug.coverage_source_files as u64),
            format_count(debug.coverage_loc),
            debug.coverage_loc_label,
            format_count(debug.coverage_chunks as u64),
            debug.coverage_chunk_line_limit
        );
        println!(
            "  agent coverage: {} chunks delivered, {} pass verdicts rejected before full coverage",
            format_count(debug.coverage_chunks_delivered as u64),
            format_count(debug.coverage_pass_rejections as u64)
        );
    }
    if !debug.agent_runs.is_empty() {
        print_agent_analytics(debug);
    }
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
    agents: DebugAgentsLog,
}

#[derive(Debug, serde::Serialize)]
struct DebugReviewLog {
    hunk_count: usize,
    initial_packet_mode: &'static str,
    initial_packet_tokens: usize,
    coverage: DebugCoverageLog,
}

#[derive(Debug, serde::Serialize)]
struct DebugCoverageLog {
    source_files: usize,
    loc: u64,
    loc_label: &'static str,
    bytes: u64,
    chunks: usize,
    chunk_line_limit: usize,
    chunks_delivered: usize,
    pass_rejections: usize,
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

#[derive(Debug, serde::Serialize)]
struct DebugAgentsLog {
    total: usize,
    console_truncated_per_agent_table: bool,
    aggregate: DebugAgentAggregateLog,
    runs: Vec<AgentRunDebugStats>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
struct DebugAgentAggregateLog {
    prompts: DebugMetricSummary,
    tool_calls: DebugMetricSummary,
    elapsed_ms: DebugMetricSummary,
    unique_loc_read: DebugMetricSummary,
    coverage_chunks_delivered: DebugMetricSummary,
    cache_hits: DebugMetricSummary,
    cache_misses: DebugMetricSummary,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
struct DebugMetricSummary {
    min: f64,
    max: f64,
    mean: f64,
    stddev: f64,
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
            coverage: DebugCoverageLog {
                source_files: debug.coverage_source_files,
                loc: debug.coverage_loc,
                loc_label: debug.coverage_loc_label,
                bytes: debug.coverage_bytes,
                chunks: debug.coverage_chunks,
                chunk_line_limit: debug.coverage_chunk_line_limit,
                chunks_delivered: debug.coverage_chunks_delivered,
                pass_rejections: debug.coverage_pass_rejections,
            },
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
        agents: build_agents_log(debug),
    }
}

fn print_agent_analytics(debug: &DebugRunStats) {
    let aggregate = build_agent_aggregate(&debug.agent_runs);
    println!(
        "  agent analytics: {} agents measured",
        format_count(debug.agent_runs.len() as u64)
    );
    print_metric("prompts/agent", &aggregate.prompts);
    print_metric("tools/agent", &aggregate.tool_calls);
    print_metric("elapsed ms/agent", &aggregate.elapsed_ms);
    print_metric("unique LOC read/agent", &aggregate.unique_loc_read);
    print_metric(
        "coverage chunks/agent",
        &aggregate.coverage_chunks_delivered,
    );
    if debug.agent_runs.len() <= 32 {
        println!("  per-agent:");
        println!(
            "    {:<32} {:<6} {:>7} {:>7} {:>9} {:>9}",
            "id", "status", "prompts", "tools", "loc_read", "elapsed"
        );
        for run in &debug.agent_runs {
            println!(
                "    {:<32} {:<6} {:>7} {:>7} {:>9} {:>8}ms",
                truncate_agent_id(&run.test_id),
                status_label(run.status),
                run.llm_calls,
                run.native_tool_calls,
                run.unique_loc_read,
                run.elapsed_ms
            );
        }
    } else {
        println!(
            "  per-agent table omitted in console ({} agents > 32); full rows are in the debug JSON.",
            debug.agent_runs.len()
        );
    }
}

fn print_metric(label: &str, summary: &DebugMetricSummary) {
    println!(
        "    {label}: min {:.2}, max {:.2}, mean {:.2}, stddev {:.2}",
        summary.min, summary.max, summary.mean, summary.stddev
    );
}

fn build_agents_log(debug: &DebugRunStats) -> DebugAgentsLog {
    DebugAgentsLog {
        total: debug.agent_runs.len(),
        console_truncated_per_agent_table: debug.agent_runs.len() > 32,
        aggregate: build_agent_aggregate(&debug.agent_runs),
        runs: debug.agent_runs.clone(),
    }
}

fn build_agent_aggregate(runs: &[AgentRunDebugStats]) -> DebugAgentAggregateLog {
    DebugAgentAggregateLog {
        prompts: metric_summary(runs.iter().map(|run| run.llm_calls as f64)),
        tool_calls: metric_summary(runs.iter().map(|run| run.native_tool_calls as f64)),
        elapsed_ms: metric_summary(runs.iter().map(|run| run.elapsed_ms as f64)),
        unique_loc_read: metric_summary(runs.iter().map(|run| run.unique_loc_read as f64)),
        coverage_chunks_delivered: metric_summary(
            runs.iter().map(|run| run.coverage_chunks_delivered as f64),
        ),
        cache_hits: metric_summary(runs.iter().map(|run| run.tool_cache_hits as f64)),
        cache_misses: metric_summary(runs.iter().map(|run| run.tool_cache_misses as f64)),
    }
}

fn metric_summary(values: impl IntoIterator<Item = f64>) -> DebugMetricSummary {
    let values = values.into_iter().collect::<Vec<_>>();
    if values.is_empty() {
        return DebugMetricSummary::default();
    }
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|value| {
            let delta = value - mean;
            delta * delta
        })
        .sum::<f64>()
        / values.len() as f64;
    DebugMetricSummary {
        min,
        max,
        mean,
        stddev: variance.sqrt(),
    }
}

fn truncate_agent_id(value: &str) -> String {
    const MAX: usize = 32;
    if value.chars().count() <= MAX {
        return value.to_string();
    }
    let keep = MAX.saturating_sub(3);
    format!("{}...", value.chars().take(keep).collect::<String>())
}

fn status_label(status: crate::llm::TestStatus) -> &'static str {
    match status {
        crate::llm::TestStatus::Passed => "pass",
        crate::llm::TestStatus::Failed => "fail",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn row(index: usize) -> AgentRunDebugStats {
        AgentRunDebugStats {
            test_id: format!("agent-{index}"),
            status: crate::llm::TestStatus::Passed,
            elapsed_ms: index as u128,
            llm_calls: index,
            native_tool_calls: index + 1,
            native_final_calls: 1,
            text_fallback_turns: 0,
            tool_cache_hits: index + 2,
            tool_cache_misses: index + 3,
            non_progress_terminations: 0,
            coverage_chunks_delivered: index + 4,
            coverage_pass_rejections: 0,
            unique_loc_read: index + 5,
            review_scope_loc: 100,
            tool_counts: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn metric_summary_reports_min_max_mean_and_stddev() {
        let summary = metric_summary([1.0, 2.0, 3.0]);

        assert_eq!(summary.min, 1.0);
        assert_eq!(summary.max, 3.0);
        assert_eq!(summary.mean, 2.0);
        assert!((summary.stddev - (2.0_f64 / 3.0).sqrt()).abs() < 0.000_001);
    }

    #[test]
    fn agents_log_marks_console_truncation_after_thirty_two_agents() {
        let mut debug = DebugRunStats::default();
        debug.agent_runs = (0..32).map(row).collect();
        assert!(!build_agents_log(&debug).console_truncated_per_agent_table);

        debug.agent_runs.push(row(32));
        let log = build_agents_log(&debug);
        assert!(log.console_truncated_per_agent_table);
        assert_eq!(log.total, 33);
        assert_eq!(log.runs.len(), 33);
    }
}
