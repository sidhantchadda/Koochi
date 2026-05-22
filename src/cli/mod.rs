use crate::agents::run_agents;
use crate::config::ConfigError;
use crate::config::KoochiConfig;
use crate::config::discover_config;
use crate::llm::LlmBusError;
use crate::llm::build_llm_bus;
use crate::scope::ScopeError;
use crate::scope::build_scope;
use crate::search::LocalSearchSession;
use crate::synthesis::SynthesisReport;
use crate::synthesis::synthesize_results;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Parser)]
#[command(name = "koochi", about = "Run lightweight parallel agentic tests.")]
pub struct Cli {
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long)]
    pub json_output: Option<PathBuf>,
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
    #[error("failed to serialize JSON report: {0}")]
    Serialize(serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunExit {
    Success,
    TestFailures,
}

pub async fn run(cli: Cli) -> Result<RunExit, CliError> {
    let config_path = match cli.config {
        Some(path) => path,
        None => discover_config(std::env::current_dir().map_err(|source| ConfigError::Read {
            path: PathBuf::from("."),
            source,
        })?)?,
    };
    let config = KoochiConfig::from_path(&config_path)?;
    let config_dir = config_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let scope = build_scope(&config, config_dir)?;
    let search = Arc::new(LocalSearchSession::new(scope.clone()));
    let bus = build_llm_bus(&config)?;
    let verdicts = run_agents(
        scope.agents.clone(),
        search,
        bus,
        config.max_parallel_agents,
    )
    .await?;
    let report = synthesize_results(verdicts);
    print_report(&report);

    if let Some(path) = cli.json_output {
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

fn print_report(report: &SynthesisReport) {
    let total = report.passed.len() + report.failed.len();
    println!(
        "Koochi: {} agentic tests run, {} passed, {} failed",
        total,
        report.passed.len(),
        report.failed.len()
    );
    for verdict in &report.failed {
        let severity = verdict
            .severity
            .map(|severity| format!("{severity:?}"))
            .unwrap_or_else(|| "Unknown".to_string());
        println!(
            "- [{}] {}: {}",
            severity, verdict.test_id, verdict.description
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        })
        .await
        .unwrap();
        assert_eq!(exit, RunExit::Success);
        assert!(output.exists());
    }
}
