use crate::AgentId;
use crate::FilePath;
use crate::RepoId;
use crate::ToolName;
use crate::config::AgentTestConfig;
use crate::config::KoochiConfig;
use std::path::PathBuf;
use std::process::Command;

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
    pub review: ReviewScope,
    pub accessible_repos: Vec<RepoScope>,
    pub mcp_servers: Vec<McpServerScope>,
    pub tools: Vec<ToolScope>,
    pub agents: Vec<AgentSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReviewScope {
    pub mode: ReviewMode,
    pub files: Vec<FilePath>,
    pub commit: Option<CommitInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReviewMode {
    LocalChanges,
    HeadCommit,
    FullRepoFallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommitInfo {
    pub short_id: String,
    pub subject: String,
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
        root: root.clone(),
        revision: parse_revision(&config.repo.revision),
    };
    let review = build_review_scope(&root);
    let agents = config.tests.iter().map(AgentSpec::from).collect();
    Ok(ScopeConfig {
        primary_repo,
        review,
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents,
    })
}

fn build_review_scope(root: &std::path::Path) -> ReviewScope {
    if !is_git_root(root) {
        return ReviewScope {
            mode: ReviewMode::FullRepoFallback,
            files: Vec::new(),
            commit: None,
        };
    }

    if let Some(files) = local_changed_files(root) {
        if !files.is_empty() {
            return ReviewScope {
                mode: ReviewMode::LocalChanges,
                files,
                commit: None,
            };
        }
    } else {
        return ReviewScope {
            mode: ReviewMode::FullRepoFallback,
            files: Vec::new(),
            commit: None,
        };
    }

    if let Some(files) = head_commit_files(root) {
        return ReviewScope {
            mode: ReviewMode::HeadCommit,
            files,
            commit: head_commit_info(root),
        };
    }

    ReviewScope {
        mode: ReviewMode::FullRepoFallback,
        files: Vec::new(),
        commit: None,
    }
}

fn is_git_root(root: &std::path::Path) -> bool {
    let output = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["rev-parse", "--show-toplevel"])
        .output();
    let Ok(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let top_level = String::from_utf8_lossy(&output.stdout);
    let Ok(top_level) = std::path::Path::new(top_level.trim()).canonicalize() else {
        return false;
    };
    root.canonicalize().is_ok_and(|root| root == top_level)
}

fn local_changed_files(root: &std::path::Path) -> Option<Vec<FilePath>> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let mut files = Vec::new();
    for entry in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|e| !e.is_empty())
    {
        if entry.len() < 4 {
            continue;
        }
        let status = &entry[..2];
        let path = String::from_utf8_lossy(&entry[3..]).to_string();
        if status.contains(&b'D') {
            continue;
        }
        let path = if let Some((_, new_path)) = path.split_once(" -> ") {
            new_path.to_string()
        } else {
            path
        };
        if root.join(&path).is_file() {
            let path = path_to_unix(&path);
            if !is_koochi_local_file(&path) {
                files.push(path);
            }
        }
    }
    files.sort();
    files.dedup();
    Some(files)
}

fn head_commit_files(root: &std::path::Path) -> Option<Vec<FilePath>> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut files = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .filter(|path| root.join(path).is_file())
        .map(path_to_unix)
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    Some(files)
}

fn head_commit_info(root: &std::path::Path) -> Option<CommitInfo> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["log", "-1", "--format=%h%x00%s"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let (short_id, subject) = text.trim_end().split_once('\0')?;
    Some(CommitInfo {
        short_id: short_id.to_string(),
        subject: subject.to_string(),
    })
}

fn path_to_unix(path: &str) -> String {
    path.replace('\\', "/")
}

fn is_koochi_local_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower == "koochi.toml"
        || lower == ".env.local"
        || lower.starts_with(".koochi/")
        || lower.starts_with("koochi-debug/")
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn review_scope_prefers_local_changes() {
        let temp = tempfile::tempdir().unwrap();
        if !git(temp.path(), ["init"]) {
            return;
        }
        git(temp.path(), ["config", "user.email", "koochi@example.test"]);
        git(temp.path(), ["config", "user.name", "Koochi"]);
        fs::write(temp.path().join("committed.rs"), "fn committed() {}\n").unwrap();
        git(temp.path(), ["add", "."]);
        git(temp.path(), ["commit", "-m", "initial"]);
        fs::write(temp.path().join("changed.rs"), "fn changed() {}\n").unwrap();

        let review = build_review_scope(temp.path());

        assert_eq!(review.mode, ReviewMode::LocalChanges);
        assert_eq!(review.files, vec!["changed.rs"]);
    }

    #[test]
    fn review_scope_uses_head_commit_when_worktree_is_clean() {
        let temp = tempfile::tempdir().unwrap();
        if !git(temp.path(), ["init"]) {
            return;
        }
        git(temp.path(), ["config", "user.email", "koochi@example.test"]);
        git(temp.path(), ["config", "user.name", "Koochi"]);
        fs::write(temp.path().join("first.rs"), "fn first() {}\n").unwrap();
        git(temp.path(), ["add", "."]);
        git(temp.path(), ["commit", "-m", "initial"]);
        fs::write(temp.path().join("second.rs"), "fn second() {}\n").unwrap();
        git(temp.path(), ["add", "."]);
        git(temp.path(), ["commit", "-m", "second"]);

        let review = build_review_scope(temp.path());

        assert_eq!(review.mode, ReviewMode::HeadCommit);
        assert_eq!(review.files, vec!["second.rs"]);
    }

    #[test]
    fn review_scope_ignores_only_koochi_local_changes() {
        let temp = tempfile::tempdir().unwrap();
        if !git(temp.path(), ["init"]) {
            return;
        }
        git(temp.path(), ["config", "user.email", "koochi@example.test"]);
        git(temp.path(), ["config", "user.name", "Koochi"]);
        fs::write(temp.path().join("first.rs"), "fn first() {}\n").unwrap();
        git(temp.path(), ["add", "."]);
        git(temp.path(), ["commit", "-m", "initial"]);
        fs::write(temp.path().join("second.rs"), "fn second() {}\n").unwrap();
        git(temp.path(), ["add", "."]);
        git(temp.path(), ["commit", "-m", "second"]);
        fs::write(temp.path().join("koochi.toml"), "tests = [\"x\"]\n").unwrap();
        fs::create_dir_all(temp.path().join(".koochi/debug")).unwrap();
        fs::write(temp.path().join(".koochi/debug/run.json"), "{}\n").unwrap();

        let review = build_review_scope(temp.path());

        assert_eq!(review.mode, ReviewMode::HeadCommit);
        assert_eq!(review.files, vec!["second.rs"]);
    }

    #[test]
    fn review_scope_falls_back_when_config_root_is_inside_parent_git_repo() {
        let temp = tempfile::tempdir().unwrap();
        if !git(temp.path(), ["init"]) {
            return;
        }
        git(temp.path(), ["config", "user.email", "koochi@example.test"]);
        git(temp.path(), ["config", "user.name", "Koochi"]);
        fs::create_dir_all(temp.path().join("fixture/src")).unwrap();
        fs::write(temp.path().join("fixture/src/lib.rs"), "fn fixture() {}\n").unwrap();
        git(temp.path(), ["add", "."]);
        git(temp.path(), ["commit", "-m", "initial"]);

        let review = build_review_scope(&temp.path().join("fixture"));

        assert_eq!(review.mode, ReviewMode::FullRepoFallback);
        assert!(review.files.is_empty());
    }

    fn git<const N: usize>(root: &std::path::Path, args: [&str; N]) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}
