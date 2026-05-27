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
    #[error(
        "cannot review HEAD because its parent commit is not available locally. This usually means the repository is a shallow clone. Fetch enough history with `git fetch --depth=2` or use a full clone, then rerun Koochi."
    )]
    MissingHeadParent,
    #[error("cannot review git ref `{0}` because it does not resolve to a commit")]
    InvalidReviewRef(String),
    #[error("`--all`/`--full-repo` cannot be combined with `--commit`, `--base`, or `--head`")]
    ConflictingFullRepoOptions,
    #[error("`--commit` cannot be combined with `--base` or `--head`")]
    ConflictingReviewOptions,
    #[error("`--base` and `--head` must be provided together")]
    IncompleteReviewRange,
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
    pub hunks: Vec<ReviewHunk>,
    pub commit: Option<CommitInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReviewChanges {
    files: Vec<FilePath>,
    hunks: Vec<ReviewHunk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReviewMode {
    LocalChanges,
    HeadCommit,
    Commit,
    DiffRange { base: String, head: String },
    FullRepo,
    FullRepoFallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommitInfo {
    pub short_id: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
pub struct ReviewHunk {
    pub id: String,
    pub path: FilePath,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<ReviewHunkLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
pub struct ReviewHunkLine {
    pub kind: ReviewLineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewLineKind {
    Added,
    Removed,
    Context,
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

impl GitRevision {
    pub fn as_ref(&self) -> &str {
        match self {
            GitRevision::Head => "HEAD",
            GitRevision::Commit(rev) | GitRevision::Branch(rev) | GitRevision::Tag(rev) => rev,
        }
    }
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
    pub initial_context_token_budget: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReviewTarget {
    Auto,
    Commit(String),
    Range { base: String, head: String },
    FullRepo,
}

pub fn build_scope(
    config: &KoochiConfig,
    config_dir: PathBuf,
    target: ReviewTarget,
) -> Result<ScopeConfig, ScopeError> {
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
    let revision = match &target {
        ReviewTarget::Auto | ReviewTarget::FullRepo => parse_revision(&config.repo.revision),
        ReviewTarget::Commit(commit) => GitRevision::Commit(commit.clone()),
        ReviewTarget::Range { head, .. } => GitRevision::Commit(head.clone()),
    };
    let primary_repo = RepoScope {
        repo_id,
        root: root.clone(),
        revision,
    };
    let review = build_review_scope(&root, &target)?;
    let agents = config
        .tests
        .iter()
        .map(|test| AgentSpec {
            initial_context_token_budget: config.initial_context_token_budget,
            ..AgentSpec::from(test)
        })
        .collect();
    Ok(ScopeConfig {
        primary_repo,
        review,
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents,
    })
}

fn build_review_scope(
    root: &std::path::Path,
    target: &ReviewTarget,
) -> Result<ReviewScope, ScopeError> {
    if !is_git_root(root) {
        if !matches!(target, ReviewTarget::Auto | ReviewTarget::FullRepo) {
            return Err(ScopeError::InvalidReviewRef(
                "explicit review target requires a git repository root".to_string(),
            ));
        }
        return Ok(ReviewScope {
            mode: ReviewMode::FullRepoFallback,
            files: Vec::new(),
            hunks: Vec::new(),
            commit: None,
        });
    }

    match target {
        ReviewTarget::Auto => {}
        ReviewTarget::Commit(commit) => return explicit_commit_review_scope(root, commit),
        ReviewTarget::Range { base, head } => return diff_range_review_scope(root, base, head),
        ReviewTarget::FullRepo => {
            return Ok(ReviewScope {
                mode: ReviewMode::FullRepo,
                files: Vec::new(),
                hunks: Vec::new(),
                commit: None,
            });
        }
    }

    if let Some(changes) = local_review_changes(root) {
        if !changes.files.is_empty() {
            return Ok(ReviewScope {
                mode: ReviewMode::LocalChanges,
                files: changes.files,
                hunks: changes.hunks,
                commit: None,
            });
        }
    } else {
        return Ok(ReviewScope {
            mode: ReviewMode::FullRepoFallback,
            files: Vec::new(),
            hunks: Vec::new(),
            commit: None,
        });
    }

    if !head_parent_exists(root) {
        return Err(ScopeError::MissingHeadParent);
    }

    if let Some(changes) = head_commit_review(root) {
        return Ok(ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: changes.files,
            hunks: changes.hunks,
            commit: head_commit_info(root),
        });
    }

    Ok(ReviewScope {
        mode: ReviewMode::FullRepoFallback,
        files: Vec::new(),
        hunks: Vec::new(),
        commit: None,
    })
}

pub fn review_target_from_options(
    commit: Option<String>,
    base: Option<String>,
    head: Option<String>,
    full_repo: bool,
) -> Result<ReviewTarget, ScopeError> {
    if full_repo && (commit.is_some() || base.is_some() || head.is_some()) {
        return Err(ScopeError::ConflictingFullRepoOptions);
    }
    if full_repo {
        return Ok(ReviewTarget::FullRepo);
    }
    if commit.is_some() && (base.is_some() || head.is_some()) {
        return Err(ScopeError::ConflictingReviewOptions);
    }
    match (commit, base, head) {
        (Some(commit), None, None) => Ok(ReviewTarget::Commit(commit)),
        (None, Some(base), Some(head)) => Ok(ReviewTarget::Range { base, head }),
        (None, None, None) => Ok(ReviewTarget::Auto),
        _ => Err(ScopeError::IncompleteReviewRange),
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

fn local_review_changes(root: &std::path::Path) -> Option<ReviewChanges> {
    let files = local_changed_files(root)?;
    let hunks = local_changed_hunks(root, &files)?;
    Some(ReviewChanges { files, hunks })
}

fn head_commit_review(root: &std::path::Path) -> Option<ReviewChanges> {
    let files = head_commit_files(root)?;
    let hunks = head_commit_hunks(root).unwrap_or_default();
    Some(ReviewChanges { files, hunks })
}

fn explicit_commit_review_scope(
    root: &std::path::Path,
    commit: &str,
) -> Result<ReviewScope, ScopeError> {
    ensure_commit_ref(root, commit)?;
    if !commit_parent_exists(root, commit) {
        return Err(ScopeError::MissingHeadParent);
    }
    let changes = commit_review(root, commit).unwrap_or_else(|| ReviewChanges {
        files: Vec::new(),
        hunks: Vec::new(),
    });
    Ok(ReviewScope {
        mode: ReviewMode::Commit,
        files: changes.files,
        hunks: changes.hunks,
        commit: commit_info(root, commit),
    })
}

fn diff_range_review_scope(
    root: &std::path::Path,
    base: &str,
    head: &str,
) -> Result<ReviewScope, ScopeError> {
    ensure_commit_ref(root, base)?;
    ensure_commit_ref(root, head)?;
    let changes = range_review(root, base, head).unwrap_or_else(|| ReviewChanges {
        files: Vec::new(),
        hunks: Vec::new(),
    });
    Ok(ReviewScope {
        mode: ReviewMode::DiffRange {
            base: base.to_string(),
            head: head.to_string(),
        },
        files: changes.files,
        hunks: changes.hunks,
        commit: commit_info(root, head),
    })
}

fn commit_review(root: &std::path::Path, commit: &str) -> Option<ReviewChanges> {
    let files = commit_files(root, commit)?;
    let hunks = commit_hunks(root, commit).unwrap_or_default();
    Some(ReviewChanges { files, hunks })
}

fn range_review(root: &std::path::Path, base: &str, head: &str) -> Option<ReviewChanges> {
    let files = range_files(root, base, head)?;
    let hunks = range_hunks(root, base, head).unwrap_or_default();
    Some(ReviewChanges { files, hunks })
}

fn head_parent_exists(root: &std::path::Path) -> bool {
    commit_parent_exists(root, "HEAD")
}

fn commit_parent_exists(root: &std::path::Path, commit: &str) -> bool {
    let parent = format!("{commit}^");
    Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["rev-parse", "--verify", "--quiet", &parent])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn ensure_commit_ref(root: &std::path::Path, rev: &str) -> Result<(), ScopeError> {
    let spec = format!("{rev}^{{commit}}");
    let ok = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["rev-parse", "--verify", "--quiet", &spec])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    ok.then_some(())
        .ok_or_else(|| ScopeError::InvalidReviewRef(rev.to_string()))
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
    let entries = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();
    let mut index = 0;
    while index < entries.len() {
        let entry = entries[index];
        index += 1;
        if entry.len() < 4 {
            continue;
        }
        let status = &entry[..2];
        let path = String::from_utf8_lossy(&entry[3..]).to_string();
        if status.contains(&b'R') || status.contains(&b'C') {
            index += 1;
        }
        if status.contains(&b'D') {
            continue;
        }
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
        .filter(|path| !is_koochi_local_file(path))
        .filter(|path| root.join(path).is_file())
        .map(path_to_unix)
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    Some(files)
}

fn commit_files(root: &std::path::Path, commit: &str) -> Option<Vec<FilePath>> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args([
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "--diff-filter=ACMR",
            "-r",
            commit,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(parse_changed_files(&output.stdout))
}

fn range_files(root: &std::path::Path, base: &str, head: &str) -> Option<Vec<FilePath>> {
    let range = format!("{base}...{head}");
    let output = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["diff", "--name-only", "--diff-filter=ACMR", &range])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(parse_changed_files(&output.stdout))
}

fn parse_changed_files(stdout: &[u8]) -> Vec<FilePath> {
    let mut files = String::from_utf8_lossy(stdout)
        .lines()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(path_to_unix)
        .filter(|path| !is_koochi_local_file(path))
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files
}

fn local_changed_hunks(root: &std::path::Path, files: &[FilePath]) -> Option<Vec<ReviewHunk>> {
    let mut hunks = git_diff_hunks(root, &["diff", "--unified=0", "--no-ext-diff", "HEAD"])?;
    hunks.retain(|hunk| !is_koochi_local_file(&hunk.path));
    let tracked_paths = hunks
        .iter()
        .map(|hunk| hunk.path.clone())
        .collect::<std::collections::HashSet<_>>();
    for path in files {
        if tracked_paths.contains(path.as_str()) || is_koochi_local_file(path) {
            continue;
        }
        if let Some(hunk) = untracked_file_hunk(root, path) {
            hunks.push(hunk);
        }
    }
    renumber_hunks(&mut hunks);
    Some(hunks)
}

fn head_commit_hunks(root: &std::path::Path) -> Option<Vec<ReviewHunk>> {
    git_diff_hunks(
        root,
        &["show", "--format=", "--unified=0", "--no-ext-diff", "HEAD"],
    )
}

fn commit_hunks(root: &std::path::Path, commit: &str) -> Option<Vec<ReviewHunk>> {
    git_diff_hunks(
        root,
        &["show", "--format=", "--unified=0", "--no-ext-diff", commit],
    )
}

fn range_hunks(root: &std::path::Path, base: &str, head: &str) -> Option<Vec<ReviewHunk>> {
    let range = format!("{base}...{head}");
    git_diff_hunks(root, &["diff", "--unified=0", "--no-ext-diff", &range])
}

fn git_diff_hunks(root: &std::path::Path, args: &[&str]) -> Option<Vec<ReviewHunk>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut hunks = parse_unified_diff(&String::from_utf8_lossy(&output.stdout));
    hunks.retain(|hunk| !is_koochi_local_file(&hunk.path));
    Some(hunks)
}

fn untracked_file_hunk(root: &std::path::Path, path: &str) -> Option<ReviewHunk> {
    let content = std::fs::read_to_string(root.join(path)).ok()?;
    let lines = content
        .lines()
        .enumerate()
        .map(|(index, line)| ReviewHunkLine {
            kind: ReviewLineKind::Added,
            old_line: None,
            new_line: Some((index + 1) as u32),
            content: line.to_string(),
        })
        .collect::<Vec<_>>();
    Some(ReviewHunk {
        id: String::new(),
        path: path.to_string(),
        old_start: 0,
        old_lines: 0,
        new_start: 1,
        new_lines: lines.len() as u32,
        lines,
    })
}

fn parse_unified_diff(diff: &str) -> Vec<ReviewHunk> {
    let mut hunks = Vec::new();
    let mut current_path: Option<FilePath> = None;
    let mut current_hunk: Option<ReviewHunk> = None;
    let mut old_line = 0_u32;
    let mut new_line = 0_u32;

    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            if let Some(hunk) = current_hunk.take() {
                hunks.push(hunk);
            }
            current_path = None;
            continue;
        }
        if let Some(path) = line.strip_prefix("+++ b/") {
            current_path = Some(path_to_unix(path));
            continue;
        }
        if line.starts_with("+++ /dev/null") {
            current_path = None;
            continue;
        }
        if let Some(header) = line.strip_prefix("@@ ") {
            if let Some(hunk) = current_hunk.take() {
                hunks.push(hunk);
            }
            let Some(path) = current_path.clone() else {
                continue;
            };
            let Some((old_start, old_lines, new_start, new_lines)) = parse_hunk_header(header)
            else {
                continue;
            };
            old_line = old_start;
            new_line = new_start;
            current_hunk = Some(ReviewHunk {
                id: String::new(),
                path,
                old_start,
                old_lines,
                new_start,
                new_lines,
                lines: Vec::new(),
            });
            continue;
        }

        let Some(hunk) = current_hunk.as_mut() else {
            continue;
        };
        if let Some(content) = line.strip_prefix('+') {
            hunk.lines.push(ReviewHunkLine {
                kind: ReviewLineKind::Added,
                old_line: None,
                new_line: Some(new_line),
                content: content.to_string(),
            });
            new_line += 1;
        } else if let Some(content) = line.strip_prefix('-') {
            hunk.lines.push(ReviewHunkLine {
                kind: ReviewLineKind::Removed,
                old_line: Some(old_line),
                new_line: None,
                content: content.to_string(),
            });
            old_line += 1;
        } else if let Some(content) = line.strip_prefix(' ') {
            hunk.lines.push(ReviewHunkLine {
                kind: ReviewLineKind::Context,
                old_line: Some(old_line),
                new_line: Some(new_line),
                content: content.to_string(),
            });
            old_line += 1;
            new_line += 1;
        } else if line == r"\ No newline at end of file" {
            continue;
        }
    }
    if let Some(hunk) = current_hunk {
        hunks.push(hunk);
    }
    renumber_hunks(&mut hunks);
    hunks
}

fn parse_hunk_header(header: &str) -> Option<(u32, u32, u32, u32)> {
    let mut parts = header.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    Some((
        parse_range_start(old)?,
        parse_range_len(old),
        parse_range_start(new)?,
        parse_range_len(new),
    ))
}

fn parse_range_start(value: &str) -> Option<u32> {
    value.split(',').next()?.parse().ok()
}

fn parse_range_len(value: &str) -> u32 {
    value
        .split_once(',')
        .and_then(|(_, len)| len.parse().ok())
        .unwrap_or(1)
}

fn renumber_hunks(hunks: &mut [ReviewHunk]) {
    let mut counts = std::collections::HashMap::<String, usize>::new();
    for hunk in hunks {
        let count = counts.entry(hunk.path.clone()).or_insert(0);
        *count += 1;
        hunk.id = format!("{}#{}", hunk.path, count);
    }
}

fn head_commit_info(root: &std::path::Path) -> Option<CommitInfo> {
    commit_info(root, "HEAD")
}

fn commit_info(root: &std::path::Path, rev: &str) -> Option<CommitInfo> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["log", "-1", "--format=%h%x00%s", rev])
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
    lower
        .rsplit('/')
        .next()
        .is_some_and(|name| name == "koochi.toml")
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
            initial_context_token_budget: 0,
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

        let review = build_review_scope(temp.path(), &ReviewTarget::Auto).unwrap();

        assert_eq!(review.mode, ReviewMode::LocalChanges);
        assert_eq!(review.files, vec!["changed.rs"]);
        assert_eq!(review.hunks.len(), 1);
        assert_eq!(review.hunks[0].path, "changed.rs");
        assert_eq!(review.hunks[0].lines[0].kind, ReviewLineKind::Added);
        assert_eq!(review.hunks[0].lines[0].new_line, Some(1));
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

        let review = build_review_scope(temp.path(), &ReviewTarget::Auto).unwrap();

        assert_eq!(review.mode, ReviewMode::HeadCommit);
        assert_eq!(review.files, vec!["second.rs"]);
        assert_eq!(review.hunks.len(), 1);
        assert_eq!(review.hunks[0].path, "second.rs");
        assert_eq!(review.hunks[0].lines[0].kind, ReviewLineKind::Added);
        assert_eq!(review.hunks[0].lines[0].new_line, Some(1));
    }

    #[test]
    fn review_scope_can_target_explicit_commit() {
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
        let second = git_stdout(temp.path(), ["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();
        fs::write(temp.path().join("third.rs"), "fn third() {}\n").unwrap();
        git(temp.path(), ["add", "."]);
        git(temp.path(), ["commit", "-m", "third"]);

        let review =
            build_review_scope(temp.path(), &ReviewTarget::Commit(second.clone())).unwrap();

        assert_eq!(review.mode, ReviewMode::Commit);
        assert_eq!(review.files, vec!["second.rs"]);
        assert_eq!(review.commit.as_ref().unwrap().subject, "second");
    }

    #[test]
    fn review_scope_can_target_diff_range() {
        let temp = tempfile::tempdir().unwrap();
        if !git(temp.path(), ["init"]) {
            return;
        }
        git(temp.path(), ["config", "user.email", "koochi@example.test"]);
        git(temp.path(), ["config", "user.name", "Koochi"]);
        fs::write(temp.path().join("first.rs"), "fn first() {}\n").unwrap();
        git(temp.path(), ["add", "."]);
        git(temp.path(), ["commit", "-m", "initial"]);
        let base = git_stdout(temp.path(), ["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();
        fs::write(temp.path().join("second.rs"), "fn second() {}\n").unwrap();
        git(temp.path(), ["add", "."]);
        git(temp.path(), ["commit", "-m", "second"]);
        fs::write(temp.path().join("third.rs"), "fn third() {}\n").unwrap();
        git(temp.path(), ["add", "."]);
        git(temp.path(), ["commit", "-m", "third"]);
        let head = git_stdout(temp.path(), ["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();

        let review = build_review_scope(
            temp.path(),
            &ReviewTarget::Range {
                base: base.clone(),
                head: head.clone(),
            },
        )
        .unwrap();

        assert_eq!(
            review.mode,
            ReviewMode::DiffRange {
                base,
                head: head.clone()
            }
        );
        assert_eq!(review.files, vec!["second.rs", "third.rs"]);
        assert_eq!(review.commit.as_ref().unwrap().subject, "third");
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

        let review = build_review_scope(temp.path(), &ReviewTarget::Auto).unwrap();

        assert_eq!(review.mode, ReviewMode::HeadCommit);
        assert_eq!(review.files, vec!["second.rs"]);
    }

    #[test]
    fn review_scope_excludes_tracked_koochi_config_hunks() {
        let temp = tempfile::tempdir().unwrap();
        if !git(temp.path(), ["init"]) {
            return;
        }
        git(temp.path(), ["config", "user.email", "koochi@example.test"]);
        git(temp.path(), ["config", "user.name", "Koochi"]);
        fs::write(temp.path().join("koochi.toml"), "tests = []\n").unwrap();
        fs::write(temp.path().join("lib.rs"), "fn original() {}\n").unwrap();
        git(temp.path(), ["add", "."]);
        git(temp.path(), ["commit", "-m", "initial"]);
        fs::write(temp.path().join("koochi.toml"), "tests = [\"secret\"]\n").unwrap();
        fs::write(temp.path().join("lib.rs"), "fn changed() {}\n").unwrap();

        let review = build_review_scope(temp.path(), &ReviewTarget::Auto).unwrap();

        assert_eq!(review.mode, ReviewMode::LocalChanges);
        assert_eq!(review.files, vec!["lib.rs"]);
        assert_eq!(review.hunks.len(), 1);
        assert_eq!(review.hunks[0].path, "lib.rs");
    }

    #[test]
    fn review_scope_tracks_renamed_local_file_new_path() {
        let temp = tempfile::tempdir().unwrap();
        if !git(temp.path(), ["init"]) {
            return;
        }
        git(temp.path(), ["config", "user.email", "koochi@example.test"]);
        git(temp.path(), ["config", "user.name", "Koochi"]);
        fs::write(temp.path().join("old.rs"), "fn old_name() {}\n").unwrap();
        git(temp.path(), ["add", "."]);
        git(temp.path(), ["commit", "-m", "initial"]);
        fs::rename(temp.path().join("old.rs"), temp.path().join("new.rs")).unwrap();
        fs::write(temp.path().join("new.rs"), "fn new_name() {}\n").unwrap();
        git(temp.path(), ["add", "-A"]);

        let review = build_review_scope(temp.path(), &ReviewTarget::Auto).unwrap();

        assert_eq!(review.mode, ReviewMode::LocalChanges);
        assert_eq!(review.files, vec!["new.rs"]);
        assert!(review.hunks.iter().all(|hunk| hunk.path == "new.rs"));
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

        let review = build_review_scope(&temp.path().join("fixture"), &ReviewTarget::Auto).unwrap();

        assert_eq!(review.mode, ReviewMode::FullRepoFallback);
        assert!(review.files.is_empty());
    }

    #[test]
    fn review_scope_can_target_full_repo_inside_git_repo() {
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

        let review = build_review_scope(temp.path(), &ReviewTarget::FullRepo).unwrap();

        assert_eq!(review.mode, ReviewMode::FullRepo);
        assert!(review.files.is_empty());
        assert!(review.hunks.is_empty());
    }

    #[test]
    fn review_target_options_accept_full_repo_and_reject_mixes() {
        assert_eq!(
            review_target_from_options(None, None, None, true).unwrap(),
            ReviewTarget::FullRepo
        );
        let err = review_target_from_options(Some("HEAD".to_string()), None, None, true)
            .expect_err("full-repo cannot combine with commit");
        assert!(matches!(err, ScopeError::ConflictingFullRepoOptions));
    }

    #[test]
    fn review_scope_errors_when_head_parent_is_missing() {
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
        let head = git_stdout(temp.path(), ["rev-parse", "HEAD"]).unwrap();
        fs::write(temp.path().join(".git/shallow"), head).unwrap();

        let err = build_review_scope(temp.path(), &ReviewTarget::Auto).unwrap_err();

        assert!(matches!(err, ScopeError::MissingHeadParent));
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

    fn git_stdout<const N: usize>(root: &std::path::Path, args: [&str; N]) -> Option<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).to_string())
    }
}
