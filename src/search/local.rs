use super::api::CodeSearchApi;
use super::api::DefinitionMatch;
use super::api::FindDefinitionsRequest;
use super::api::FindDefinitionsResponse;
use super::api::FindReferencesRequest;
use super::api::FindReferencesResponse;
use super::api::GetFileContextRequest;
use super::api::GetFileContextResponse;
use super::api::GetHunkContextRequest;
use super::api::GetHunkContextResponse;
use super::api::ListFilesRequest;
use super::api::ListFilesResponse;
use super::api::ListReviewHunksResponse;
use super::api::ReadFileRequest;
use super::api::ReadFileResponse;
use super::api::ReferenceMatch;
use super::api::SearchTextRequest;
use super::api::SearchTextResponse;
use super::api::TextMatch;
use super::file_kind::FileKind;
use super::file_kind::kind_matches;
use super::symbols::definition_regexes;
use crate::FilePath;
use crate::scope::GitRevision;
use crate::scope::RepoScope;
use crate::scope::ScopeConfig;
use async_trait::async_trait;
use ignore::WalkBuilder;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use tokio::sync::Mutex;

const CONTEXT_RADIUS: u32 = 40;

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("failed to walk repo: {0}")]
    Walk(ignore::Error),
    #[error("path `{0}` is outside repo scope")]
    OutsideRepo(String),
    #[error("failed to read `{path}`: {source}")]
    Read {
        path: FilePath,
        source: std::io::Error,
    },
    #[error("invalid regex used by heuristic search: {0}")]
    Regex(#[from] regex::Error),
    #[error("unknown review hunk id `{0}`")]
    UnknownHunk(String),
    #[error("git command failed: {0}")]
    Git(String),
    #[error("agent access to Koochi control-plane file `{0}` is not allowed")]
    BlockedControlPlaneFile(String),
}

#[derive(Debug, Clone)]
pub struct LocalSearchSession {
    scope: ScopeConfig,
    cache: Arc<Mutex<SearchCache>>,
    stats: Arc<SearchStats>,
}

#[derive(Debug, Default)]
struct SearchCache {
    files: HashMap<FileKind, Vec<FilePath>>,
    file_locks: HashMap<FileKind, Arc<Mutex<()>>>,
    review_files: HashMap<FileKind, Vec<FilePath>>,
    review_file_locks: HashMap<FileKind, Arc<Mutex<()>>>,
    contents: HashMap<FilePath, ReadFileResponse>,
    read_locks: HashMap<FilePath, Arc<Mutex<()>>>,
    text: HashMap<SearchTextRequest, SearchTextResponse>,
    text_locks: HashMap<SearchTextRequest, Arc<Mutex<()>>>,
    definitions: HashMap<String, FindDefinitionsResponse>,
    definition_locks: HashMap<String, Arc<Mutex<()>>>,
    references: HashMap<String, FindReferencesResponse>,
    reference_locks: HashMap<String, Arc<Mutex<()>>>,
}

#[derive(Debug, Default)]
struct SearchStats {
    list_files_hits: AtomicUsize,
    list_files_misses: AtomicUsize,
    list_review_files_hits: AtomicUsize,
    list_review_files_misses: AtomicUsize,
    read_file_hits: AtomicUsize,
    read_file_misses: AtomicUsize,
    get_hunk_context_calls: AtomicUsize,
    get_file_context_calls: AtomicUsize,
    search_text_hits: AtomicUsize,
    search_text_misses: AtomicUsize,
    definition_hits: AtomicUsize,
    definition_misses: AtomicUsize,
    reference_hits: AtomicUsize,
    reference_misses: AtomicUsize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct SearchStatsSnapshot {
    pub list_files_hits: usize,
    pub list_files_misses: usize,
    pub list_review_files_hits: usize,
    pub list_review_files_misses: usize,
    pub read_file_hits: usize,
    pub read_file_misses: usize,
    pub get_hunk_context_calls: usize,
    pub get_file_context_calls: usize,
    pub search_text_hits: usize,
    pub search_text_misses: usize,
    pub definition_hits: usize,
    pub definition_misses: usize,
    pub reference_hits: usize,
    pub reference_misses: usize,
}

impl LocalSearchSession {
    pub fn new(scope: ScopeConfig) -> Self {
        Self {
            scope,
            cache: Arc::new(Mutex::new(SearchCache::default())),
            stats: Arc::new(SearchStats::default()),
        }
    }

    pub fn scope(&self) -> &ScopeConfig {
        &self.scope
    }

    pub fn stats(&self) -> SearchStatsSnapshot {
        SearchStatsSnapshot {
            list_files_hits: self.stats.list_files_hits.load(Ordering::Relaxed),
            list_files_misses: self.stats.list_files_misses.load(Ordering::Relaxed),
            list_review_files_hits: self.stats.list_review_files_hits.load(Ordering::Relaxed),
            list_review_files_misses: self.stats.list_review_files_misses.load(Ordering::Relaxed),
            read_file_hits: self.stats.read_file_hits.load(Ordering::Relaxed),
            read_file_misses: self.stats.read_file_misses.load(Ordering::Relaxed),
            get_hunk_context_calls: self.stats.get_hunk_context_calls.load(Ordering::Relaxed),
            get_file_context_calls: self.stats.get_file_context_calls.load(Ordering::Relaxed),
            search_text_hits: self.stats.search_text_hits.load(Ordering::Relaxed),
            search_text_misses: self.stats.search_text_misses.load(Ordering::Relaxed),
            definition_hits: self.stats.definition_hits.load(Ordering::Relaxed),
            definition_misses: self.stats.definition_misses.load(Ordering::Relaxed),
            reference_hits: self.stats.reference_hits.load(Ordering::Relaxed),
            reference_misses: self.stats.reference_misses.load(Ordering::Relaxed),
        }
    }
}

#[async_trait]
impl CodeSearchApi for LocalSearchSession {
    type Error = SearchError;

    fn review_mode(&self) -> Option<crate::scope::ReviewMode> {
        Some(self.scope.review.mode.clone())
    }

    async fn list_files(
        &self,
        request: ListFilesRequest,
    ) -> Result<ListFilesResponse, Self::Error> {
        let file_lock = {
            let mut cache = self.cache.lock().await;
            if let Some(files) = cache.files.get(&request.kind).cloned() {
                self.stats.list_files_hits.fetch_add(1, Ordering::Relaxed);
                return Ok(ListFilesResponse { files });
            }
            cache
                .file_locks
                .entry(request.kind)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };

        let _guard = file_lock.lock().await;
        if let Some(files) = self.cache.lock().await.files.get(&request.kind).cloned() {
            self.stats.list_files_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(ListFilesResponse { files });
        }

        self.stats.list_files_misses.fetch_add(1, Ordering::Relaxed);
        let files_result = collect_files_for_repo(&self.scope.primary_repo, request.kind);
        let mut cache = self.cache.lock().await;
        cache.file_locks.remove(&request.kind);
        let files = files_result?;
        cache.files.insert(request.kind, files.clone());
        Ok(ListFilesResponse { files })
    }

    async fn list_review_files(
        &self,
        request: ListFilesRequest,
    ) -> Result<ListFilesResponse, Self::Error> {
        if self.scope.review.files.is_empty() {
            if let Some(files) = self.cache.lock().await.files.get(&request.kind).cloned() {
                self.stats
                    .list_review_files_hits
                    .fetch_add(1, Ordering::Relaxed);
                return Ok(ListFilesResponse { files });
            }
            self.stats
                .list_review_files_misses
                .fetch_add(1, Ordering::Relaxed);
            return self.list_files(request).await;
        }

        let review_file_lock = {
            let mut cache = self.cache.lock().await;
            if let Some(files) = cache.review_files.get(&request.kind).cloned() {
                self.stats
                    .list_review_files_hits
                    .fetch_add(1, Ordering::Relaxed);
                return Ok(ListFilesResponse { files });
            }
            cache
                .review_file_locks
                .entry(request.kind)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };

        let _guard = review_file_lock.lock().await;
        if let Some(files) = self
            .cache
            .lock()
            .await
            .review_files
            .get(&request.kind)
            .cloned()
        {
            self.stats
                .list_review_files_hits
                .fetch_add(1, Ordering::Relaxed);
            return Ok(ListFilesResponse { files });
        }

        self.stats
            .list_review_files_misses
            .fetch_add(1, Ordering::Relaxed);
        let files: Vec<FilePath> = self
            .scope
            .review
            .files
            .iter()
            .filter(|path| !is_agent_hidden_file(path))
            .filter(|path| kind_matches(path, request.kind))
            .filter(|path| file_exists_for_repo(&self.scope.primary_repo, path))
            .cloned()
            .collect();
        let mut cache = self.cache.lock().await;
        cache.review_file_locks.remove(&request.kind);
        cache.review_files.insert(request.kind, files.clone());
        Ok(ListFilesResponse { files })
    }

    async fn list_review_hunks(&self) -> Result<ListReviewHunksResponse, Self::Error> {
        Ok(ListReviewHunksResponse {
            hunks: self
                .scope
                .review
                .hunks
                .iter()
                .filter(|hunk| !is_agent_hidden_file(&hunk.path))
                .cloned()
                .collect(),
        })
    }

    async fn get_hunk_context(
        &self,
        request: GetHunkContextRequest,
    ) -> Result<GetHunkContextResponse, Self::Error> {
        self.stats
            .get_hunk_context_calls
            .fetch_add(1, Ordering::Relaxed);
        let hunk = self
            .scope
            .review
            .hunks
            .iter()
            .filter(|hunk| !is_agent_hidden_file(&hunk.path))
            .find(|hunk| hunk.id == request.hunk_id)
            .cloned()
            .ok_or_else(|| SearchError::UnknownHunk(request.hunk_id.clone()))?;
        let line = hunk
            .lines
            .iter()
            .find_map(|line| line.new_line)
            .or_else(|| hunk.lines.iter().find_map(|line| line.old_line))
            .unwrap_or(hunk.new_start.max(1));
        let context = self
            .get_file_context(GetFileContextRequest {
                path: hunk.path.clone(),
                line,
            })
            .await?;
        Ok(GetHunkContextResponse {
            hunk_id: hunk.id,
            path: context.path,
            start_line: context.start_line,
            end_line: context.end_line,
            content: context.content,
        })
    }

    async fn search_text(
        &self,
        request: SearchTextRequest,
    ) -> Result<SearchTextResponse, Self::Error> {
        let normalized = SearchTextRequest {
            query: request.query.trim().to_string(),
            kind: request.kind,
        };
        if normalized.query.is_empty() {
            return Ok(SearchTextResponse {
                matches: Vec::new(),
            });
        }
        if let Some(response) = self.cache.lock().await.text.get(&normalized).cloned() {
            self.stats.search_text_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(response);
        }
        let text_lock = {
            let mut cache = self.cache.lock().await;
            if let Some(response) = cache.text.get(&normalized).cloned() {
                self.stats.search_text_hits.fetch_add(1, Ordering::Relaxed);
                return Ok(response);
            }
            cache
                .text_locks
                .entry(normalized.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };

        let _guard = text_lock.lock().await;
        if let Some(response) = self.cache.lock().await.text.get(&normalized).cloned() {
            self.stats.search_text_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(response);
        }
        self.stats
            .search_text_misses
            .fetch_add(1, Ordering::Relaxed);

        let response_result: Result<SearchTextResponse, SearchError> = async {
            let files = self
                .list_files(ListFilesRequest {
                    kind: normalized.kind,
                })
                .await?
                .files;
            let mut matches = Vec::new();
            for path in files {
                let file = self.read_file(ReadFileRequest { path }).await?;
                for (index, line) in file.content.lines().enumerate() {
                    if line.contains(&normalized.query) {
                        matches.push(TextMatch {
                            path: file.path.clone(),
                            line: (index + 1) as u32,
                            preview: line.trim().to_string(),
                        });
                    }
                }
            }
            Ok(SearchTextResponse { matches })
        }
        .await;
        let mut cache = self.cache.lock().await;
        cache.text_locks.remove(&normalized);
        let response = response_result?;
        cache.text.insert(normalized, response.clone());
        Ok(response)
    }

    async fn read_file(&self, request: ReadFileRequest) -> Result<ReadFileResponse, Self::Error> {
        let path = normalize_repo_path(&request.path);
        if is_agent_hidden_file(&path) {
            return Err(SearchError::BlockedControlPlaneFile(path));
        }
        let read_lock = {
            let mut cache = self.cache.lock().await;
            if let Some(response) = cache.contents.get(&path).cloned() {
                self.stats.read_file_hits.fetch_add(1, Ordering::Relaxed);
                return Ok(response);
            }
            cache
                .read_locks
                .entry(path.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };

        let _guard = read_lock.lock().await;
        if let Some(response) = self.cache.lock().await.contents.get(&path).cloned() {
            self.stats.read_file_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(response);
        }

        self.stats.read_file_misses.fetch_add(1, Ordering::Relaxed);
        let content = match read_file_for_repo(&self.scope.primary_repo, &path).await {
            Ok(content) => content,
            Err(error) => {
                self.cache.lock().await.read_locks.remove(&path);
                return Err(error);
            }
        };
        let mut cache = self.cache.lock().await;
        cache.read_locks.remove(&path);
        let line_count = content.lines().count() as u32;
        let response = ReadFileResponse {
            path: path.clone(),
            content,
            line_count,
        };
        cache.contents.insert(path.clone(), response.clone());
        Ok(response)
    }

    async fn get_file_context(
        &self,
        request: GetFileContextRequest,
    ) -> Result<GetFileContextResponse, Self::Error> {
        self.stats
            .get_file_context_calls
            .fetch_add(1, Ordering::Relaxed);
        let file = self
            .read_file(ReadFileRequest { path: request.path })
            .await?;
        if file.line_count == 0 {
            return Ok(GetFileContextResponse {
                path: file.path,
                start_line: 0,
                end_line: 0,
                content: String::new(),
            });
        }
        let line = request.line.max(1).min(file.line_count);
        let start_line = line.saturating_sub(CONTEXT_RADIUS).max(1);
        let end_line = (line + CONTEXT_RADIUS).min(file.line_count);
        let content = file
            .content
            .lines()
            .enumerate()
            .filter_map(|(index, text)| {
                let line_no = (index + 1) as u32;
                (line_no >= start_line && line_no <= end_line).then_some(text)
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(GetFileContextResponse {
            path: file.path,
            start_line,
            end_line,
            content,
        })
    }

    async fn find_definitions(
        &self,
        request: FindDefinitionsRequest,
    ) -> Result<FindDefinitionsResponse, Self::Error> {
        let symbol = request.symbol.trim().to_string();
        if let Some(response) = self.cache.lock().await.definitions.get(&symbol).cloned() {
            self.stats.definition_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(response);
        }
        if symbol.is_empty() {
            return Ok(FindDefinitionsResponse {
                definitions: Vec::new(),
            });
        }
        let definition_lock = {
            let mut cache = self.cache.lock().await;
            if let Some(response) = cache.definitions.get(&symbol).cloned() {
                self.stats.definition_hits.fetch_add(1, Ordering::Relaxed);
                return Ok(response);
            }
            cache
                .definition_locks
                .entry(symbol.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };

        let _guard = definition_lock.lock().await;
        if let Some(response) = self.cache.lock().await.definitions.get(&symbol).cloned() {
            self.stats.definition_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(response);
        }
        self.stats.definition_misses.fetch_add(1, Ordering::Relaxed);
        let response_result: Result<FindDefinitionsResponse, SearchError> = async {
            let regexes = definition_regexes(&symbol)?;
            let files = self
                .list_files(ListFilesRequest {
                    kind: FileKind::Source,
                })
                .await?
                .files;
            let mut definitions = Vec::new();
            for path in files {
                let file = self.read_file(ReadFileRequest { path }).await?;
                for (index, line) in file.content.lines().enumerate() {
                    if let Some(kind) = regexes
                        .iter()
                        .find_map(|(kind, regex)| regex.is_match(line).then_some(*kind))
                    {
                        definitions.push(DefinitionMatch {
                            path: file.path.clone(),
                            line: (index + 1) as u32,
                            kind,
                            preview: line.trim().to_string(),
                        });
                    }
                }
            }
            Ok(FindDefinitionsResponse { definitions })
        }
        .await;
        let mut cache = self.cache.lock().await;
        cache.definition_locks.remove(&symbol);
        let response = response_result?;
        cache.definitions.insert(symbol, response.clone());
        Ok(response)
    }

    async fn find_references(
        &self,
        request: FindReferencesRequest,
    ) -> Result<FindReferencesResponse, Self::Error> {
        let symbol = request.symbol.trim().to_string();
        if let Some(response) = self.cache.lock().await.references.get(&symbol).cloned() {
            self.stats.reference_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(response);
        }
        if symbol.is_empty() {
            return Ok(FindReferencesResponse {
                references: Vec::new(),
            });
        }
        let reference_lock = {
            let mut cache = self.cache.lock().await;
            if let Some(response) = cache.references.get(&symbol).cloned() {
                self.stats.reference_hits.fetch_add(1, Ordering::Relaxed);
                return Ok(response);
            }
            cache
                .reference_locks
                .entry(symbol.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };

        let _guard = reference_lock.lock().await;
        if let Some(response) = self.cache.lock().await.references.get(&symbol).cloned() {
            self.stats.reference_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(response);
        }
        self.stats.reference_misses.fetch_add(1, Ordering::Relaxed);
        let response_result: Result<FindReferencesResponse, SearchError> = async {
            let regex = Regex::new(&format!(r"\b{}\b", regex::escape(&symbol)))?;
            let files = self
                .list_files(ListFilesRequest {
                    kind: FileKind::Source,
                })
                .await?
                .files;
            let mut references = Vec::new();
            for path in files {
                let file = self.read_file(ReadFileRequest { path }).await?;
                for (index, line) in file.content.lines().enumerate() {
                    if regex.is_match(line) {
                        references.push(ReferenceMatch {
                            path: file.path.clone(),
                            line: (index + 1) as u32,
                            preview: line.trim().to_string(),
                        });
                    }
                }
            }
            Ok(FindReferencesResponse { references })
        }
        .await;
        let mut cache = self.cache.lock().await;
        cache.reference_locks.remove(&symbol);
        let response = response_result?;
        cache.references.insert(symbol, response.clone());
        Ok(response)
    }
}

fn collect_files(root: &Path, kind: FileKind) -> Result<Vec<FilePath>, SearchError> {
    let mut files = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .filter_entry(|entry| !is_git_metadata(entry.path()))
        .build();
    for entry in walker {
        let entry = entry.map_err(SearchError::Walk)?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| SearchError::OutsideRepo(path.to_string_lossy().to_string()))?;
        let relative = path_to_unix(relative);
        if !is_agent_hidden_file(&relative) && kind_matches(&relative, kind) {
            files.push(relative);
        }
    }
    files.sort();
    Ok(files)
}

fn collect_files_for_repo(repo: &RepoScope, kind: FileKind) -> Result<Vec<FilePath>, SearchError> {
    match &repo.revision {
        GitRevision::Head => collect_files(&repo.root, kind),
        revision => {
            let rev = revision.as_ref();
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo.root)
                .args(["ls-tree", "-r", "--name-only", rev])
                .output()
                .map_err(|source| SearchError::Git(source.to_string()))?;
            if !output.status.success() {
                return Err(SearchError::Git(
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ));
            }
            let mut files = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(str::to_string)
                .filter(|path| !is_agent_hidden_file(path))
                .filter(|path| kind_matches(path, kind))
                .collect::<Vec<_>>();
            files.sort();
            Ok(files)
        }
    }
}

fn file_exists_for_repo(repo: &RepoScope, path: &str) -> bool {
    match &repo.revision {
        GitRevision::Head => repo.root.join(path).is_file(),
        revision => {
            let spec = format!("{}:{path}", revision.as_ref());
            std::process::Command::new("git")
                .arg("-C")
                .arg(&repo.root)
                .args(["cat-file", "-e", &spec])
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        }
    }
}

async fn read_file_for_repo(repo: &RepoScope, path: &str) -> Result<String, SearchError> {
    match &repo.revision {
        GitRevision::Head => {
            let absolute = scoped_path(&repo.root, path)?;
            tokio::fs::read_to_string(&absolute)
                .await
                .map_err(|source| SearchError::Read {
                    path: path.to_string(),
                    source,
                })
        }
        revision => {
            let spec = format!("{}:{path}", revision.as_ref());
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo.root)
                .args(["show", &spec])
                .output()
                .map_err(|source| SearchError::Git(source.to_string()))?;
            if !output.status.success() {
                return Err(SearchError::Git(
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ));
            }
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        }
    }
}

fn normalize_repo_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_string()
}

fn is_agent_hidden_file(path: &str) -> bool {
    let normalized = normalize_repo_path(path).to_ascii_lowercase();
    normalized
        .rsplit('/')
        .next()
        .is_some_and(|name| name == "koochi.toml")
}

fn scoped_path(root: &Path, relative: &str) -> Result<PathBuf, SearchError> {
    let normalized = normalize_repo_path(relative);
    let path = root.join(&normalized);
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(SearchError::OutsideRepo(relative.to_string()));
    }
    Ok(path)
}

fn is_git_metadata(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == ".git")
}

fn path_to_unix(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
#[path = "local_tests.rs"]
mod local_tests;
