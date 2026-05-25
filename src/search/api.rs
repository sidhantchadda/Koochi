use super::file_kind::FileKind;
use crate::FilePath;
use crate::scope::ReviewHunk;
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ListFilesRequest {
    pub kind: FileKind,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ListFilesResponse {
    pub files: Vec<FilePath>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ListReviewHunksResponse {
    pub hunks: Vec<ReviewHunk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GetHunkContextRequest {
    pub hunk_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GetHunkContextResponse {
    pub hunk_id: String,
    pub path: FilePath,
    pub start_line: u32,
    pub end_line: u32,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SearchTextRequest {
    pub query: String,
    pub kind: FileKind,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SearchTextResponse {
    pub matches: Vec<TextMatch>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TextMatch {
    pub path: FilePath,
    pub line: u32,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReadFileRequest {
    pub path: FilePath,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ReadFileResponse {
    pub path: FilePath,
    pub content: String,
    pub line_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GetFileContextRequest {
    pub path: FilePath,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GetFileContextResponse {
    pub path: FilePath,
    pub start_line: u32,
    pub end_line: u32,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FindDefinitionsRequest {
    pub symbol: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FindDefinitionsResponse {
    pub definitions: Vec<DefinitionMatch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FindReferencesRequest {
    pub symbol: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FindReferencesResponse {
    pub references: Vec<ReferenceMatch>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DefinitionMatch {
    pub path: FilePath,
    pub line: u32,
    pub kind: SymbolKind,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ReferenceMatch {
    pub path: FilePath,
    pub line: u32,
    pub preview: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Trait,
    Interface,
    Type,
    Variable,
    Constant,
    Module,
    Unknown,
}

#[async_trait]
pub trait CodeSearchApi: Send + Sync {
    type Error;

    async fn list_files(&self, request: ListFilesRequest)
    -> Result<ListFilesResponse, Self::Error>;

    async fn list_review_files(
        &self,
        request: ListFilesRequest,
    ) -> Result<ListFilesResponse, Self::Error>;

    async fn list_review_hunks(&self) -> Result<ListReviewHunksResponse, Self::Error>;

    async fn get_hunk_context(
        &self,
        request: GetHunkContextRequest,
    ) -> Result<GetHunkContextResponse, Self::Error>;

    async fn search_text(
        &self,
        request: SearchTextRequest,
    ) -> Result<SearchTextResponse, Self::Error>;

    async fn read_file(&self, request: ReadFileRequest) -> Result<ReadFileResponse, Self::Error>;

    async fn get_file_context(
        &self,
        request: GetFileContextRequest,
    ) -> Result<GetFileContextResponse, Self::Error>;

    async fn find_definitions(
        &self,
        request: FindDefinitionsRequest,
    ) -> Result<FindDefinitionsResponse, Self::Error>;

    async fn find_references(
        &self,
        request: FindReferencesRequest,
    ) -> Result<FindReferencesResponse, Self::Error>;
}
