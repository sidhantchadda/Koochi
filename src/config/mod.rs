use crate::AgentId;
use crate::Severity;
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

pub const DEFAULT_MODEL: &str = "gpt-5-nano";
pub const DEFAULT_PROVIDER: AiProvider = AiProvider::Fake;
pub const DEFAULT_MAX_PARALLEL_AGENTS: usize = 128;
pub const DEFAULT_MAX_AGENT_STEPS: usize = 128;
pub const DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET: usize = 24_000;
pub const DEFAULT_LLM_MAX_RETRIES: usize = 2;
pub const DEFAULT_CONFIG_FILE: &str = "koochi.toml";

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config `{path}`: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse config `{path}`: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("no koochi.toml config found from `{0}`")]
    NotFound(PathBuf),
    #[error("config must define at least one test")]
    NoTests,
    #[error("test instruction must not be empty")]
    EmptyInstruction,
    #[error("test id `{0}` is duplicated")]
    DuplicateTestId(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KoochiConfig {
    pub repo: RepoConfig,
    pub provider: AiProvider,
    pub model: String,
    pub api_key_env: Option<String>,
    pub base_url: Option<String>,
    pub max_parallel_agents: usize,
    pub max_agent_steps: usize,
    pub initial_context_token_budget: usize,
    pub max_parallel_llm_requests: usize,
    pub llm_max_retries: usize,
    pub tests: Vec<AgentTestConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoConfig {
    pub root: PathBuf,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTestConfig {
    pub id: AgentId,
    pub name: String,
    pub instruction: String,
    pub model: String,
    pub severity: Option<Severity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiProvider {
    Fake,
    #[serde(rename = "openai", alias = "open_ai")]
    OpenAi,
    Anthropic,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(default)]
    repo: RawRepoConfig,
    provider: Option<AiProvider>,
    model: Option<String>,
    api_key_env: Option<String>,
    base_url: Option<String>,
    max_parallel_agents: Option<usize>,
    max_agent_steps: Option<usize>,
    initial_context_token_budget: Option<usize>,
    max_parallel_llm_requests: Option<usize>,
    llm_max_retries: Option<usize>,
    #[serde(default)]
    tests: Vec<String>,
    #[serde(default, rename = "test")]
    test_tables: Vec<RawAgentTestConfig>,
}

#[derive(Debug, Deserialize)]
struct RawRepoConfig {
    root: Option<PathBuf>,
    revision: Option<String>,
}

impl Default for RawRepoConfig {
    fn default() -> Self {
        Self {
            root: None,
            revision: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawAgentTestConfig {
    id: Option<String>,
    name: Option<String>,
    instruction: String,
    model: Option<String>,
    severity: Option<Severity>,
}

impl KoochiConfig {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let content = fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let raw = toml::from_str::<RawConfig>(&content).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawConfig) -> Result<Self, ConfigError> {
        let model = raw.model.unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let mut tests = Vec::new();

        for (index, instruction) in raw.tests.into_iter().enumerate() {
            let instruction = normalize_required(instruction)?;
            let id = format!("test-{}", index + 1);
            tests.push(AgentTestConfig {
                name: id.clone(),
                id,
                instruction,
                model: model.clone(),
                severity: None,
            });
        }

        for (index, test) in raw.test_tables.into_iter().enumerate() {
            let instruction = normalize_required(test.instruction)?;
            let id = test
                .id
                .map(normalize_required)
                .transpose()?
                .unwrap_or_else(|| format!("test-{}", tests.len() + index + 1));
            let name = test
                .name
                .map(normalize_required)
                .transpose()?
                .unwrap_or_else(|| id.clone());
            tests.push(AgentTestConfig {
                id,
                name,
                instruction,
                model: test.model.unwrap_or_else(|| model.clone()),
                severity: test.severity,
            });
        }

        if tests.is_empty() {
            return Err(ConfigError::NoTests);
        }
        let mut seen = std::collections::HashSet::new();
        for test in &tests {
            if !seen.insert(test.id.clone()) {
                return Err(ConfigError::DuplicateTestId(test.id.clone()));
            }
        }

        let max_parallel_agents = raw
            .max_parallel_agents
            .unwrap_or(DEFAULT_MAX_PARALLEL_AGENTS)
            .max(1);

        Ok(Self {
            repo: RepoConfig {
                root: raw.repo.root.unwrap_or_else(|| PathBuf::from(".")),
                revision: raw.repo.revision.unwrap_or_else(|| "HEAD".to_string()),
            },
            provider: raw.provider.unwrap_or(DEFAULT_PROVIDER),
            model,
            api_key_env: raw.api_key_env,
            base_url: raw.base_url,
            max_parallel_agents,
            max_agent_steps: raw
                .max_agent_steps
                .unwrap_or(DEFAULT_MAX_AGENT_STEPS)
                .max(1),
            initial_context_token_budget: raw
                .initial_context_token_budget
                .unwrap_or(DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET)
                .max(1),
            max_parallel_llm_requests: raw
                .max_parallel_llm_requests
                .unwrap_or(max_parallel_agents)
                .max(1),
            llm_max_retries: raw.llm_max_retries.unwrap_or(DEFAULT_LLM_MAX_RETRIES),
            tests,
        })
    }
}

pub fn discover_config(start: impl AsRef<Path>) -> Result<PathBuf, ConfigError> {
    let start = start.as_ref();
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    };

    loop {
        if let Some(path) = find_config_in_dir(&current)? {
            return Ok(path);
        }
        if !current.pop() {
            return Err(ConfigError::NotFound(start.to_path_buf()));
        }
    }
}

fn find_config_in_dir(dir: &Path) -> Result<Option<PathBuf>, ConfigError> {
    let entries = fs::read_dir(dir).map_err(|source| ConfigError::Read {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| ConfigError::Read {
            path: dir.to_path_buf(),
            source,
        })?;
        let file_name = entry.file_name();
        if file_name
            .to_string_lossy()
            .eq_ignore_ascii_case(DEFAULT_CONFIG_FILE)
        {
            return Ok(Some(entry.path()));
        }
    }
    Ok(None)
}

fn normalize_required(value: String) -> Result<String, ConfigError> {
    let value = value.trim().to_string();
    if value.is_empty() {
        Err(ConfigError::EmptyInstruction)
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_tests_and_defaults() {
        let raw = toml::from_str::<RawConfig>(
            r#"
            tests = ["Check retries", "Check auth"]
            "#,
        )
        .unwrap();
        let config = KoochiConfig::from_raw(raw).unwrap();
        assert_eq!(config.model, DEFAULT_MODEL);
        assert_eq!(config.provider, AiProvider::Fake);
        assert_eq!(config.max_parallel_agents, 128);
        assert_eq!(config.max_agent_steps, DEFAULT_MAX_AGENT_STEPS);
        assert_eq!(
            config.initial_context_token_budget,
            DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET
        );
        assert_eq!(config.max_parallel_llm_requests, 128);
        assert_eq!(config.llm_max_retries, DEFAULT_LLM_MAX_RETRIES);
        assert_eq!(config.tests.len(), 2);
        assert_eq!(config.tests[0].id, "test-1");
        assert_eq!(config.tests[0].instruction, "Check retries");
    }

    #[test]
    fn parses_table_overrides_and_order() {
        let raw = toml::from_str::<RawConfig>(
            r#"
            model = "global"
            tests = ["Simple"]

            [[test]]
            id = "retry"
            name = "Retry"
            instruction = "Detailed"
            model = "override"
            severity = "high"
            "#,
        )
        .unwrap();
        let config = KoochiConfig::from_raw(raw).unwrap();
        assert_eq!(config.tests[0].id, "test-1");
        assert_eq!(config.tests[1].id, "retry");
        assert_eq!(config.tests[1].model, "override");
        assert_eq!(config.tests[1].severity, Some(Severity::High));
    }

    #[test]
    fn parses_provider_options() {
        let raw = toml::from_str::<RawConfig>(
            r#"
            provider = "openai"
            api_key_env = "MY_KEY"
            base_url = "https://example.test"
            max_agent_steps = 9
            initial_context_token_budget = 1234
            max_parallel_llm_requests = 16
            llm_max_retries = 4
            tests = ["x"]
            "#,
        )
        .unwrap();
        let config = KoochiConfig::from_raw(raw).unwrap();
        assert_eq!(config.provider, AiProvider::OpenAi);
        assert_eq!(config.api_key_env.as_deref(), Some("MY_KEY"));
        assert_eq!(config.base_url.as_deref(), Some("https://example.test"));
        assert_eq!(config.max_agent_steps, 9);
        assert_eq!(config.initial_context_token_budget, 1234);
        assert_eq!(config.max_parallel_llm_requests, 16);
        assert_eq!(config.llm_max_retries, 4);
    }

    #[test]
    fn parses_anthropic_provider() {
        let raw = toml::from_str::<RawConfig>(
            r#"
            provider = "anthropic"
            tests = ["x"]
            "#,
        )
        .unwrap();
        let config = KoochiConfig::from_raw(raw).unwrap();
        assert_eq!(config.provider, AiProvider::Anthropic);
    }

    #[test]
    fn discovers_config_case_insensitively() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("KOOCHI.TOML"), "tests = [\"x\"]").unwrap();
        let found = discover_config(temp.path()).unwrap();
        assert_eq!(found.file_name().unwrap(), "KOOCHI.TOML");
    }
}
