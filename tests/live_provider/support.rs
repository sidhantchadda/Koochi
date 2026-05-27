use serde_json::Value;
use std::collections::BTreeSet;
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
pub struct LiveProviderCase<'a> {
    pub fixtures: &'a [Fixture],
    pub expected: ExpectedReport<'a>,
    pub expected_exit: i32,
    pub debug: bool,
}

#[derive(Debug)]
pub struct LiveProviderRun {
    pub output: Output,
    pub report: Value,
    pub debug_log: Option<Value>,
}

impl<'a> LiveProviderCase<'a> {
    pub fn live_fixture_config(fixtures: &'a [Fixture], expected: ExpectedReport<'a>) -> Self {
        let expected_exit = if expected.failed.is_empty() { 0 } else { 1 };
        Self {
            fixtures,
            expected,
            expected_exit,
            debug: false,
        }
    }

    pub fn with_debug(mut self) -> Self {
        self.debug = true;
        self
    }

    pub fn run_with_config_name(self, config_name: &str) -> LiveProviderRun {
        run_case_with_config_name(self, config_name)
    }
}

pub fn run_case(case: LiveProviderCase<'_>) -> LiveProviderRun {
    run_case_with_config_name(case, "koochi.toml")
}

fn run_case_with_config_name(case: LiveProviderCase<'_>, config_name: &str) -> LiveProviderRun {
    let temp = tempfile::tempdir().unwrap();
    for fixture in case.fixtures {
        match fixture {
            Fixture::Copy { language, name } => {
                copy_fixture_codebase(language, name, temp.path());
            }
        }
    }
    let config_path = temp.path().join(config_name);
    assert!(
        config_path.exists(),
        "fixture copy must contain requested config file {}",
        config_path.display()
    );
    let live = live_provider_for_config(&config_path);
    let report_path = temp.path().join("report.json");
    let output = run_case_command(temp.path(), &report_path, &case, &live);
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
    LiveProviderRun {
        output,
        report,
        debug_log,
    }
}

fn run_case_command(
    path: &Path,
    report_path: &Path,
    case: &LiveProviderCase<'_>,
    live: &LiveProvider,
) -> Output {
    let mut command = Command::new(koochi_bin());
    command
        .current_dir(path)
        .arg("--yes")
        .arg("--json-output")
        .arg(report_path);
    if case.debug {
        command.arg("--debug");
    }
    command.env(&live.api_key_env, &live.api_key);
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
    pub api_key_env: String,
    pub api_key: String,
}

pub fn live_provider_for_config(config_path: &Path) -> LiveProvider {
    let config = fs::read_to_string(config_path).unwrap();
    let config: toml::Value = toml::from_str(&config).unwrap();
    let provider = config
        .get("provider")
        .and_then(toml::Value::as_str)
        .unwrap_or("openai");
    let api_key_env = config
        .get("api_key_env")
        .and_then(toml::Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| default_api_key_env(provider).to_string());
    let api_key = key_from_env_or_dotenv(&api_key_env)
        .unwrap_or_else(|| panic!("set {api_key_env} or create .env.local"));
    LiveProvider {
        api_key_env,
        api_key,
    }
}

fn default_api_key_env(provider: &str) -> &'static str {
    match provider {
        "openai" => "OPENAI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        other => panic!("unsupported provider `{other}` in fixture config"),
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
        let file_name = entry.file_name();
        if matches!(
            file_name.to_str(),
            Some("target" | ".koochi" | "Cargo.lock")
        ) {
            continue;
        }
        let source_path = entry.path();
        let destination_path = destination.join(file_name);
        if source_path.is_dir() {
            copy_dir(&source_path, &destination_path);
        } else {
            fs::copy(&source_path, &destination_path).unwrap();
        }
    }
}

const REFERENCED_FIXTURES: &[(&str, &str)] = &[
    ("js", "creator_market"),
    ("polyglot", "review_workspace"),
    ("python", "clinic_scheduler"),
    ("rust", "config_discovery"),
    ("rust", "config_override"),
    ("rust", "consistency"),
    ("rust", "fulfillment_hub"),
    ("rust", "parallel_stress"),
    ("rust", "tool_loop"),
];

#[test]
fn live_provider_fixture_projects_have_owned_configs() {
    for (language, name) in fixture_projects() {
        let root = fixture_codebase(&language, &name);
        assert!(
            has_koochi_config(&root),
            "fixture {language}/{name} must own koochi.toml or KOOCHI.TOML"
        );
    }
}

#[test]
fn live_provider_fixture_configs_parse() {
    for (language, name) in fixture_projects() {
        let root = fixture_codebase(&language, &name);
        for config in fixture_configs(&root) {
            koochi::KoochiConfig::from_path(&config).unwrap_or_else(|error| {
                panic!(
                    "fixture {language}/{name} config {} should parse: {error}",
                    config.display()
                )
            });
        }
    }
}

#[test]
fn live_provider_fixture_projects_are_all_referenced() {
    let actual = fixture_projects();
    let expected = REFERENCED_FIXTURES
        .iter()
        .map(|(language, name)| ((*language).to_string(), (*name).to_string()))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual, expected,
        "every fixture project must be covered by a live provider test"
    );
}

#[test]
fn legacy_e2e_fixture_tree_is_removed() {
    assert!(
        !Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("e2e")
            .exists(),
        "tests/e2e should not contain tracked live provider fixtures or harness files"
    );
}

fn fixture_projects() -> BTreeSet<(String, String)> {
    let mut fixtures = BTreeSet::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("codebases");
    for language in fs::read_dir(root).unwrap() {
        let language = language.unwrap();
        if !language.path().is_dir() {
            continue;
        }
        for project in fs::read_dir(language.path()).unwrap() {
            let project = project.unwrap();
            if !project.path().is_dir() {
                continue;
            }
            fixtures.insert((
                language.file_name().to_string_lossy().into_owned(),
                project.file_name().to_string_lossy().into_owned(),
            ));
        }
    }
    fixtures
}

fn has_koochi_config(root: &Path) -> bool {
    root.join("koochi.toml").exists() || root.join("KOOCHI.TOML").exists()
}

fn fixture_configs(root: &Path) -> Vec<PathBuf> {
    ["koochi.toml", "KOOCHI.TOML", "override.toml"]
        .into_iter()
        .map(|name| root.join(name))
        .filter(|path| path.exists())
        .collect()
}
