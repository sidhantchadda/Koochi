use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

pub fn koochi_bin() -> &'static str {
    env!("CARGO_BIN_EXE_koochi")
}

pub fn fixture_codebase(language: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("codebases")
        .join(language)
        .join(name)
}

pub fn copy_fixture_codebase(language: &str, name: &str, destination: &Path) {
    copy_dir(&fixture_codebase(language, name), destination);
}

pub fn copy_fixture_codebase_under(language: &str, name: &str, destination: &Path, child: &str) {
    copy_dir(&fixture_codebase(language, name), &destination.join(child));
}

pub fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
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
            model: std::env::var("KOOCHI_E2E_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()),
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

pub fn write_koochi_config(
    path: &Path,
    live: &LiveProvider,
    max_parallel_agents: usize,
    max_parallel_llm_requests: usize,
    tests: &str,
) {
    fs::write(
        path.join("koochi.toml"),
        format!(
            "{}\n{}",
            live.toml_header(max_parallel_agents, max_parallel_llm_requests),
            tests
        ),
    )
    .unwrap();
}

pub fn run_koochi_with_live_provider(
    path: &Path,
    report_path: &Path,
    live: &LiveProvider,
) -> std::process::Output {
    std::process::Command::new(koochi_bin())
        .current_dir(path)
        .env(live.api_key_env, &live.api_key)
        .arg("--json-output")
        .arg(report_path)
        .output()
        .unwrap()
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
