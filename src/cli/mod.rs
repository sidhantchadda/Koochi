use crate::Severity;
use crate::agents::AgentProgressEvent;
use crate::agents::AgentTraceEvent;
use crate::agents::run_agent_with_trace;
use crate::agents::run_agents_with_progress;
use crate::config::ConfigError;
use crate::config::KoochiConfig;
use crate::config::discover_config;
use crate::llm::LlmBusError;
use crate::llm::LlmTokenUsage;
use crate::llm::ManagedLlmBusStatsSnapshot;
use crate::llm::build_llm_bus;
use crate::scope::ReviewMode;
use crate::scope::ScopeError;
use crate::scope::build_scope;
use crate::search::LocalSearchSession;
use crate::search::SearchStatsSnapshot;
use crate::synthesis::SynthesisReport;
use crate::synthesis::synthesize_results;
use clap::Parser;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

#[derive(Debug, Parser)]
#[command(name = "koochi", about = "Run lightweight parallel agentic tests.")]
pub struct Cli {
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long)]
    pub json_output: Option<PathBuf>,
    #[arg(short, long)]
    pub verbose: bool,
    #[arg(short, long)]
    pub debug: bool,
    #[arg(short = 't', long)]
    pub trace: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Scope(#[from] ScopeError),
    #[error(transparent)]
    LlmBus(#[from] LlmBusError),
    #[error(transparent)]
    Agent(#[from] crate::agents::AgentError),
    #[error("failed to write JSON report `{path}`: {source}")]
    WriteJson {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to write debug log `{path}`: {source}")]
    WriteDebug {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to serialize JSON report: {0}")]
    Serialize(serde_json::Error),
    #[error("trace test id `{0}` was not found in config")]
    TraceTestNotFound(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunExit {
    Success,
    TestFailures,
}

pub async fn run(cli: Cli) -> Result<RunExit, CliError> {
    let started = Instant::now();
    if cli.verbose {
        println!("Koochi: starting");
    }
    let config_path = match cli.config {
        Some(path) => path,
        None => discover_config(std::env::current_dir().map_err(|source| ConfigError::Read {
            path: PathBuf::from("."),
            source,
        })?)?,
    };
    if cli.verbose {
        println!("Koochi: using config {}", config_path.display());
    }
    let config = KoochiConfig::from_path(&config_path)?;
    if cli.verbose {
        println!(
            "Koochi: provider {:?}, model {}",
            config.provider, config.model
        );
        println!(
            "Koochi: max_parallel_agents={}, max_agent_steps={}, initial_context_token_budget={}, max_parallel_llm_requests={}, llm_max_retries={}",
            config.max_parallel_agents,
            config.max_agent_steps,
            config.initial_context_token_budget,
            config.max_parallel_llm_requests,
            config.llm_max_retries
        );
    }
    let config_dir = config_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let scope = build_scope(&config, config_dir)?;
    println!("{}", review_scope_line(&scope.review));
    if cli.verbose {
        println!(
            "Koochi: scoped repo {} at {:?}",
            scope.primary_repo.root.display(),
            scope.primary_repo.revision
        );
        println!(
            "Koochi: review scope {:?} ({} files)",
            scope.review.mode,
            scope.review.files.len()
        );
    }
    let search = Arc::new(LocalSearchSession::new(scope.clone()));
    let built_bus = build_llm_bus(&config)?;
    if let Some(test_id) = &cli.trace {
        let agent = scope
            .agents
            .iter()
            .find(|agent| &agent.id == test_id)
            .cloned()
            .ok_or_else(|| CliError::TraceTestNotFound(test_id.clone()))?;
        println!("Tracing 1 agentic test: {}", agent.id);
        let mut trace_debug_stats = DebugRunStats::default();
        trace_debug_stats.agent_batches = 1;
        let trace_started = Instant::now();
        let verdict = run_agent_with_trace(
            agent,
            search.clone(),
            built_bus.bus.clone(),
            config.max_agent_steps,
            |event| {
                trace_debug_stats.record_trace(&event);
                print_trace_event(event, cli.verbose);
            },
        )
        .await?;
        trace_debug_stats.llm_elapsed = trace_started.elapsed();
        let llm_bus_stats = built_bus.stats();
        let report = synthesize_results(vec![verdict]);
        print_report(&report, started.elapsed(), llm_bus_stats.token_usage);
        if cli.debug {
            let search_stats = search.stats();
            print_debug_report(&trace_debug_stats, llm_bus_stats, search_stats);
        }
        return Ok(if report.has_failures() {
            RunExit::TestFailures
        } else {
            RunExit::Success
        });
    }

    println!("Running {} agentic tests", scope.agents.len());
    let mut debug_stats = DebugRunStats::default();
    let verdicts = run_agents_with_progress(
        scope.agents.clone(),
        search.clone(),
        built_bus.bus.clone(),
        config.max_parallel_agents,
        config.max_agent_steps,
        |event| {
            debug_stats.record(&event);
            match event {
                AgentProgressEvent::AgentCompleted { .. }
                | AgentProgressEvent::ProgressTick { .. } => {
                    print_live_agent_progress(&event, cli.verbose)
                }
                _ if cli.verbose => {
                    clear_live_agent_progress();
                    print_agent_progress(&event);
                }
                _ => {}
            }
        },
    )
    .await?;
    clear_live_agent_progress();
    if cli.verbose {
        println!("Koochi: synthesizing results");
    }
    let llm_bus_stats = built_bus.stats();
    let report = synthesize_results(verdicts);
    print_report(&report, started.elapsed(), llm_bus_stats.token_usage);
    if cli.debug {
        let search_stats = search.stats();
        print_debug_report(&debug_stats, llm_bus_stats, search_stats);
        let debug_path = write_debug_log(
            &scope.primary_repo.root,
            &scope.review,
            config.initial_context_token_budget,
            &debug_stats,
            llm_bus_stats,
            search_stats,
            &report,
            started.elapsed(),
        )
        .await?;
        println!("  debug log: {}", debug_path.display());
    }

    if let Some(path) = cli.json_output {
        if cli.verbose {
            println!("Koochi: writing JSON report {}", path.display());
        }
        let json = serde_json::to_string_pretty(&report).map_err(CliError::Serialize)?;
        tokio::fs::write(&path, json)
            .await
            .map_err(|source| CliError::WriteJson { path, source })?;
    }

    Ok(if report.has_failures() {
        RunExit::TestFailures
    } else {
        RunExit::Success
    })
}

fn print_agent_progress(event: &AgentProgressEvent) {
    match event {
        AgentProgressEvent::BatchPreparing {
            batch_index,
            batch_count,
            agent_count,
        } => println!("Koochi: preparing batch {batch_index}/{batch_count} ({agent_count} agents)"),
        AgentProgressEvent::BatchCallingLlm {
            batch_index,
            batch_count,
            agent_count,
        } => println!(
            "Koochi: running LLM loop for batch {batch_index}/{batch_count} ({agent_count} agents)"
        ),
        AgentProgressEvent::AgentCompleted {
            test_id,
            completed_agents,
            total_agents,
            running_agent_ids,
        } => println!(
            "{completed_agents}/{total_agents} test agents completed. Last finished: {test_id}. Still running: {}",
            running_agent_ids.join(", ")
        ),
        AgentProgressEvent::ProgressTick { .. } => {}
        AgentProgressEvent::BatchCompleted {
            batch_index,
            batch_count,
            agent_count,
            llm_calls,
            llm_elapsed,
            ..
        } => println!(
            "Koochi: completed batch {batch_index}/{batch_count} ({agent_count} agents, {llm_calls} LLM calls, LLM {})",
            format_duration(*llm_elapsed)
        ),
    }
}

fn print_live_agent_progress(event: &AgentProgressEvent, verbose: bool) {
    let (completed_agents, total_agents, running_agent_ids) = match event {
        AgentProgressEvent::AgentCompleted {
            completed_agents,
            total_agents,
            running_agent_ids,
            ..
        }
        | AgentProgressEvent::ProgressTick {
            completed_agents,
            total_agents,
            running_agent_ids,
        } => (*completed_agents, *total_agents, running_agent_ids),
        _ => return,
    };
    let spinner = live_spinner();
    let mut line = format!("{spinner} {completed_agents}/{total_agents} test agents completed.");
    if verbose && !running_agent_ids.is_empty() {
        let running = running_agent_ids
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let remaining = running_agent_ids.len().saturating_sub(8);
        if remaining > 0 {
            line.push_str(&format!(" Still running: {running}, +{remaining} more"));
        } else {
            line.push_str(&format!(" Still running: {running}"));
        }
    }
    print!("\r\x1b[2K{line}");
    let _ = std::io::stdout().flush();
}

fn live_spinner() -> &'static str {
    let tick = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() / 150)
        .unwrap_or_default();
    ["|", "/", "-", "\\"][(tick as usize) % 4]
}

fn clear_live_agent_progress() {
    print!("\r\x1b[2K");
    let _ = std::io::stdout().flush();
}

fn print_trace_event(event: AgentTraceEvent, verbose: bool) {
    match event {
        AgentTraceEvent::Started { test_id, max_steps } => {
            println!("trace: started {test_id} (max {max_steps} steps)");
        }
        AgentTraceEvent::StepStarted {
            step,
            prompt_tokens,
            prompt,
        } => {
            println!();
            println!("trace: step {step} ({prompt_tokens} tokens)");
            if verbose {
                println!("  {}", cyan("input prompt:"));
                println!(
                    "{}",
                    dim(&indent_for_trace(&middle_truncate_for_trace(
                        &prompt, 12_000
                    )))
                );
            }
        }
        AgentTraceEvent::LlmAction {
            step: _,
            action,
            output,
        } => {
            println!("  llm: {action}");
            if verbose {
                println!("  {}", green("model output:"));
                println!(
                    "{}",
                    indent_for_trace(&middle_truncate_for_trace(&output, 6_000))
                );
            }
        }
        AgentTraceEvent::InvalidResponse { step: _, content } => {
            println!("  {}", yellow("rejected: invalid provider response"));
            println!("    {}", compact_for_trace(&content, 1200));
        }
        AgentTraceEvent::PrematureFinal { step: _, guidance } => {
            println!("  {}", yellow("rejected: premature final verdict"));
            println!("    {}", compact_for_trace(&guidance, 1200));
        }
        AgentTraceEvent::EvidenceClassified { items } => {
            if verbose && !items.is_empty() {
                println!("  {}", dim("evidence classification:"));
                for item in items {
                    let label = match item.classification {
                        crate::agents::EvidenceClassification::Changed => green("changed-line"),
                        crate::agents::EvidenceClassification::ReviewContext => {
                            yellow("review-context")
                        }
                        crate::agents::EvidenceClassification::OutsideReview => {
                            red("outside-review")
                        }
                    };
                    let verdict = if item.accepted {
                        "accepted"
                    } else {
                        "rejected"
                    };
                    println!("    - {}:{} {label} {verdict}", item.path, item.line);
                }
            }
        }
        AgentTraceEvent::ToolExecuted {
            step: _,
            tool,
            observation,
        } => {
            println!("  tool: {tool}");
            println!("  observation: {}", summarize_observation(&observation));
        }
        AgentTraceEvent::FinalVerdict { step: _, response } => {
            println!(
                "  final: {:?} severity={:?} evidence={}",
                response.status,
                response.severity,
                response.evidence.len()
            );
            println!("    {}", response.description);
        }
        AgentTraceEvent::StepLimit { response } => {
            println!("  step limit: {:?}", response.status);
            println!("    {}", response.description);
        }
    }
}

fn summarize_observation(observation: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(observation) else {
        return compact_for_trace(observation, 1200);
    };
    if let Some(files) = value.get("files").and_then(|value| value.as_array()) {
        return format!("{} files: {}", files.len(), preview_json_items(files, 8));
    }
    if let Some(matches) = value.get("matches").and_then(|value| value.as_array()) {
        return format!(
            "{} matches: {}",
            matches.len(),
            preview_locations(matches, 8)
        );
    }
    if let Some(hunks) = value.get("hunks").and_then(|value| value.as_array()) {
        return format!("{} hunks: {}", hunks.len(), preview_hunks(hunks, 8));
    }
    if let Some(definitions) = value.get("definitions").and_then(|value| value.as_array()) {
        return format!(
            "{} definitions: {}",
            definitions.len(),
            preview_locations(definitions, 8)
        );
    }
    if let Some(references) = value.get("references").and_then(|value| value.as_array()) {
        return format!(
            "{} references: {}",
            references.len(),
            preview_locations(references, 8)
        );
    }
    if let Some(path) = value.get("path").and_then(|value| value.as_str()) {
        let line_count = value
            .get("line_count")
            .and_then(|value| value.as_u64())
            .map(|line_count| format!("{line_count} lines"))
            .or_else(|| {
                let start = value.get("start_line")?.as_u64()?;
                let end = value.get("end_line")?.as_u64()?;
                Some(format!("lines {start}-{end}"))
            })
            .unwrap_or_else(|| "file content".to_string());
        if let Some(hunk_id) = value.get("hunk_id").and_then(|value| value.as_str()) {
            return format!("{path} {hunk_id} ({line_count})");
        }
        return format!("{path} ({line_count})");
    }
    compact_for_trace(observation, 1200)
}

fn preview_hunks(items: &[serde_json::Value], limit: usize) -> String {
    let shown = items
        .iter()
        .take(limit)
        .map(|item| {
            let id = item
                .get("id")
                .and_then(|value| value.as_str())
                .unwrap_or("?");
            let path = item
                .get("path")
                .and_then(|value| value.as_str())
                .unwrap_or("?");
            format!("{id} {path}")
        })
        .collect::<Vec<_>>()
        .join("; ");
    let remaining = items.len().saturating_sub(limit);
    if remaining > 0 {
        format!("{shown}; +{remaining} more")
    } else {
        shown
    }
}

fn preview_locations(items: &[serde_json::Value], limit: usize) -> String {
    let shown = items
        .iter()
        .take(limit)
        .map(|item| {
            let path = item
                .get("path")
                .and_then(|value| value.as_str())
                .unwrap_or("?");
            let line = item
                .get("line")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let preview = item
                .get("preview")
                .and_then(|value| value.as_str())
                .map(|preview| format!(" {}", compact_for_trace(preview, 90)))
                .unwrap_or_default();
            format!("{path}:{line}{preview}")
        })
        .collect::<Vec<_>>()
        .join("; ");
    let remaining = items.len().saturating_sub(limit);
    if remaining > 0 {
        format!("{shown}; +{remaining} more")
    } else {
        shown
    }
}

fn preview_json_items(items: &[serde_json::Value], limit: usize) -> String {
    let shown = items
        .iter()
        .take(limit)
        .map(|item| {
            item.as_str()
                .map(ToString::to_string)
                .unwrap_or_else(|| item.to_string())
        })
        .collect::<Vec<_>>()
        .join(", ");
    let remaining = items.len().saturating_sub(limit);
    if remaining > 0 {
        format!("{shown}, +{remaining} more")
    } else {
        shown
    }
}

fn compact_for_trace(value: &str, max_chars: usize) -> String {
    let mut compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() > max_chars {
        compact = compact.chars().take(max_chars).collect::<String>();
        compact.push_str("...");
    }
    compact
}

fn middle_truncate_for_trace(value: &str, max_chars: usize) -> String {
    let char_count = value.chars().count();
    if char_count <= max_chars {
        return value.to_string();
    }
    let edge_chars = max_chars.saturating_sub(160) / 2;
    let start = value.chars().take(edge_chars).collect::<String>();
    let end = value
        .chars()
        .rev()
        .take(edge_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!(
        "{start}\n\n... trace prompt truncated: {} chars omitted ...\n\n{end}",
        char_count.saturating_sub(edge_chars * 2)
    )
}

fn indent_for_trace(value: &str) -> String {
    value
        .lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn review_scope_line(review: &crate::scope::ReviewScope) -> String {
    let changed_loc = review_changed_loc(review);
    let changed_loc = format_changed_loc(changed_loc);
    match review.mode {
        ReviewMode::HeadCommit => {
            if let Some(commit) = &review.commit {
                format!(
                    "Koochi: {} {} ({changed_loc})",
                    commit.short_id, commit.subject
                )
            } else {
                format!("Koochi: HEAD ({changed_loc})")
            }
        }
        ReviewMode::LocalChanges => format!("Koochi: local changes ({changed_loc})"),
        ReviewMode::FullRepoFallback => format!("Koochi: full repo fallback"),
    }
}

fn review_changed_loc(review: &crate::scope::ReviewScope) -> usize {
    review
        .hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .filter(|line| {
            matches!(
                line.kind,
                crate::scope::ReviewLineKind::Added | crate::scope::ReviewLineKind::Removed
            )
        })
        .count()
}

fn format_changed_loc(changed_loc: usize) -> String {
    match changed_loc {
        1 => "1 LOC changed".to_string(),
        count => format!("{count} LOC changed"),
    }
}

#[derive(Debug, Default)]
struct DebugRunStats {
    agent_batches: usize,
    llm_turns: usize,
    native_tool_calls: usize,
    native_final_calls: usize,
    text_fallback_turns: usize,
    llm_elapsed: Duration,
}

impl DebugRunStats {
    fn record(&mut self, event: &AgentProgressEvent) {
        match event {
            AgentProgressEvent::BatchCompleted {
                llm_elapsed,
                llm_calls,
                native_tool_calls,
                native_final_calls,
                text_fallback_turns,
                ..
            } => {
                self.agent_batches += 1;
                self.llm_turns += llm_calls;
                self.native_tool_calls += native_tool_calls;
                self.native_final_calls += native_final_calls;
                self.text_fallback_turns += text_fallback_turns;
                self.llm_elapsed += *llm_elapsed;
            }
            AgentProgressEvent::BatchPreparing { .. }
            | AgentProgressEvent::BatchCallingLlm { .. }
            | AgentProgressEvent::AgentCompleted { .. }
            | AgentProgressEvent::ProgressTick { .. } => {}
        }
    }

    fn record_trace(&mut self, event: &AgentTraceEvent) {
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
    }
}

fn print_debug_report(
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

async fn write_debug_log(
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

fn print_report(report: &SynthesisReport, elapsed: Duration, token_usage: LlmTokenUsage) {
    let total = report.passed.len() + report.failed.len();
    println!();
    for verdict in &report.failed {
        let severity = severity_label(verdict.severity);
        println!(
            "- [{}] {}: {}",
            severity, verdict.test_id, verdict.description
        );
        if verdict.evidence.is_empty() {
            println!("  {} none returned", dim("evidence:"));
        } else {
            println!("  {}", dim("evidence:"));
            for evidence in &verdict.evidence {
                println!(
                    "    - {}:{} {}",
                    cyan(&evidence.path),
                    yellow(&evidence.line.to_string()),
                    dim(&evidence.preview)
                );
            }
        }
    }
    if !report.failed.is_empty() {
        println!();
    }

    let status = summary_status(report);
    let token_suffix = if token_usage.total_tokens > 0 {
        format!(", {} tokens used", format_count(token_usage.total_tokens))
    } else {
        String::new()
    };
    let summary = format!(
        "Finished in {}: {}/{} passed, {} failed{}",
        format_duration(elapsed),
        report.passed.len(),
        total,
        report.failed.len(),
        token_suffix
    );
    println!("{}", color_for_status(status, &summary));
}

fn format_count(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(ch);
    }
    formatted.chars().rev().collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SummaryStatus {
    Pass,
    Warning,
    Error,
}

fn summary_status(report: &SynthesisReport) -> SummaryStatus {
    if report.failed.is_empty() {
        SummaryStatus::Pass
    } else if report
        .failed
        .iter()
        .any(|verdict| matches!(verdict.severity, Some(Severity::High | Severity::Critical)))
    {
        SummaryStatus::Error
    } else {
        SummaryStatus::Warning
    }
}

fn severity_label(severity: Option<Severity>) -> String {
    match severity {
        Some(Severity::Critical) => red("Critical"),
        Some(Severity::High) => red("High"),
        Some(Severity::Medium) => yellow("Medium"),
        Some(Severity::Low) => cyan("Low"),
        None => dim("Unknown"),
    }
}

fn color_for_status(status: SummaryStatus, text: &str) -> String {
    match status {
        SummaryStatus::Pass => green(text),
        SummaryStatus::Warning => yellow(text),
        SummaryStatus::Error => red(text),
    }
}

fn format_duration(duration: Duration) -> String {
    if duration.as_secs() > 0 {
        format!("{:.2}s", duration.as_secs_f64())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

fn green(text: &str) -> String {
    ansi("32", text)
}

fn yellow(text: &str) -> String {
    ansi("33", text)
}

fn red(text: &str) -> String {
    ansi("31", text)
}

fn cyan(text: &str) -> String {
    ansi("36", text)
}

fn dim(text: &str) -> String {
    ansi("2", text)
}

fn ansi(code: &str, text: &str) -> String {
    format!("\x1b[{code}m{text}\x1b[0m")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::{CommitInfo, ReviewMode, ReviewScope};

    #[tokio::test]
    async fn run_writes_json_output() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("koochi.toml"),
            r#"
            tests = ["Simple pass"]
            "#,
        )
        .unwrap();
        let output = temp.path().join("report.json");
        let exit = run(Cli {
            config: Some(temp.path().join("koochi.toml")),
            json_output: Some(output.clone()),
            verbose: false,
            debug: false,
            trace: None,
        })
        .await
        .unwrap();
        assert_eq!(exit, RunExit::Success);
        assert!(output.exists());
    }

    #[test]
    fn formats_head_commit_scope_line() {
        let line = review_scope_line(&ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["src/lib.rs".to_string(), "README.md".to_string()],
            hunks: vec![crate::scope::ReviewHunk {
                id: "src/lib.rs#1".to_string(),
                path: "src/lib.rs".to_string(),
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 2,
                lines: vec![
                    crate::scope::ReviewHunkLine {
                        kind: crate::scope::ReviewLineKind::Removed,
                        old_line: Some(1),
                        new_line: None,
                        content: "old".to_string(),
                    },
                    crate::scope::ReviewHunkLine {
                        kind: crate::scope::ReviewLineKind::Added,
                        old_line: None,
                        new_line: Some(1),
                        content: "new".to_string(),
                    },
                    crate::scope::ReviewHunkLine {
                        kind: crate::scope::ReviewLineKind::Added,
                        old_line: None,
                        new_line: Some(2),
                        content: "newer".to_string(),
                    },
                ],
            }],
            commit: Some(CommitInfo {
                short_id: "abc1234".to_string(),
                subject: "tighten review scope".to_string(),
            }),
        });

        assert_eq!(line, "Koochi: abc1234 tighten review scope (3 LOC changed)");
    }

    #[tokio::test]
    async fn debug_mode_writes_debug_log() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("lib.rs"), "pub fn handler() {}\n").unwrap();
        std::fs::write(
            temp.path().join("koochi.toml"),
            r#"
            provider = "fake"
            tests = ["Simple pass"]
            "#,
        )
        .unwrap();
        let exit = run(Cli {
            config: Some(temp.path().join("koochi.toml")),
            json_output: None,
            verbose: false,
            debug: true,
            trace: None,
        })
        .await
        .unwrap();

        assert_eq!(exit, RunExit::Success);
        let debug_dir = temp.path().join(".koochi").join("debug");
        let logs = std::fs::read_dir(&debug_dir)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(logs.len(), 1);
        let json = std::fs::read_to_string(logs[0].path()).unwrap();
        assert!(json.contains(r#""total_calls""#));
        assert!(json.contains(r#""list_files_calls""#));
    }

    #[tokio::test]
    async fn trace_mode_runs_only_named_agent() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("lib.rs"), "pub fn handler() {}\n").unwrap();
        std::fs::write(
            temp.path().join("koochi.toml"),
            r#"
            provider = "fake"

            [[test]]
            id = "selected"
            instruction = "Simple pass"

            [[test]]
            id = "not-selected"
            instruction = "Check missing retry handling."
            "#,
        )
        .unwrap();
        let exit = run(Cli {
            config: Some(temp.path().join("koochi.toml")),
            json_output: None,
            verbose: false,
            debug: false,
            trace: Some("selected".to_string()),
        })
        .await
        .unwrap();

        assert_eq!(exit, RunExit::Success);
    }
}
