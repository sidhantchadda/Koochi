use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

pub fn koochi_bin() -> &'static str {
    env!("CARGO_BIN_EXE_koochi")
}

pub fn fixture_codebase(language: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("e2e")
        .join("codebases")
        .join(language)
        .join(name)
}

pub fn copy_fixture_codebase(language: &str, name: &str, destination: &Path) {
    copy_dir(&fixture_codebase(language, name), destination);
}

pub fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[derive(Debug, Clone)]
pub enum Fixture {
    Copy {
        language: &'static str,
        name: &'static str,
    },
}

#[derive(Debug, Clone)]
pub struct E2eCase<'a> {
    pub fixtures: &'a [Fixture],
    pub max_parallel_agents: usize,
    pub max_parallel_llm_requests: usize,
    pub expected: ExpectedReport<'a>,
    pub expected_exit: i32,
    pub debug: bool,
}

#[derive(Debug)]
pub struct E2eRun {
    pub output: Output,
    pub report: Value,
    pub debug_log: Option<Value>,
}

impl<'a> E2eCase<'a> {
    pub fn live_fixture_config(
        fixtures: &'a [Fixture],
        max_parallel_agents: usize,
        max_parallel_llm_requests: usize,
        expected: ExpectedReport<'a>,
    ) -> Self {
        let expected_exit = if expected.failed.is_empty() { 0 } else { 1 };
        Self {
            fixtures,
            max_parallel_agents,
            max_parallel_llm_requests,
            expected,
            expected_exit,
            debug: false,
        }
    }

    pub fn with_debug(mut self) -> Self {
        self.debug = true;
        self
    }

    pub fn run_with_config_name(self, config_name: &str) -> E2eRun {
        run_case_with_config_name(self, config_name)
    }
}

pub fn run_case(case: E2eCase<'_>) -> E2eRun {
    run_case_with_config_name(case, "koochi.toml")
}

fn run_case_with_config_name(case: E2eCase<'_>, config_name: &str) -> E2eRun {
    let temp = tempfile::tempdir().unwrap();
    for fixture in case.fixtures {
        match fixture {
            Fixture::Copy { language, name } => {
                copy_fixture_codebase(language, name, temp.path());
            }
        }
    }
    write_case_config(temp.path(), config_name, &case);
    let report_path = temp.path().join("report.json");
    let output = run_case_command(temp.path(), &report_path, &case);
    assert_eq!(
        output.status.code(),
        Some(case.expected_exit),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = read_json(&report_path);
    assert_report_matches(&report, case.expected);
    let debug_log = case.debug.then(|| latest_debug_log(temp.path()));
    E2eRun {
        output,
        report,
        debug_log,
    }
}

fn write_case_config(path: &Path, config_name: &str, case: &E2eCase<'_>) {
    let live = live_provider();
    let header = live.toml_header(case.max_parallel_agents, case.max_parallel_llm_requests);
    let config_path = path.join(config_name);
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(
        config_path,
        format!("{header}\n{}", strip_provider_header(&config)),
    )
    .unwrap();
}

fn strip_provider_header(config: &str) -> String {
    config
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("provider =")
                && !trimmed.starts_with("model =")
                && !trimmed.starts_with("api_key_env =")
                && !trimmed.starts_with("max_parallel_agents =")
                && !trimmed.starts_with("max_parallel_llm_requests =")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn run_case_command(path: &Path, report_path: &Path, case: &E2eCase<'_>) -> Output {
    let mut command = Command::new(koochi_bin());
    command
        .current_dir(path)
        .arg("--json-output")
        .arg(report_path);
    if case.debug {
        command.arg("--debug");
    }
    let live = live_provider();
    command.env(live.api_key_env, &live.api_key);
    command.output().unwrap()
}

pub fn latest_debug_log(root: &Path) -> Value {
    let mut entries = fs::read_dir(root.join(".koochi").join("debug"))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    entries.sort_by_key(|entry| entry.file_name());
    read_json(&entries.last().unwrap().path())
}

#[derive(Debug, Clone)]
pub struct ExpectedReport<'a> {
    pub passed: &'a [&'a str],
    pub failed: &'a [&'a str],
}

impl<'a> ExpectedReport<'a> {
    pub fn all_failed(failed: &'a [&'a str]) -> Self {
        Self {
            passed: &[],
            failed,
        }
    }
}

pub fn assert_report_matches(report: &Value, expected: ExpectedReport<'_>) {
    let passed = ids(report, "passed");
    let failed = ids(report, "failed");
    assert_eq!(
        passed,
        expected.passed.iter().copied().collect::<HashSet<_>>(),
        "unexpected passed tests in report: {report:#}"
    );
    assert_eq!(
        failed,
        expected.failed.iter().copied().collect::<HashSet<_>>(),
        "unexpected failed tests in report: {report:#}"
    );
}

pub fn assert_failures_have_evidence(report: &Value) {
    for item in report["failed"].as_array().unwrap() {
        assert!(
            !item["evidence"].as_array().unwrap().is_empty(),
            "missing evidence for {}",
            item["test_id"]
        );
    }
}

fn ids<'a>(report: &'a Value, field: &str) -> HashSet<&'a str> {
    report[field]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["test_id"].as_str().unwrap())
        .collect()
}

#[derive(Debug, Clone)]
pub struct LiveProvider {
    pub provider: String,
    pub model: String,
    pub api_key_env: &'static str,
    pub api_key: String,
}

impl LiveProvider {
    pub fn toml_header(
        &self,
        max_parallel_agents: usize,
        max_parallel_llm_requests: usize,
    ) -> String {
        format!(
            r#"provider = "{provider}"
model = "{model}"
api_key_env = "{api_key_env}"
max_parallel_agents = {max_parallel_agents}
max_parallel_llm_requests = {max_parallel_llm_requests}
"#,
            provider = self.provider,
            model = self.model,
            api_key_env = self.api_key_env,
        )
    }
}

pub fn live_provider() -> LiveProvider {
    let provider = std::env::var("KOOCHI_E2E_PROVIDER").unwrap_or_else(|_| "openai".to_string());
    match provider.as_str() {
        "openai" => LiveProvider {
            provider,
            model: std::env::var("KOOCHI_E2E_MODEL").unwrap_or_else(|_| "gpt-5-nano".to_string()),
            api_key_env: "OPENAI_API_KEY",
            api_key: key_from_env_or_dotenv("OPENAI_API_KEY")
                .expect("set OPENAI_API_KEY or create .env.local"),
        },
        "anthropic" => LiveProvider {
            provider,
            model: std::env::var("KOOCHI_E2E_MODEL")
                .unwrap_or_else(|_| "claude-3-5-haiku-latest".to_string()),
            api_key_env: "ANTHROPIC_API_KEY",
            api_key: key_from_env_or_dotenv("ANTHROPIC_API_KEY")
                .expect("set ANTHROPIC_API_KEY or create .env.local"),
        },
        other => panic!("unsupported KOOCHI_E2E_PROVIDER `{other}`; expected openai or anthropic"),
    }
}

fn key_from_env_or_dotenv(name: &str) -> Option<String> {
    std::env::var(name).ok().or_else(|| read_dotenv_key(name))
}

fn read_dotenv_key(name: &str) -> Option<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".env.local");
    let content = fs::read_to_string(path).ok()?;
    content.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == name).then(|| {
            value
                .trim()
                .trim_matches('\'')
                .trim_matches('"')
                .to_string()
        })
    })
}

fn copy_dir(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir(&source_path, &destination_path);
        } else {
            fs::copy(&source_path, &destination_path).unwrap();
        }
    }
}
