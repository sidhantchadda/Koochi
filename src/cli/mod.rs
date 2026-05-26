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

mod debug;
mod render;

use debug::{DebugRunStats, print_debug_report, write_debug_log};
use render::{
    clear_live_agent_progress, print_agent_progress, print_live_agent_progress, print_report,
    print_trace_event, review_scope_line,
};

#[derive(Debug, Parser)]
#[command(
    name = "koochi",
    about = "Run lightweight parallel agentic invariants."
)]
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
        println!("Tracing 1 agentic invariant: {}", agent.id);
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

    println!("Running {} agentic invariants", scope.agents.len());
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
