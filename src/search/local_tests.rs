use super::*;
use crate::scope::{
    GitRevision, RepoScope, ReviewHunk, ReviewHunkLine, ReviewLineKind, ReviewMode, ReviewScope,
    ScopeConfig,
};
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
            hunks: Vec::new(),
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
async fn file_listing_skips_git_metadata() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join(".git")).unwrap();
    std::fs::write(temp.path().join(".git").join("index"), b"\xff\x00not utf8").unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn real_code() {}\n").unwrap();
    let search = session(temp.path().to_path_buf());

    let files = search
        .list_files(ListFilesRequest {
            kind: FileKind::All,
        })
        .await
        .unwrap();

    assert_eq!(files.files, vec!["lib.rs"]);
}

#[tokio::test]
async fn commit_revision_reads_files_from_git_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    if !git(temp.path(), ["init"]) {
        return;
    }
    git(temp.path(), ["config", "user.email", "koochi@example.test"]);
    git(temp.path(), ["config", "user.name", "Koochi"]);
    std::fs::write(temp.path().join("lib.rs"), "pub fn old_name() {}\n").unwrap();
    git(temp.path(), ["add", "."]);
    git(temp.path(), ["commit", "-m", "old"]);
    let old = git_stdout(temp.path(), ["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    std::fs::write(temp.path().join("lib.rs"), "pub fn new_name() {}\n").unwrap();
    git(temp.path(), ["add", "."]);
    git(temp.path(), ["commit", "-m", "new"]);

    let search = LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "test".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Commit(old),
        },
        review: ReviewScope {
            mode: ReviewMode::Commit,
            files: vec!["lib.rs".to_string()],
            hunks: Vec::new(),
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    });

    let file = search
        .read_file(ReadFileRequest {
            path: "lib.rs".to_string(),
        })
        .await
        .unwrap();

    assert!(file.content.contains("old_name"));
    assert!(!file.content.contains("new_name"));
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
            hunks: Vec::new(),
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
async fn list_review_hunks_uses_review_scope() {
    let temp = tempfile::tempdir().unwrap();
    let search = LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "test".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::LocalChanges,
            files: vec!["changed.rs".to_string()],
            hunks: vec![ReviewHunk {
                id: "changed.rs#1".to_string(),
                path: "changed.rs".to_string(),
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                lines: vec![ReviewHunkLine {
                    kind: ReviewLineKind::Added,
                    old_line: None,
                    new_line: Some(1),
                    content: "fn changed() {}".to_string(),
                }],
            }],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    });

    let hunks = search.list_review_hunks().await.unwrap();
    assert_eq!(hunks.hunks.len(), 1);
    assert_eq!(hunks.hunks[0].id, "changed.rs#1");
    assert_eq!(hunks.hunks[0].lines[0].content, "fn changed() {}");
}

#[tokio::test]
async fn agents_cannot_read_or_discover_koochi_config() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("src")).unwrap();
    std::fs::write(
        temp.path().join("koochi.toml"),
        "secret = \"control-plane\"\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("src/lib.rs"), "pub fn reviewed() {}\n").unwrap();
    let search = LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "test".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::LocalChanges,
            files: vec!["koochi.toml".to_string(), "src/lib.rs".to_string()],
            hunks: vec![
                ReviewHunk {
                    id: "koochi.toml#1".to_string(),
                    path: "koochi.toml".to_string(),
                    old_start: 0,
                    old_lines: 0,
                    new_start: 1,
                    new_lines: 1,
                    lines: vec![ReviewHunkLine {
                        kind: ReviewLineKind::Added,
                        old_line: None,
                        new_line: Some(1),
                        content: "secret = \"control-plane\"".to_string(),
                    }],
                },
                ReviewHunk {
                    id: "src/lib.rs#1".to_string(),
                    path: "src/lib.rs".to_string(),
                    old_start: 0,
                    old_lines: 0,
                    new_start: 1,
                    new_lines: 1,
                    lines: vec![ReviewHunkLine {
                        kind: ReviewLineKind::Added,
                        old_line: None,
                        new_line: Some(1),
                        content: "pub fn reviewed() {}".to_string(),
                    }],
                },
            ],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    });

    let all_files = search
        .list_files(ListFilesRequest {
            kind: FileKind::All,
        })
        .await
        .unwrap();
    assert_eq!(all_files.files, vec!["src/lib.rs"]);

    let config_files = search
        .list_files(ListFilesRequest {
            kind: FileKind::Configs,
        })
        .await
        .unwrap();
    assert!(config_files.files.is_empty());

    let review_files = search
        .list_review_files(ListFilesRequest {
            kind: FileKind::All,
        })
        .await
        .unwrap();
    assert_eq!(review_files.files, vec!["src/lib.rs"]);

    let hunks = search.list_review_hunks().await.unwrap();
    assert_eq!(hunks.hunks.len(), 1);
    assert_eq!(hunks.hunks[0].path, "src/lib.rs");

    let read_error = search
        .read_file(ReadFileRequest {
            path: "koochi.toml".to_string(),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        read_error,
        SearchError::BlockedControlPlaneFile(_)
    ));

    let context_error = search
        .get_file_context(GetFileContextRequest {
            path: "koochi.toml".to_string(),
            line: 1,
        })
        .await
        .unwrap_err();
    assert!(matches!(
        context_error,
        SearchError::BlockedControlPlaneFile(_)
    ));

    let hunk_error = search
        .get_hunk_context(GetHunkContextRequest {
            hunk_id: "koochi.toml#1".to_string(),
        })
        .await
        .unwrap_err();
    assert!(matches!(hunk_error, SearchError::UnknownHunk(_)));

    let matches = search
        .search_text(SearchTextRequest {
            query: "control-plane".to_string(),
            kind: FileKind::All,
        })
        .await
        .unwrap();
    assert!(matches.matches.is_empty());
}

#[tokio::test]
async fn get_hunk_context_returns_context_around_hunk() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("changed.rs"),
        "one\ntwo\nthree\nfour\nfive\n",
    )
    .unwrap();
    let search = LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "test".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::LocalChanges,
            files: vec!["changed.rs".to_string()],
            hunks: vec![ReviewHunk {
                id: "changed.rs#1".to_string(),
                path: "changed.rs".to_string(),
                old_start: 0,
                old_lines: 0,
                new_start: 3,
                new_lines: 1,
                lines: vec![ReviewHunkLine {
                    kind: ReviewLineKind::Added,
                    old_line: None,
                    new_line: Some(3),
                    content: "three".to_string(),
                }],
            }],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    });

    let context = search
        .get_hunk_context(GetHunkContextRequest {
            hunk_id: "changed.rs#1".to_string(),
        })
        .await
        .unwrap();

    assert_eq!(context.hunk_id, "changed.rs#1");
    assert_eq!(context.path, "changed.rs");
    assert_eq!(context.start_line, 1);
    assert_eq!(context.end_line, 5);
    assert!(context.content.contains("three"));
}

#[tokio::test]
async fn get_hunk_context_rejects_unknown_hunk() {
    let temp = tempfile::tempdir().unwrap();
    let search = session(temp.path().to_path_buf());

    let error = search
        .get_hunk_context(GetHunkContextRequest {
            hunk_id: "missing#1".to_string(),
        })
        .await
        .unwrap_err();

    assert_eq!(error.to_string(), "unknown review hunk id `missing#1`");
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
async fn coalesces_concurrent_text_searches() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("a.rs"), "pub fn handler() {}\n").unwrap();
    std::fs::write(temp.path().join("b.rs"), "pub fn handler_two() {}\n").unwrap();
    let search = session(temp.path().to_path_buf());

    let request = SearchTextRequest {
        query: "handler".to_string(),
        kind: FileKind::Source,
    };
    let (first, second, third) = tokio::join!(
        search.search_text(request.clone()),
        search.search_text(request.clone()),
        search.search_text(request)
    );

    assert_eq!(first.unwrap().matches.len(), 2);
    assert_eq!(second.unwrap().matches.len(), 2);
    assert_eq!(third.unwrap().matches.len(), 2);
    let stats = search.stats();
    assert_eq!(stats.search_text_misses, 1);
    assert_eq!(stats.search_text_hits, 2);
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

fn git<const N: usize>(root: &std::path::Path, args: [&str; N]) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn git_stdout<const N: usize>(root: &std::path::Path, args: [&str; N]) -> Option<String> {
    let output = std::process::Command::new("git")
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
