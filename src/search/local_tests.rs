use super::*;
use crate::scope::{GitRevision, RepoScope, ReviewMode, ReviewScope, ScopeConfig};
use crate::search::SymbolKind;

fn session(root: PathBuf) -> LocalSearchSession {
    LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "test".to_string(),
            root,
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::FullRepoFallback,
            files: Vec::new(),
            commit: None,
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
async fn list_review_files_uses_review_scope_when_present() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("changed.rs"), "fn changed() {}\n").unwrap();
    std::fs::write(temp.path().join("unchanged.rs"), "fn unchanged() {}\n").unwrap();
    let search = LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "test".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::LocalChanges,
            files: vec!["changed.rs".to_string()],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    });

    let files = search
        .list_review_files(ListFilesRequest {
            kind: FileKind::Source,
        })
        .await
        .unwrap();
    assert_eq!(files.files, vec!["changed.rs"]);

    let all_files = search
        .list_files(ListFilesRequest {
            kind: FileKind::Source,
        })
        .await
        .unwrap();
    assert_eq!(all_files.files, vec!["changed.rs", "unchanged.rs"]);
}

#[tokio::test]
async fn coalesces_concurrent_file_reads() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("a.rs"), "pub fn handler() {}\n").unwrap();
    let search = session(temp.path().to_path_buf());

    let (first, second, third) = tokio::join!(
        search.read_file(ReadFileRequest {
            path: "a.rs".to_string()
        }),
        search.read_file(ReadFileRequest {
            path: "a.rs".to_string()
        }),
        search.read_file(ReadFileRequest {
            path: "a.rs".to_string()
        })
    );

    assert_eq!(first.unwrap().content, "pub fn handler() {}\n");
    assert_eq!(second.unwrap().content, "pub fn handler() {}\n");
    assert_eq!(third.unwrap().content, "pub fn handler() {}\n");
    let stats = search.stats();
    assert_eq!(stats.read_file_misses, 1);
    assert_eq!(stats.read_file_hits, 2);
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
