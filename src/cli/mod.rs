use crate::Severity;
use crate::agents::AgentProgressEvent;
use crate::agents::run_agents_with_progress;
use crate::config::ConfigError;
use crate::config::KoochiConfig;
use crate::config::discover_config;
use crate::llm::LlmBusError;
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
            "Koochi: max_parallel_agents={}, max_agent_steps={}, max_parallel_llm_requests={}, llm_max_retries={}",
            config.max_parallel_agents,
            config.max_agent_steps,
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
    println!("Running {} agentic tests", scope.agents.len());
    let mut debug_stats = DebugRunStats::default();
    let verdicts = run_agents_with_progress(
        scope.agents.clone(),
        search.clone(),
        built_bus.bus.clone(),
        config.max_parallel_agents,
        config.max_agent_steps,
        |event| {
            debug_stats.record(event);
            if cli.verbose {
                print_agent_progress(event);
            }
        },
    )
    .await?;
    if cli.verbose {
        println!("Koochi: synthesizing results");
    }
    let report = synthesize_results(verdicts);
    print_report(&report, started.elapsed());
    if cli.debug {
        let search_stats = search.stats();
        let llm_bus_stats = built_bus.stats();
        print_debug_report(&debug_stats, llm_bus_stats, search_stats);
        let debug_path = write_debug_log(
            &scope.primary_repo.root,
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

fn print_agent_progress(event: AgentProgressEvent) {
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
        AgentProgressEvent::BatchCompleted {
            batch_index,
            batch_count,
            agent_count,
            llm_calls,
            llm_elapsed,
            ..
        } => println!(
            "Koochi: completed batch {batch_index}/{batch_count} ({agent_count} agents, {llm_calls} LLM calls, LLM {})",
            format_duration(llm_elapsed)
        ),
    }
}

fn review_scope_line(review: &crate::scope::ReviewScope) -> String {
    match review.mode {
        ReviewMode::HeadCommit => {
            if let Some(commit) = &review.commit {
                format!(
                    "Koochi: {} {} ({} files)",
                    commit.short_id,
                    commit.subject,
                    review.files.len()
                )
            } else {
                format!("Koochi: HEAD ({} files)", review.files.len())
            }
        }
        ReviewMode::LocalChanges => format!("Koochi: local changes ({} files)", review.files.len()),
        ReviewMode::FullRepoFallback => format!("Koochi: full repo fallback"),
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
    fn record(&mut self, event: AgentProgressEvent) {
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
                self.llm_elapsed += llm_elapsed;
            }
            AgentProgressEvent::BatchPreparing { .. }
            | AgentProgressEvent::BatchCallingLlm { .. } => {}
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
    println!(
        "  LLM actions: {} native tool calls, {} native final verdicts, {} text fallback turns",
        debug.native_tool_calls, debug.native_final_calls, debug.text_fallback_turns
    );
    println!(
        "  search tools: {} calls total (list_files {}, list_review_files {}, read_file {}, get_file_context {}, search_text {}, definitions {}, references {})",
        total_search_calls,
        list_files_calls,
        list_review_files_calls,
        read_file_calls,
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
    llm: DebugLlmLog,
    search: DebugSearchLog,
}

#[derive(Debug, serde::Serialize)]
struct DebugLlmLog {
    turns: usize,
    agent_batches: usize,
    provider_calls: usize,
    retry_attempts: usize,
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
    get_file_context_calls: usize,
    search_text_calls: usize,
    definition_calls: usize,
    reference_calls: usize,
    cache: SearchStatsSnapshot,
}

async fn write_debug_log(
    repo_root: &std::path::Path,
    debug: &DebugRunStats,
    llm_bus: ManagedLlmBusStatsSnapshot,
    search: SearchStatsSnapshot,
    report: &SynthesisReport,
    elapsed: Duration,
) -> Result<PathBuf, CliError> {
    let log = build_debug_log(debug, llm_bus, search, report, elapsed);
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
        + search.get_file_context_calls
        + search_text_calls
        + definition_calls
        + reference_calls;
    DebugLog {
        elapsed_ms: elapsed.as_millis(),
        passed: report.passed.len(),
        failed: report.failed.len(),
        llm: DebugLlmLog {
            turns: debug.llm_turns,
            agent_batches: debug.agent_batches,
            provider_calls: llm_bus.provider_calls,
            retry_attempts: llm_bus.retry_attempts,
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
            get_file_context_calls: search.get_file_context_calls,
            search_text_calls,
            definition_calls,
            reference_calls,
            cache: search,
        },
    }
}

fn print_report(report: &SynthesisReport, elapsed: Duration) {
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
    let summary = format!(
        "Finished in {}: {}/{} passed, {} failed",
        format_duration(elapsed),
        report.passed.len(),
        total,
        report.failed.len()
    );
    println!("{}", color_for_status(status, &summary));
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
            commit: Some(CommitInfo {
                short_id: "abc1234".to_string(),
                subject: "tighten review scope".to_string(),
            }),
        });

        assert_eq!(line, "Koochi: abc1234 tighten review scope (2 files)");
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
}
