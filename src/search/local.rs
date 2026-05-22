use super::api::CodeSearchApi;
use super::api::DefinitionMatch;
use super::api::FindDefinitionsRequest;
use super::api::FindDefinitionsResponse;
use super::api::FindReferencesRequest;
use super::api::FindReferencesResponse;
use super::api::GetFileContextRequest;
use super::api::GetFileContextResponse;
use super::api::ListFilesRequest;
use super::api::ListFilesResponse;
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
use crate::scope::ScopeConfig;
use async_trait::async_trait;
use ignore::WalkBuilder;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
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
}

#[derive(Debug, Clone)]
pub struct LocalSearchSession {
    scope: ScopeConfig,
    cache: Arc<Mutex<SearchCache>>,
}

#[derive(Debug, Default)]
struct SearchCache {
    files: HashMap<FileKind, Vec<FilePath>>,
    contents: HashMap<FilePath, ReadFileResponse>,
    text: HashMap<SearchTextRequest, SearchTextResponse>,
    definitions: HashMap<String, FindDefinitionsResponse>,
    references: HashMap<String, FindReferencesResponse>,
}

impl LocalSearchSession {
    pub fn new(scope: ScopeConfig) -> Self {
        Self {
            scope,
            cache: Arc::new(Mutex::new(SearchCache::default())),
        }
    }

    pub fn scope(&self) -> &ScopeConfig {
        &self.scope
    }
}

#[async_trait]
impl CodeSearchApi for LocalSearchSession {
    type Error = SearchError;

    async fn list_files(
        &self,
        request: ListFilesRequest,
    ) -> Result<ListFilesResponse, Self::Error> {
        if let Some(files) = self.cache.lock().await.files.get(&request.kind).cloned() {
            return Ok(ListFilesResponse { files });
        }
        let files = collect_files(&self.scope.primary_repo.root, request.kind)?;
        self.cache
            .lock()
            .await
            .files
            .insert(request.kind, files.clone());
        Ok(ListFilesResponse { files })
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
            return Ok(response);
        }

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
        let response = SearchTextResponse { matches };
        self.cache
            .lock()
            .await
            .text
            .insert(normalized, response.clone());
        Ok(response)
    }

    async fn read_file(&self, request: ReadFileRequest) -> Result<ReadFileResponse, Self::Error> {
        let path = normalize_repo_path(&request.path);
        if let Some(response) = self.cache.lock().await.contents.get(&path).cloned() {
            return Ok(response);
        }
        let absolute = scoped_path(&self.scope.primary_repo.root, &path)?;
        let content = tokio::fs::read_to_string(&absolute)
            .await
            .map_err(|source| SearchError::Read {
                path: path.clone(),
                source,
            })?;
        let line_count = content.lines().count() as u32;
        let response = ReadFileResponse {
            path: path.clone(),
            content,
            line_count,
        };
        self.cache
            .lock()
            .await
            .contents
            .insert(path, response.clone());
        Ok(response)
    }

    async fn get_file_context(
        &self,
        request: GetFileContextRequest,
    ) -> Result<GetFileContextResponse, Self::Error> {
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
            return Ok(response);
        }
        if symbol.is_empty() {
            return Ok(FindDefinitionsResponse {
                definitions: Vec::new(),
            });
        }
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
        let response = FindDefinitionsResponse { definitions };
        self.cache
            .lock()
            .await
            .definitions
            .insert(symbol, response.clone());
        Ok(response)
    }

    async fn find_references(
        &self,
        request: FindReferencesRequest,
    ) -> Result<FindReferencesResponse, Self::Error> {
        let symbol = request.symbol.trim().to_string();
        if let Some(response) = self.cache.lock().await.references.get(&symbol).cloned() {
            return Ok(response);
        }
        if symbol.is_empty() {
            return Ok(FindReferencesResponse {
                references: Vec::new(),
            });
        }
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
        let response = FindReferencesResponse { references };
        self.cache
            .lock()
            .await
            .references
            .insert(symbol, response.clone());
        Ok(response)
    }
}

fn collect_files(root: &Path, kind: FileKind) -> Result<Vec<FilePath>, SearchError> {
    let mut files = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
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
        if kind_matches(&relative, kind) {
            files.push(relative);
        }
    }
    files.sort();
    Ok(files)
}

fn normalize_repo_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_string()
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

fn path_to_unix(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::{GitRevision, RepoScope, ScopeConfig};
    use crate::search::SymbolKind;

    fn session(root: PathBuf) -> LocalSearchSession {
        LocalSearchSession::new(ScopeConfig {
            primary_repo: RepoScope {
                repo_id: "test".to_string(),
                root,
                revision: GitRevision::Head,
            },
            accessible_repos: Vec::new(),
            mcp_servers: Vec::new(),
            tools: Vec::new(),
            agents: Vec::new(),
        })
    }

    #[tokio::test]
    async fn searches_and_reads_files() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/lib.rs"), "fn create_payment() {}\n").unwrap();
        let search = session(temp.path().to_path_buf());

        let files = search
            .list_files(ListFilesRequest {
                kind: FileKind::Source,
            })
            .await
            .unwrap();
        assert_eq!(files.files, vec!["src/lib.rs"]);

        let matches = search
            .search_text(SearchTextRequest {
                query: "create_payment".to_string(),
                kind: FileKind::Source,
            })
            .await
            .unwrap();
        assert_eq!(matches.matches[0].line, 1);
    }

    #[tokio::test]
    async fn clamps_file_context() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("a.rs"), "one\ntwo\nthree\n").unwrap();
        let search = session(temp.path().to_path_buf());
        let context = search
            .get_file_context(GetFileContextRequest {
                path: "a.rs".to_string(),
                line: 1,
            })
            .await
            .unwrap();
        assert_eq!(context.start_line, 1);
        assert_eq!(context.end_line, 3);
    }

    #[tokio::test]
    async fn finds_heuristic_definitions_and_references() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("a.rs"),
            "pub fn create_payment() {}\nlet x = create_payment();\n",
        )
        .unwrap();
        let search = session(temp.path().to_path_buf());
        let definitions = search
            .find_definitions(FindDefinitionsRequest {
                symbol: "create_payment".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(definitions.definitions[0].kind, SymbolKind::Function);

        let references = search
            .find_references(FindReferencesRequest {
                symbol: "create_payment".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(references.references.len(), 2);
    }
}
