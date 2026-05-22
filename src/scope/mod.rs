use crate::AgentId;
use crate::RepoId;
use crate::ToolName;
use crate::config::AgentTestConfig;
use crate::config::KoochiConfig;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ScopeError {
    #[error("repo root `{0}` does not exist")]
    MissingRoot(PathBuf),
    #[error("failed to canonicalize repo root `{path}`: {source}")]
    Canonicalize {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScopeConfig {
    pub primary_repo: RepoScope,
    pub accessible_repos: Vec<RepoScope>,
    pub mcp_servers: Vec<McpServerScope>,
    pub tools: Vec<ToolScope>,
    pub agents: Vec<AgentSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoScope {
    pub repo_id: RepoId,
    pub root: PathBuf,
    pub revision: GitRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GitRevision {
    Head,
    Commit(String),
    Branch(String),
    Tag(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct McpServerScope {
    pub name: String,
    pub enabled_tools: Vec<ToolName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolScope {
    pub name: ToolName,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentSpec {
    pub id: AgentId,
    pub name: String,
    pub instruction: String,
    pub model: String,
    pub severity: Option<crate::Severity>,
}

pub fn build_scope(config: &KoochiConfig, config_dir: PathBuf) -> Result<ScopeConfig, ScopeError> {
    let root = if config.repo.root.is_absolute() {
        config.repo.root.clone()
    } else {
        config_dir.join(&config.repo.root)
    };
    if !root.exists() {
        return Err(ScopeError::MissingRoot(root));
    }
    let root = root
        .canonicalize()
        .map_err(|source| ScopeError::Canonicalize {
            path: root.clone(),
            source,
        })?;
    let repo_id = root.to_string_lossy().to_string();
    let primary_repo = RepoScope {
        repo_id,
        root,
        revision: parse_revision(&config.repo.revision),
    };
    let agents = config.tests.iter().map(AgentSpec::from).collect();
    Ok(ScopeConfig {
        primary_repo,
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents,
    })
}

fn parse_revision(revision: &str) -> GitRevision {
    if revision.eq_ignore_ascii_case("head") {
        GitRevision::Head
    } else {
        GitRevision::Commit(revision.to_string())
    }
}

impl From<&AgentTestConfig> for AgentSpec {
    fn from(value: &AgentTestConfig) -> Self {
        Self {
            id: value.id.clone(),
            name: value.name.clone(),
            instruction: value.instruction.clone(),
            model: value.model.clone(),
            severity: value.severity,
        }
    }
}
